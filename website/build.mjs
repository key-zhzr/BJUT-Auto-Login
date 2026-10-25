import { build } from 'vite';
import { releaseCatalog, openwrtCatalog } from './downloads.js';
import { downloadCards, openwrtMatrixMarkup } from './render-downloads.js';
import { renderGuidePage, checkSiteLinks } from './guide-utils.js';
import { mkdir, readFile, writeFile, copyFile, cp, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('./', import.meta.url));
const output = `${root}dist/`;
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
for (const name of ['style.css', 'app.js', 'downloads.js', 'render-downloads.js', 'guide.js', '_headers']) {
  await copyFile(`${root}${name}`, `${output}${name}`);
}
await copyFile(`${root}../public/logo.png`, `${output}logo.png`);

const pages = [
  ['Getting-Started', '快速开始'],
  ['Features', '功能一览'],
  ['Network', '网络与 VPN'],
  ['Billing', '计费与充值'],
  ['Backup', '备份与迁移'],
  ['Android', 'Android 使用'],
  ['Privacy', '隐私与安全'],
  ['Development', '开发与发布'],
  ['OpenWrt', 'OpenWrt 路由端'],
];
const validSlugs = pages.map(([slug]) => slug);

const release = JSON.parse(await readFile(`${root}release.json`, 'utf8'));
const openwrtTargets = JSON.parse(await readFile(`${root}openwrt-targets.json`, 'utf8'));
const catalog = releaseCatalog(release);
if (!catalog) throw new Error('Missing valid public installer metadata');
const openwrt = openwrtCatalog(null, openwrtTargets);

await writeFile(
  `${output}release-data.js`,
  `export const openwrtTargets = ${JSON.stringify(openwrtTargets)};\nexport default ${JSON.stringify(release)};\n`,
);
await cp(`${root}icons`, `${output}icons`, { recursive: true });

const shell = (await readFile(`${root}index.html`, 'utf8'))
  .replace('<!-- DOWNLOAD_CARDS -->', downloadCards(catalog))
  .replace('<!-- RELEASE_VERSION -->', `正式版 ${catalog.tag}`)
  .replace('<!-- OPENWRT_VERSION -->', `最新构建标签 ${openwrt.publishedTag} · 源码修订 ${openwrt.packageVersion}-r${openwrt.sourceRevision}`)
  .replace('<!-- OPENWRT_MATRIX -->', openwrtMatrixMarkup(openwrt));
await writeFile(`${output}index.html`, shell);

await build({
  configFile: false,
  root: `${root}preview`,
  base: '/preview/',
  publicDir: false,
  build: { outDir: `${output}preview`, emptyOutDir: true },
  logLevel: 'warn',
});

const guideIndex = [];
const pagesMap = new Map([['/', shell]]);

for (const [slug, title] of pages) {
  const markdown = await readFile(`${root}../wiki/${slug}.md`, 'utf8');
  const { content, tocHtml, sections } = renderGuidePage(markdown, { slug, title, validSlugs });
  guideIndex.push(...sections);
  const navLinks = pages.map(([page, label]) => `<a href="/guide/${page}/"${page === slug ? ' class="current" aria-current="page"' : ''}>${label}</a>`).join('');
  const searchBox = `<div class="guide-search"><label for="guide-search-input" class="sr-only">搜索使用指南</label><input id="guide-search-input" type="search" placeholder="搜索指南章节…" autocomplete="off" aria-controls="guide-search-results"><div id="guide-search-results" class="guide-search-results" role="region" aria-label="指南搜索结果" aria-live="polite" hidden></div></div>`;
  const pageTitle = `${title} · BJUT Auto Login`;
  const html = shell
    .replace(/<title>.*?<\/title>/, `<title>${pageTitle}</title>`)
    .replace('<meta property="og:type" content="website">', '<meta property="og:type" content="article">')
    .replace('<meta property="og:title" content="BJUT Auto Login · 校园网连接助手">', `<meta property="og:title" content="${pageTitle}">`)
    .replace('<meta name="twitter:title" content="BJUT Auto Login · 校园网连接助手">', `<meta name="twitter:title" content="${pageTitle}">`)
    .replace('<meta property="og:url" content="https://al.bjutdown.work/">', `<meta property="og:url" content="https://al.bjutdown.work/guide/${slug}/">`)
    .replace('<link rel="canonical" href="https://al.bjutdown.work/">', `<link rel="canonical" href="https://al.bjutdown.work/guide/${slug}/">`)
    .replace(/<main id="main">[\s\S]*?<\/main>/, `<main id="main" class="guide-layout wrap"><nav class="guide-nav" aria-label="使用指南">${searchBox}<div class="guide-nav-links">${navLinks}</div></nav><article class="guide-content">${content}</article>${tocHtml}</main>`)
    .replace('href="#features"', 'href="/#features"')
    .replace('href="#download"', 'href="/#download"')
    .replace('<script type="module" src="/app.js"></script>', '<script type="module" src="/guide.js"></script>');
  await mkdir(`${output}guide/${slug}/`, { recursive: true });
  await writeFile(`${output}guide/${slug}/index.html`, html);
  pagesMap.set(`/guide/${slug}/`, html);
}

await writeFile(`${output}guide-index.json`, JSON.stringify(guideIndex) + '\n');

const notFoundHtml = shell
  .replace(/<title>.*?<\/title>/, '<title>页面暂未找到 · BJUT Auto Login</title>')
  .replace('<meta property="og:title" content="BJUT Auto Login · 校园网连接助手">', '<meta property="og:title" content="页面暂未找到 · BJUT Auto Login">')
  .replace('<meta name="twitter:title" content="BJUT Auto Login · 校园网连接助手">', '<meta name="twitter:title" content="页面暂未找到 · BJUT Auto Login">')
  .replace('href="#features"', 'href="/#features"')
  .replace('href="#download"', 'href="/#download"')
  .replace('<script type="module" src="/app.js"></script>', '')
  .replace(/<main id="main">[\s\S]*?<\/main>/, '<main id="main" class="wrap features"><h1>页面暂未找到</h1><p>可以回到首页，或查阅使用指南。</p><a class="button primary" href="/">返回首页</a></main>');
await writeFile(`${output}404.html`, notFoundHtml);
pagesMap.set('/404.html', notFoundHtml);

const staticPaths = new Set([
  '/logo.png',
  '/style.css',
  '/app.js',
  '/downloads.js',
  '/render-downloads.js',
  '/guide.js',
  '/guide-index.json',
  '/preview/index.html',
  '/icons/windows.svg',
  '/icons/apple.svg',
  '/icons/linux.svg',
  '/icons/android.svg',
]);

const linkCheck = checkSiteLinks(pagesMap, staticPaths);
if (!linkCheck.ok) {
  throw new Error(`Site link verification failed:\n${linkCheck.errors.join('\n')}`);
}

await writeFile(`${output}robots.txt`, 'User-agent: *\nAllow: /\nSitemap: https://al.bjutdown.work/sitemap.xml\n');
await writeFile(`${output}sitemap.xml`, `<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${['', ...pages.map(([slug]) => `guide/${slug}/`)].map(path => `<url><loc>https://al.bjutdown.work/${path}</loc></url>`).join('')}</urlset>`);
console.log(`Built website and ${pages.length} guide pages; all internal links and anchors verified.`);
