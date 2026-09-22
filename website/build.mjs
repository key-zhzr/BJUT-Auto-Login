import { mkdir, readFile, writeFile, copyFile, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { marked } from 'marked';
const root = fileURLToPath(new URL('./', import.meta.url));
const output = `${root}dist/`;
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
for (const name of ['index.html','style.css','app.js','_headers']) await copyFile(`${root}${name}`, `${output}${name}`);
await copyFile(`${root}../public/logo.png`, `${output}logo.png`);
const pages = [['Getting-Started','快速开始'],['Features','功能一览'],['Network','网络与 VPN'],['Billing','计费与充值'],['Backup','备份与迁移'],['Android','Android 使用'],['Privacy','隐私与安全'],['Development','开发与发布'],['OpenWrt','OpenWrt 路由端']];
const shell = await readFile(`${root}index.html`, 'utf8');
for (const [slug,title] of pages) {
  const markdown = await readFile(`${root}../wiki/${slug}.md`, 'utf8');
  const content = marked.parse(markdown).replace(/href="(\.\.\/)?([A-Z][A-Za-z-]+)(?:\.md)?(?:#[^"]*)?"/g, (_, _prefix, page) => `href="/guide/${page}/"`);
  const nav = pages.map(([page,label]) => `<a href="/guide/${page}/"${page === slug ? ' class="current" aria-current="page"' : ''}>${label}</a>`).join('');
  let html = shell.replace(/<title>.*?<\/title>/, `<title>${title} · BJUT Auto Login</title>`)
    .replace('https://al.bjutdown.work/"', `https://al.bjutdown.work/guide/${slug}/"`)
    .replace(/<main id="main">[\s\S]*?<\/main>/, `<main id="main" class="guide-layout wrap"><nav class="guide-nav" aria-label="使用指南">${nav}</nav><article class="guide-content">${content}</article></main>`)
    .replace('href="#features"','href="/#features"');
  await mkdir(`${output}guide/${slug}/`, { recursive:true });
  await writeFile(`${output}guide/${slug}/index.html`, html);
}
await writeFile(`${output}404.html`, shell.replace(/<main id="main">[\s\S]*?<\/main>/, '<main id="main" class="wrap features"><h1>页面暂未找到</h1><p>可以回到首页，或查阅使用指南。</p><a class="button primary" href="/">返回首页</a></main>'));
await writeFile(`${output}robots.txt`, 'User-agent: *\nAllow: /\nSitemap: https://al.bjutdown.work/sitemap.xml\n');
await writeFile(`${output}sitemap.xml`, `<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${['',...pages.map(([slug])=>`guide/${slug}/`)].map(path=>`<url><loc>https://al.bjutdown.work/${path}</loc></url>`).join('')}</urlset>`);
console.log(`Built website and ${pages.length} guide pages.`);
