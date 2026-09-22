// Publish only prepared Markdown pages to the separate GitHub Wiki repository.
// No main-repository push, force push, or deletion of unrelated Wiki pages.
import { mkdtemp, readdir, copyFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const project = fileURLToPath(new URL('../', import.meta.url));
const source = fileURLToPath(new URL('../wiki/', import.meta.url));
const checkout = await mkdtemp(join(tmpdir(), 'bjut-wiki-'));
const run = (...args) => {
  const result = spawnSync('git', args, { cwd:checkout, encoding:'utf8', stdio:['ignore','pipe','pipe'] });
  if (result.status !== 0) throw new Error(result.stderr?.trim() || 'Git command failed');
  return result.stdout.trim();
};
try {
  try { run('clone', 'https://github.com/key-zhzr/BJUT-Auto-Login.wiki.git', '.'); }
  catch { throw new Error('无法打开 Wiki 仓库。请确认 GitHub 登录，并在项目 Wiki 中创建首个页面后重试。'); }
  const pages = (await readdir(source)).filter(name => name.endsWith('.md'));
  for (const page of pages) await copyFile(join(source,page), join(checkout,page));
  run('add', '--', ...pages);
  const diff = run('diff', '--cached', '--stat');
  if (!diff) console.log('Wiki already matches the prepared pages.');
  else {
    console.log(diff);
    if (!process.argv.includes('--publish')) console.log('Preview only. Add --publish to commit and publish these Wiki pages.');
    else {
      for (const key of ['user.name', 'user.email']) {
        const identity = spawnSync('git', ['config', '--get', key], {cwd:project, encoding:'utf8'});
        if (identity.status !== 0 || !identity.stdout.trim()) throw new Error(`请先为主仓库设置 ${key}`);
        run('config', key, identity.stdout.trim());
      }
      run('commit','-m','docs: migrate usage guide from project README'); run('push','origin','HEAD'); console.log('Published https://github.com/key-zhzr/BJUT-Auto-Login/wiki'); }
  }
} catch (error) { console.error(error.message); process.exitCode = 1; }
finally { await rm(checkout, {recursive:true,force:true}); }
