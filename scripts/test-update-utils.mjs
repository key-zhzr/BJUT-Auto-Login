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
