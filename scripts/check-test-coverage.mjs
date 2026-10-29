#!/usr/bin/env node
// Front-end test entry guard.
//
// Source of truth: package.json scripts["test:frontend"].
// Every scripts/test-*.mjs on disk must be referenced by that command, and
// every referenced file must exist. Either mismatch exits with code 1.
//
// Two existing invocation styles are tolerated:
//   1) node --experimental-strip-types scripts/test-update-utils.mjs  (top-level self-run)
//   2) node --experimental-strip-types --test a.mjs b.mjs c.mjs       (node:test batch)
// This guard only compares the file set; it does not care which style is used.
//
// Usage: node scripts/check-test-coverage.mjs
// Exit: 0 when consistent, 1 when a file is unregistered or dangling.

import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const manifest = JSON.parse(readFileSync(join(repoRoot, 'package.json'), 'utf8'));
const command = manifest.scripts?.['test:frontend'] ?? '';

const referenced = new Set(
  [...command.matchAll(/(?:^|\s)(scripts\/test-[^\s&|]+\.mjs)/g)].map((match) => match[1]),
);
const onDisk = new Set(
  readdirSync(join(repoRoot, 'scripts'))
    .filter((name) => /^test-.*\.mjs$/.test(name))
    .map((name) => `scripts/${name}`),
);

const unregistered = [...onDisk].filter((file) => !referenced.has(file)).sort();
const dangling = [...referenced].filter((file) => !onDisk.has(file)).sort();

if (unregistered.length || dangling.length) {
  for (const file of unregistered) {
    console.error(`漏注册：${file} 存在但未出现在 scripts["test:frontend"]`);
  }
  for (const file of dangling) {
    console.error(`失效引用：${file} 被 scripts["test:frontend"] 引用但文件不存在`);
  }
  process.exit(1);
}
console.log(`前端测试入口完整：${onDisk.size} 个 scripts/test-*.mjs 全部注册`);
