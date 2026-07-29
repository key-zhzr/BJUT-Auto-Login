import assert from 'node:assert/strict';

const { isVersionNewer } = await import('../src/update-utils.ts');
const {
  buildWechatPaymentRelayUrl,
  isTrustedWechatLaunchUrl,
} = await import('../src/wechat-payment.ts');
const {
  normalizeAppTheme,
  normalizeAccentColor,
  normalizeAppearanceColorMode,
  observeSystemColorScheme,
  resolveColorScheme,
} = await import('../src/appearance.ts');

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

const paymentUrl = 'weixin://wap/pay?prepayid%3Dwx1234567890&package=123&noncestr=abc123&timestamp=1784697242&sign=BgAAyf6IiX7aEIMn';
assert.equal(isTrustedWechatLaunchUrl(paymentUrl), true);
assert.equal(isTrustedWechatLaunchUrl('weixin://evil/pay?prepayid=wx1'), false);
const relay = new URL(buildWechatPaymentRelayUrl(paymentUrl));
assert.equal(relay.origin, 'https://red.bjutdown.work');
assert.equal(relay.search, '');
assert.equal(decodeURIComponent(relay.hash.slice(1)), paymentUrl);
console.log('WeChat relay regression cases passed');

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
