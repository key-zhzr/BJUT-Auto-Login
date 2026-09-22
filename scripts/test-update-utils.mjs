import assert from 'node:assert/strict';

const { isVersionNewer } = await import('../src/update-utils.ts');
const {
  loadGitHubReleases,
  releaseFromOfficialManifest,
} = await import('../src/update-source.ts');
const {
  buildWechatPaymentRelayUrl,
  createWechatPaymentRelaySession,
  isTrustedWechatLaunchUrl,
} = await import('../src/wechat-payment.ts');
const {
  normalizeAppTheme,
  normalizeAccentColor,
  normalizeAppearanceColorMode,
  observeSystemColorScheme,
  resolveColorScheme,
} = await import('../src/appearance.ts');
const { missingImportedCredentialUsers } = await import('../src/config-backup.ts');

const cases = [
  ['1.0.0-alpha.1', '1.0.0-alpha.beta', true],
  ['1.0.0-alpha.beta', '1.0.0-alpha.1', false],
  ['1.0.0-alpha', '1.0.0-alpha.1', true],
  ['1.0.0-1', '1.0.0-beta', true],
  ['1.0.0', '1.0.0-beta', false],
  ['1.0.0-beta', '1.0.0', true],
  ['1.0.0+build.1', '1.0.0+build.2', false],
  ['1.0.0-01', '1.0.0-2', false],
  ['999999999999999999999.0.0', '1000000000000000000000.0.0', true],
];

for (const [current, latest, expected] of cases) {
  assert.equal(
    isVersionNewer(current, latest),
    expected,
    `${current} -> ${latest}`,
  );
}

console.log(`SemVer regression cases passed: ${cases.length}`);

assert.deepEqual(
  missingImportedCredentialUsers(
    [
      { user: 'saved', pass: '', hasPassword: true },
      { user: 'missing', pass: '', hasPassword: true },
      { user: 'embedded', pass: 'secret', hasPassword: true },
      { user: 'intentionally-empty', pass: '', hasPassword: false },
    ],
    [{ user: 'saved', hasPassword: true }],
  ),
  ['missing'],
);
console.log('Legacy configuration credential guard regression case passed');

const releaseFixture = {
  tag_name: 'v0.1.6',
  name: 'BJUT-Auto-Login v0.1.6',
  body: 'notes',
  html_url: 'https://github.com/key-zhzr/BJUT-Auto-Login/releases/tag/v0.1.6',
  prerelease: false,
  draft: false,
  assets: [],
};
const storageValues = new Map();
const storage = {
  getItem(key) { return storageValues.get(key) ?? null; },
  setItem(key, value) { storageValues.set(key, value); },
};
let apiCalls = 0;
const apiResult = await loadGitHubReleases(10, {
  storage,
  now: 1_000,
  async fetchImpl() {
    apiCalls += 1;
    return new Response(JSON.stringify([releaseFixture]), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  },
});
assert.equal(apiResult.source, 'api');
assert.equal(apiResult.releases[0].tag_name, 'v0.1.6');
const cachedResult = await loadGitHubReleases(10, {
  storage,
  now: 2_000,
  async fetchImpl() {
    throw new Error('fresh cache should avoid a network request');
  },
});
assert.equal(cachedResult.source, 'cache');
assert.equal(apiCalls, 1);
const staleResult = await loadGitHubReleases(10, {
  storage,
  now: 31 * 60 * 1_000,
  async fetchImpl() {
    return new Response('rate limited', {
      status: 403,
      headers: {
        'x-ratelimit-remaining': '0',
        'x-ratelimit-reset': '9999999999',
      },
    });
  },
});
assert.equal(staleResult.source, 'stale-cache');
assert.match(staleResult.warning, /已使用上次成功读取的发布信息/);

const syntheticRelease = releaseFromOfficialManifest({
  version: '0.1.6',
  notes: 'manifest notes',
  platforms: {},
}, {
  platform: 'android',
  arch: 'arm64',
  format: 'apk',
  currentVersion: '0.1.5',
}, 'https://github.com/key-zhzr/BJUT-Auto-Login/releases/latest/download/latest.json');
assert.equal(syntheticRelease.tag_name, 'v0.1.6');
assert.equal(syntheticRelease.assets[0].name, 'BJUT-Auto-Login_0.1.6_Android_arm64.apk');
assert.equal(syntheticRelease.assets[1].name, 'latest.json');
assert.equal(syntheticRelease.assets[0].size, 0); // Legacy manifests need a header lookup.
const manifestAsset = syntheticRelease.assets[0];
const fromSizedManifest = (asset) => releaseFromOfficialManifest({
  version: '0.1.6', platforms: {}, assets: [asset],
}, { platform: 'android', arch: 'arm64', format: 'apk', currentVersion: '0.1.5' }, 'https://github.com/key-zhzr/BJUT-Auto-Login/releases/latest/download/latest.json').assets[0].size;
assert.equal(fromSizedManifest({ ...manifestAsset, size: 12_345_678 }), 12_345_678);
assert.equal(fromSizedManifest({ ...manifestAsset, size: -1 }), 0);
assert.equal(fromSizedManifest({ ...manifestAsset, size: '12345678' }), 0);
assert.equal(fromSizedManifest({ ...manifestAsset, size: 12_345_678, name: 'different.apk' }), 0);
assert.equal(fromSizedManifest({ ...manifestAsset, size: 12_345_678, browser_download_url: 'https://example.com/package.apk' }), 0);
console.log('GitHub release fallback regression cases passed');

const paymentUrl = 'weixin://wap/pay?prepayid%3Dwx1234567890&package=123&noncestr=abc123&timestamp=1784697242&sign=BgAAyf6IiX7aEIMn';
assert.equal(isTrustedWechatLaunchUrl(paymentUrl), true);
assert.equal(
  isTrustedWechatLaunchUrl('weixin://wap/pay?prepayid%3Dwx1234567890%26package%3DWAP%26noncestr%3Dabc123%26sign%3DBgAAyf6IiX7aEIMn'),
  true,
);
assert.equal(
  isTrustedWechatLaunchUrl('weixin://wap/pay?prepayid=wx1234567890&package=Sign%3DWXPay&noncestr=abc123&sign=BgAAyf6IiX7aEIMn%2B%2F%3D'),
  true,
);
assert.equal(isTrustedWechatLaunchUrl('weixin://evil/pay?prepayid=wx1'), false);
assert.equal(isTrustedWechatLaunchUrl('weixin://wap/pay?prepayid=wx123&package=WAP'), false);
assert.equal(isTrustedWechatLaunchUrl('weixin://wap/pay?prepayid=wx123&package=WAP&sign=abc%22def'), false);
const relay = new URL(buildWechatPaymentRelayUrl(paymentUrl));
assert.equal(relay.origin, 'https://red.bjutdown.work');
assert.equal(relay.search, '');
assert.equal(decodeURIComponent(relay.hash.slice(1)), paymentUrl);
console.log('WeChat relay regression cases passed');

const relaySession = await createWechatPaymentRelaySession(paymentUrl, async (_url, init) => {
  const submitted = JSON.parse(String(init.body));
  assert.equal(submitted.launchUrl, paymentUrl);
  return new Response(JSON.stringify({
    token: 'abcdefghijklmnopqrstuvwxyzABCDEFG_1234567890',
    expiresIn: 300,
  }), { status: 200, headers: { 'Content-Type': 'application/json' } });
});
const relaySessionUrl = new URL(relaySession.url);
assert.equal(relaySessionUrl.origin, 'https://red.bjutdown.work');
assert.match(relaySessionUrl.pathname, /^\/p\/[0-9A-Za-z_-]+$/);
assert.equal(relaySessionUrl.search, '');
assert.equal(relaySessionUrl.hash, '');

assert.equal(normalizeAppTheme('Apple OS 26'), 'apple27');
assert.equal(normalizeAppTheme('windows'), 'winui');
assert.equal(normalizeAppTheme('unsupported-theme'), 'basic');
assert.equal(normalizeAccentColor('orange'), 'orange');
assert.equal(normalizeAccentColor('unsupported-accent'), 'blue');
assert.equal(normalizeAppearanceColorMode('auto'), 'system');
assert.equal(normalizeAppearanceColorMode('unsupported-mode'), 'system');
assert.equal(resolveColorScheme('system', true), 'dark');
assert.equal(resolveColorScheme('system', false), 'light');
assert.equal(resolveColorScheme('light', true), 'light');

let legacyColorSchemeListener;
let legacyColorSchemeResult = '';
let legacyListenerRemoved = false;
const stopLegacyColorSchemeObserver = observeSystemColorScheme(
  scheme => {
    legacyColorSchemeResult = scheme;
  },
  () => ({
    matches: true,
    addListener(listener) {
      legacyColorSchemeListener = listener;
    },
    removeListener(listener) {
      legacyListenerRemoved = listener === legacyColorSchemeListener;
    },
  }),
);
assert.equal(typeof legacyColorSchemeListener, 'function');
legacyColorSchemeListener();
assert.equal(legacyColorSchemeResult, 'dark');
stopLegacyColorSchemeObserver();
assert.equal(legacyListenerRemoved, true);

let partialWebViewListener;
let partialWebViewResult = '';
observeSystemColorScheme(
  scheme => {
    partialWebViewResult = scheme;
  },
  () => ({
    matches: false,
    addEventListener() {
      throw new Error('MediaQueryList EventTarget is unavailable');
    },
    addListener(listener) {
      partialWebViewListener = listener;
    },
  }),
);
assert.equal(typeof partialWebViewListener, 'function');
partialWebViewListener();
assert.equal(partialWebViewResult, 'light');
console.log('Appearance normalization regression cases passed');

const { compactFamilyLabel } = await import('../src/network-experience.ts');
for (const [status, label] of [['not_configured', '未配置'], ['dns_error', 'DNS 失败'], ['tls_error', 'TLS 失败'], ['timeout', '超时']]) {
  assert.equal(compactFamilyLabel({ status, durationMs: 0, addresses: [], detail: '' }), label);
}
assert.equal(compactFamilyLabel({ status: 'reachable', durationMs: 38.4, addresses: [], detail: '' }), '38 ms');
console.log('Network diagnostic label regression cases passed');

const { encodeConfigQr, ConfigQrCollector } = await import('../src/config-qr.ts');
const payload = JSON.stringify({version:3,ciphertext:'encrypted-fixture'.repeat(600)});
const frames = await encodeConfigQr(payload);
const collector = new ConfigQrCollector();
await collector.add(frames[0]);
assert.equal((await collector.add(frames[0])).received, 1);
let assembled;
for (const frame of frames.slice(1).reverse()) assembled = await collector.add(frame);
assert.equal(assembled.payload, payload);
const other = await encodeConfigQr(JSON.stringify({version:3,ciphertext:'different'}));
await assert.rejects(()=>collector.add(other[0]), /同一份/);
await assert.rejects(()=>new ConfigQrCollector().add('https:\/\/example.com'), /不是配置二维码/);
await assert.rejects(()=>encodeConfigQr('x'.repeat(128001)), /太大/);
const damaged = [...other]; damaged[0] = damaged[0].slice(0,-1)+'x';
await assert.rejects(()=>new ConfigQrCollector().add(damaged[0]), /校验失败/);
let time = 1000; const expired = new ConfigQrCollector(()=>time); await expired.add(frames[0]); time += 600001;
await assert.rejects(()=>expired.add(frames[1]), /超时/);
console.log('Encrypted QR framing, duplicate, mixed and expiry cases passed');
const { rechargeServiceIsOpen } = await import('../src/recharge-hours.ts');
for (const [hour, minute, open] of [[5,59,false],[6,0,true],[22,59,true],[23,0,false],[0,0,false]]) {
  assert.equal(rechargeServiceIsOpen(Date.UTC(2026,8,21,hour-8,minute)),open);
}
console.log('Beijing recharge-hours boundaries passed');

const {default:QR} = await import('qrcode');
const {default:jsQR} = await import('jsqr');
for (const text of frames) {
  const code=QR.create(text,{errorCorrectionLevel:'L'});
  const n=code.modules.size,scale=4,margin=4,size=(n+margin*2)*scale;
  const pixels=new Uint8ClampedArray(size*size*4).fill(255);
  for(let y=0;y<n;y++)for(let x=0;x<n;x++)if(code.modules.get(y,x))for(let dy=0;dy<scale;dy++)for(let dx=0;dx<scale;dx++) {
    const at=(((y+margin)*scale+dy)*size+(x+margin)*scale+dx)*4;
    pixels[at]=pixels[at+1]=pixels[at+2]=0;
  }
  assert.equal(jsQR(pixels,size,size)?.data,text);
}
console.log('Every generated QR frame decodes back to its encrypted content');
