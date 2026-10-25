import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import {
  detectDevice,
  readDevice,
  selectDeviceArch,
  releaseCatalog,
  newestCatalog,
  releaseChannels,
  recommendDownload,
  openwrtMatrix,
  parseSha256Sums,
  openwrtCatalog,
} from './downloads.js';
import { downloadCards, openwrtMatrixMarkup } from './render-downloads.js';
import {
  slugifyHeading,
  rewriteGuideLinks,
  renderGuidePage,
  checkSiteLinks,
} from './guide-utils.js';
import { searchGuideIndex } from './guide.js';

const release = JSON.parse(await readFile(new URL('./release.json', import.meta.url), 'utf8'));
const openwrtTargets = JSON.parse(await readFile(new URL('./openwrt-targets.json', import.meta.url), 'utf8'));
const catalog = releaseCatalog(release);

test('only public, exact installer URLs enter the catalog', () => {
  assert.equal(catalog.assets.length, release.assets.filter(asset => /\.(exe|dmg|deb|AppImage|apk)$/.test(asset.name)).length);
  assert.ok(catalog.assets.every(asset => !asset.name.endsWith('.sig') && !asset.name.endsWith('.tar.gz')));
  for (const url of [
    'https://example.com/file.exe',
    'https://github.com/other/repo/releases/download/v0.1.5/file.exe',
    release.assets[0].browser_download_url + '?redirect=other',
  ]) {
    assert.equal(releaseCatalog({ ...release, assets: [{ ...release.assets[0], browser_download_url: url }] }), null);
  }
  assert.equal(releaseCatalog({ ...release, draft: true }), null);
  assert.equal(releaseCatalog({ ...release, prerelease: true }), null);
});

test('release selection does not downgrade or mistake 0.1.9 for 0.1.10', () => {
  const atVersion = v => ({
    ...release,
    tag_name: `v${v}`,
    assets: release.assets.map(a => ({
      ...a,
      name: a.name.replaceAll(catalog.version, v),
      browser_download_url: a.browser_download_url.replaceAll(catalog.version, v),
    })),
  });
  assert.equal(newestCatalog([atVersion('0.1.9'), atVersion('0.1.10')], catalog).version, '0.1.10');
  assert.equal(newestCatalog([atVersion('0.1.4')], catalog).version, catalog.version);
  assert.equal(newestCatalog([{ ...atVersion('9.0.0'), prerelease: true }], catalog).version, catalog.version);
  assert.equal(newestCatalog({}, catalog).version, catalog.version);
});

const devices = [
  ['Windows ARM hints override reduced x64 UA', { ua: 'Mozilla Windows NT 10.0; Win64; x64', hints: { platform: 'Windows', architecture: 'arm', bitness: '64' } }, 'windows', 'arm64'],
  ['Windows x64', { ua: 'Windows NT 10.0; Win64; x64' }, 'windows', 'x64'],
  ['Windows WOW64 browser', { hints: { platform: 'Windows', architecture: 'x86', bitness: '32', wow64: true } }, 'windows', 'x64'],
  ['Windows 32-bit', { hints: { platform: 'Windows', architecture: 'x86', bitness: '32' } }, 'windows', 'x86'],
  ['Mac ARM hints override Intel UA', { ua: 'Macintosh; Intel Mac OS X 10_15_7', platform: 'MacIntel', hints: { platform: 'macOS', architecture: 'arm', bitness: '64' } }, 'macos', 'arm64'],
  ['Safari Intel UA is ambiguous', { ua: 'Macintosh; Intel Mac OS X 10_15_7', platform: 'MacIntel' }, 'macos', null],
  ['Mac Apple silicon renderer', { ua: 'Macintosh; Intel Mac OS X', renderer: 'ANGLE (Apple, ANGLE Metal Renderer: Apple M4 Pro, Unspecified Version)' }, 'macos', 'arm64'],
  ['Mac generic renderer is ambiguous', { ua: 'Macintosh; Intel Mac OS X', renderer: 'Apple GPU' }, 'macos', null],
  ['Mac Intel renderer', { ua: 'Macintosh; Intel Mac OS X', renderer: 'Intel Iris Plus Graphics' }, 'macos', 'x64'],
  ['iPad desktop mode', { ua: 'Macintosh; Intel Mac OS X 10_15_7', platform: 'MacIntel', touches: 5 }, 'ios', null],
  ['iPhone', { ua: 'iPhone CPU iPhone OS 18_0 like Mac OS X' }, 'ios', null],
  ['Android legacy platform does not prove a 32-bit OS', { ua: 'Linux; Android 10; K', platform: 'Linux armv8l' }, 'android', 'arm64'],
  ['Android native ARM64', { ua: 'Linux; Android 10; K', hints: { platform: 'Android', architecture: 'arm', bitness: '64' } }, 'android', 'arm64'],
  ['Android emulator', { ua: 'Linux; Android 13; x86_64' }, 'android', 'x64'],
  ['Android ambiguous', { ua: 'Linux; Android 10; K' }, 'android', null],
  ['Linux ARM', { ua: 'X11; Linux aarch64', platform: 'Linux aarch64' }, 'linux', 'arm64'],
  ['Linux x64', { ua: 'X11; Linux x86_64' }, 'linux', 'x64'],
  ['ChromeOS is not native Linux', { ua: 'X11; CrOS x86_64 14541' }, 'chromeos', null],
  ['Unknown', { ua: 'unknown' }, null, null],
];
for (const [label, input, platform, arch] of devices) {
  test(label, () => assert.deepEqual({ platform: detectDevice(input).platform, arch: detectDevice(input).arch }, { platform, arch }));
}

test('recommendations respect architecture and never choose signatures/offline packages', () => {
  for (const platform of ['windows', 'macos', 'android']) {
    for (const arch of ['arm64', 'x64']) {
      const { asset } = recommendDownload({ platform, arch, certain: true }, catalog);
      assert.equal(asset.platform, platform);
      assert.equal(asset.arch, arch);
      assert.equal(asset.offline, false);
    }
  }
  for (const device of [{ platform: 'linux', arch: 'arm64' }, { platform: 'windows', arch: 'x86' }, { platform: 'ios' }, { platform: null }]) {
    assert.equal(recommendDownload(device, catalog).asset, null);
  }
  assert.match(recommendDownload({ platform: 'macos', arch: null, certain: false }, catalog).note, /芯片未确认/);
});

test('denied client hints fall back without preventing download', async () => {
  const device = await readDevice({ userAgent: 'Windows NT 10.0; Win64; x64', userAgentData: { getHighEntropyValues: async () => { throw new Error('denied'); } } });
  assert.equal(recommendDownload(device, catalog).asset.id, 'windows-x64-exe');
});

test('non-resolving client hints have a bounded wait', async () => {
  const start = Date.now();
  const device = await readDevice({ userAgent: 'Android 10', userAgentData: { platform: 'Android', getHighEntropyValues: () => new Promise(() => {}) } });
  assert.ok(Date.now() - start < 1500);
  assert.equal(recommendDownload(device, catalog).asset.id, 'android-arm64-apk');
});

test('static fallback offers every installer as a direct link with architecture', () => {
  const html = downloadCards(catalog);
  assert.equal((html.match(/class="download-option"/g) || []).length, catalog.assets.length);
  assert.ok(!html.includes('releases/latest'));
  for (const asset of catalog.assets) assert.ok(html.includes(asset.url));
  assert.match(html, /Apple 芯片/);
  assert.match(html, /ARM64/);
  assert.match(html, /离线安装包/);
});

test('stable follows GitHub Latest while prereleases stay in their own channel', () => {
  const version = (v, pre = false, date = '2026-10-25T00:00:00Z') => ({
    ...release,
    prerelease: pre,
    published_at: date,
    tag_name: `v${v}`,
    assets: release.assets.map(a => ({
      ...a,
      name: a.name.replaceAll(catalog.version, v),
      browser_download_url: a.browser_download_url.replaceAll(catalog.version, v),
    })),
  });
  const channels = releaseChannels([version('0.1.7'), version('0.1.6'), version('0.2.0-beta.1', true)], catalog, 'v0.1.6');
  assert.equal(channels.release.tag, 'v0.1.6');
  assert.equal(channels.prerelease.tag, 'v0.2.0-beta.1');
  assert.ok(channels.prerelease.assets.every(a => a.url.includes('/v0.2.0-beta.1/')));
  assert.equal(releaseChannels([{ ...version('0.2.0', true), draft: true }], catalog).prerelease, null);
  assert.equal(releaseChannels([], catalog).release.tag, catalog.tag);
  // Passing a GitHub /releases/latest object directly also works
  const byObject = releaseChannels([version('0.1.7')], catalog, version('0.1.6'));
  assert.equal(byObject.release.tag, 'v0.1.6');
});

test('Via/WebView ARMv8 and x86 recommendations stay honest about missing bitness and support manual switching', async () => {
  const device = await readDevice({
    userAgent: 'Mozilla/5.0 (Linux; Android 15; K) AppleWebKit/537.36 Version/4.0 Chrome/130 Mobile Safari/537.36 Via',
    platform: 'Linux armv8l',
  });
  assert.equal(device.arch, 'arm64');
  assert.equal(device.certain, false);
  const recommendation = recommendDownload(device, catalog);
  assert.equal(recommendation.asset.arch, 'arm64');
  assert.match(recommendation.note, /浏览器未提供精确位数/);
  assert.match(recommendation.note, /推荐普通手机/);

  // Via in Desktop Mode on an ARM phone (spoofing X11; Linux x86_64 while platform is Linux armv8l)
  const viaDesktopSpoof = detectDevice({
    ua: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Via',
    platform: 'Linux armv8l',
    touches: 5,
  });
  assert.equal(viaDesktopSpoof.platform, 'android');
  assert.equal(viaDesktopSpoof.arch, 'arm64');
  assert.equal(viaDesktopSpoof.certain, false);

  // Android x86 environment without explicit 64-bit proof
  const x86WebView = detectDevice({ ua: 'Mozilla/5.0 (Linux; Android 13; K) Via', platform: 'Linux i686' });
  assert.equal(x86WebView.platform, 'android');
  assert.equal(x86WebView.arch, 'x64');
  assert.equal(x86WebView.certain, false);
  const x86Rec = recommendDownload(x86WebView, catalog);
  assert.equal(x86Rec.asset.arch, 'x64');
  assert.match(x86Rec.note, /浏览器未提供精确位数/);

  // Confirmed x86_64 and aarch64
  const emulator = detectDevice({ ua: 'Android 13; x86_64; Via' });
  assert.equal(emulator.arch, 'x64');
  assert.equal(emulator.certain, true);
  const hint = detectDevice({ ua: 'Android', hints: { architecture: 'aarch64' } });
  assert.equal(hint.arch, 'arm64');
  assert.equal(hint.certain, true);

  // Manual architecture switching between auto, arm64, and x64
  const manualX64 = selectDeviceArch(device, 'x64');
  assert.equal(manualX64.arch, 'x64');
  assert.equal(manualX64.manual, true);
  const manualX64Rec = recommendDownload(manualX64, catalog);
  assert.equal(manualX64Rec.asset.id, 'android-x64-apk');
  assert.match(manualX64Rec.note, /已手动选择 x86_64/);

  const manualArm64 = selectDeviceArch(x86WebView, 'arm64');
  const manualArm64Rec = recommendDownload(manualArm64, catalog);
  assert.equal(manualArm64Rec.asset.id, 'android-arm64-apk');
  assert.match(manualArm64Rec.note, /已手动选择 ARM64/);

  const resetAuto = selectDeviceArch(device, '');
  assert.equal(resetAuto.certain, false);
  assert.equal(resetAuto.manual, false);
});

test('OpenWrt APK/IPK download matrix, checksums, and non-fabricated fallback metadata', async () => {
  try {
    const routerTargetsRaw = await readFile(new URL('../openwrt-ready.local/openwrt/build-targets.json', import.meta.url), 'utf8');
    const routerTargets = JSON.parse(routerTargetsRaw);
    assert.deepEqual(
      openwrtTargets.releases.map(({ version, format }) => ({ version, format })),
      routerTargets.releases,
    );
    assert.deepEqual(
      openwrtTargets.platforms.map(({ target, arch, formats }) => (formats ? { target, arch, formats } : { target, arch })),
      routerTargets.platforms,
    );
  } catch {
    // openwrt-ready.local is optional outside the local workspace
  }
  const matrix = openwrtMatrix(openwrtTargets);
  assert.equal(matrix.platforms.length, 10);
  assert.equal(matrix.rows.length, 19);
  assert.equal(matrix.rows.filter(r => r.format === 'apk').length, 10);
  assert.equal(matrix.rows.filter(r => r.format === 'ipk').length, 9);
  assert.equal(matrix.rows.filter(r => r.target === 'qualcommax/ipq60xx').length, 1);
  assert.equal(matrix.rows.find(r => r.target === 'qualcommax/ipq60xx').format, 'apk');

  // Fallback mode: only verifiedBuilds have files/SHA-256; matrix-only rows never fabricate URLs or hashes
  const fallback = openwrtCatalog(null, openwrtTargets);
  const filogicApk = fallback.rows.find(r => r.id === 'openwrt-mediatek-filogic-25.12.5-apk');
  assert.equal(filogicApk.files.length, 2);
  assert.equal(filogicApk.files[0].sha256, '22c4a9d8bd15f075be71a4e50cf564463c099499f304d468c00602025879e6d7');
  assert.equal(filogicApk.latestBuild.packageRevision, '0.1.6-r18');
  assert.equal(filogicApk.latestBuild.files[0].sha256, 'eb955d529b31a1aeb8a694390c5fe3b1dcddc52cb6f8965201ece7f7bd2b6f06');

  for (const row of fallback.rows.filter(r => r.id !== 'openwrt-mediatek-filogic-25.12.5-apk')) {
    assert.equal(row.status, 'matrix-only');
    assert.deepEqual(row.files, []);
  }

  // Live GitHub release enrichment + SHA256SUMS validation
  const coreName = 'openwrt-x86-64-24.10.8-ipk__bjut-autologin_0.1.6-r13_x86_64.ipk';
  const luciName = 'openwrt-x86-64-24.10.8-ipk__luci-app-bjut-autologin_0.1.6-r13_all.ipk';
  const coreSha = '83e8f04758c8e16ad248fbf5cd5239ba936a688cbe85edbb5d281f19956428f3';
  const luciSha = '6c839066165f9b34377b04b9c3a2c33278e022a220c86013021cac95e217ef90';
  const liveRelease = {
    tag_name: 'openwrt-v0.1.6-r13',
    draft: false,
    published_at: '2026-10-22T08:00:00Z',
    assets: [
      {
        name: coreName,
        size: 61200,
        browser_download_url: `https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/releases/download/openwrt-v0.1.6-r13/${coreName}`,
      },
      {
        name: luciName,
        size: 23400,
        browser_download_url: `https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/releases/download/openwrt-v0.1.6-r13/${luciName}`,
      },
    ],
  };
  const liveCatalog = openwrtCatalog(liveRelease, openwrtTargets, {
    checksums: `${coreSha}  ${coreName}\n${luciSha}  ${luciName}\n`,
  });
  const x86Ipk = liveCatalog.rows.find(r => r.id === 'openwrt-x86-64-24.10.8-ipk');
  assert.equal(x86Ipk.status, 'release');
  assert.equal(x86Ipk.files[0].sha256, coreSha);
  assert.equal(x86Ipk.files[1].sha256, luciSha);

  // Rendering and filtering
  const allHtml = openwrtMatrixMarkup(fallback);
  assert.match(allHtml, /qualcommax\/ipq60xx/);
  assert.match(allHtml, /OpenWrt 24\.10\.8 无官方 IPQ60xx SDK/);
  const ipkOnlyHtml = openwrtMatrixMarkup(fallback, { formatFilter: 'ipk' });
  assert.ok(!ipkOnlyHtml.includes('qualcommax/ipq60xx'));
  const filteredHtml = openwrtMatrixMarkup(fallback, { query: 'mt7621' });
  assert.match(filteredHtml, /ramips\/mt7621/);
  assert.ok(!filteredHtml.includes('bcm27xx/bcm2711'));
  assert.equal(parseSha256Sums('invalid-hash  file.apk').size, 0);
});

test('guide heading IDs, cross-page anchors, TOC, search, code copy, and link checker', async () => {
  const validSlugs = ['Getting-Started', 'Features', 'Network', 'Billing', 'Backup', 'Android', 'Privacy', 'Development', 'OpenWrt'];
  const sampleMd = [
    '# 示例指南',
    '参见 [网络诊断](Network#查看诊断) 与 [隐私说明](../Privacy.md#网络请求与兼容策略) 及 [首页](Home#download)。',
    '## 安装步骤',
    '```sh',
    'npm run site:build',
    '```',
    '## 安装步骤',
  ].join('\n');

  const rendered = renderGuidePage(sampleMd, { slug: 'Getting-Started', title: '快速开始', validSlugs });
  assert.match(rendered.content, /<h1 id="示例指南">/);
  assert.match(rendered.content, /<h2 id="安装步骤">/);
  assert.match(rendered.content, /<h2 id="安装步骤-1">/);
  assert.match(rendered.content, /href="\/guide\/Network\/#查看诊断"/);
  assert.match(rendered.content, /href="\/guide\/Privacy\/#网络请求与兼容策略"/);
  assert.match(rendered.content, /href="\/#download"/);
  assert.match(rendered.content, /data-copy-code/);
  assert.match(rendered.tocHtml, /href="#安装步骤"/);
  assert.match(rendered.tocHtml, /href="#安装步骤-1"/);

  const searchHits = searchGuideIndex(rendered.sections, 'site:build');
  assert.equal(searchHits.length, 1);
  assert.equal(searchHits[0].id, '安装步骤');

  // Negative test for link checker catching missing cross-page anchor and unrewritten .md links
  const brokenCheck = checkSiteLinks(new Map([
    ['/', '<main id="main"><a href="/guide/Network/#不存在的锚点">坏锚点</a><a href="Relative.md">未重写</a></main>'],
    ['/guide/Network/', '<main id="main"><h1 id="网络与-vpn">网络</h1></main>'],
  ]));
  assert.equal(brokenCheck.ok, false);
  assert.equal(brokenCheck.errors.length, 2);

  // Verify outdated version clarifications in wiki/OpenWrt.md and website/README.md
  const openwrtWiki = await readFile(new URL('../wiki/OpenWrt.md', import.meta.url), 'utf8');
  assert.match(openwrtWiki, /0\.3\.0/);
  assert.match(openwrtWiki, /0\.1\.6-r18/);
  assert.match(openwrtWiki, /已过期/);
  const siteReadme = await readFile(new URL('./README.md', import.meta.url), 'utf8');
  assert.match(siteReadme, /Latest Release/);
  assert.match(siteReadme, /Pre-release/);
});

test('homepage and preview include dark mode adaptation, accessibility, and OG/Twitter metadata', async () => {
  const indexHtml = await readFile(new URL('./index.html', import.meta.url), 'utf8');
  const styleCss = await readFile(new URL('./style.css', import.meta.url), 'utf8');
  const previewMain = await readFile(new URL('./preview/main.ts', import.meta.url), 'utf8');

  assert.match(indexHtml, /property="og:title"/);
  assert.match(indexHtml, /property="og:description"/);
  assert.match(indexHtml, /property="og:image"/);
  assert.match(indexHtml, /name="twitter:card"/);
  assert.match(indexHtml, /name="twitter:title"/);
  assert.match(indexHtml, /class="skip" href="#main"/);
  assert.match(indexHtml, /\/preview\/index\.html\?layout=desktop/);
  assert.match(indexHtml, /\/preview\/index\.html\?layout=mobile/);
  assert.match(styleCss, /@media\s*\(prefers-color-scheme:\s*dark\)/);
  assert.match(styleCss, /@media\s*\(prefers-reduced-motion:\s*reduce\)/);
  assert.match(previewMain, /observeSystemColorScheme/);
});
