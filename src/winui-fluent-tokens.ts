import {
  createDarkTheme,
  createLightTheme,
  type BrandVariants,
  type Theme,
} from '@fluentui/tokens';
import type { AccentColor, AppearanceTheme, ResolvedColorScheme } from './appearance';

const ACCENT_BRAND_RAMPS: Readonly<Record<AccentColor, BrandVariants>> = {
  blue: {
    10: '#041424',
    20: '#062038',
    30: '#082e52',
    40: '#0a3e6e',
    50: '#0a4d8c',
    60: '#0859a6',
    70: '#0067c0',
    80: '#005fb8',
    90: '#2b88d8',
    100: '#60cdff',
    110: '#7cd6ff',
    120: '#99e0ff',
    130: '#b3e8ff',
    140: '#ccefff',
    150: '#e0f5ff',
    160: '#f0faff',
  },
  violet: {
    10: '#160a2e',
    20: '#211044',
    30: '#2f1660',
    40: '#3f1e7d',
    50: '#51289c',
    60: '#6232ba',
    70: '#7240d4',
    80: '#6d39d1',
    90: '#8b63e6',
    100: '#a98bff',
    110: '#bea8ff',
    120: '#cfc0ff',
    130: '#ded3ff',
    140: '#ebe4ff',
    150: '#f3eeff',
    160: '#faf7ff',
  },
  cyan: {
    10: '#031921',
    20: '#052733',
    30: '#07394a',
    40: '#094b61',
    50: '#0a5e78',
    60: '#0b6d8a',
    70: '#0e7490',
    80: '#0b6880',
    90: '#1fa2c4',
    100: '#4fd5f2',
    110: '#77e2f7',
    120: '#9aebfa',
    130: '#b8f2fc',
    140: '#d1f7fd',
    150: '#e5fbfe',
    160: '#f2fdff',
  },
  green: {
    10: '#041a08',
    20: '#072a0e',
    30: '#0b3d14',
    40: '#0e521b',
    50: '#106621',
    60: '#107625',
    70: '#107c10',
    80: '#0e700e',
    90: '#2ea043',
    100: '#5fd36e',
    110: '#7fe28b',
    120: '#9deda7',
    130: '#baf4c1',
    140: '#d4f8d8',
    150: '#e8fce9',
    160: '#f3fef4',
  },
  orange: {
    10: '#241002',
    20: '#381904',
    30: '#522506',
    40: '#6e3308',
    50: '#8a4208',
    60: '#a34e08',
    70: '#b45309',
    80: '#a24a08',
    90: '#e07b24',
    100: '#ffb84d',
    110: '#ffc973',
    120: '#ffd899',
    130: '#ffe4b8',
    140: '#ffefd4',
    150: '#fff6e8',
    160: '#fffaf2',
  },
  rose: {
    10: '#290510',
    20: '#40091a',
    30: '#5c0e26',
    40: '#7a1233',
    50: '#99163f',
    60: '#b5194a',
    70: '#cc1d53',
    80: '#bf174a',
    90: '#eb4976',
    100: '#ff7fa3',
    110: '#ff9eb9',
    120: '#ffb8cc',
    130: '#ffd0dd',
    140: '#ffe3eb',
    150: '#fff0f4',
    160: '#fff7f9',
  },
};

export const WINUI_FLUENT_CSS_VAR_NAMES = [
  '--win-accent-fill-rest',
  '--win-accent-fill-hover',
  '--win-accent-fill-pressed',
  '--win-accent-fill-selected',
  '--win-accent-fill-subtle',
  '--win-accent-fill-subtle-hover',
  '--win-accent-text-primary',
  '--win-accent-text-secondary',
  '--win-accent-text-link',
  '--win-accent-on-fill',
  '--win-accent-stroke',
  '--win-accent-stroke-subtle',
  '--win-status-success-bg',
  '--win-status-success-fg',
  '--win-status-success-border',
  '--win-status-warning-bg',
  '--win-status-warning-fg',
  '--win-status-warning-border',
  '--win-status-danger-bg',
  '--win-status-danger-fg',
  '--win-status-danger-border',
  '--win-status-info-bg',
  '--win-status-info-fg',
  '--win-status-info-border',
  '--win-shadow-card',
  '--win-shadow-flyout',
  '--win-shadow-dialog',
  '--win-ease-enter',
  '--win-ease-exit',
  '--win-ease-standard',
  '--win-duration-fast',
  '--win-duration-normal',
  '--win-duration-slow',
] as const;

type WinuiFluentCssVar = (typeof WINUI_FLUENT_CSS_VAR_NAMES)[number];

const themeCache = new Map<string, Theme>();
const cssVarCache = new Map<string, Record<WinuiFluentCssVar, string>>();
let lastAppliedKey = '';

function getFluentTheme(accent: AccentColor, scheme: ResolvedColorScheme): Theme {
  const key = `${accent}:${scheme}`;
  let theme = themeCache.get(key);
  if (!theme) {
    const brand = ACCENT_BRAND_RAMPS[accent] ?? ACCENT_BRAND_RAMPS.blue;
    theme = scheme === 'dark' ? createDarkTheme(brand) : createLightTheme(brand);
    themeCache.set(key, theme);
  }
  return theme;
}

export function getWinuiFluentCssVars(
  accent: AccentColor,
  scheme: ResolvedColorScheme,
): Readonly<Record<WinuiFluentCssVar, string>> {
  const key = `${accent}:${scheme}`;
  let cached = cssVarCache.get(key);
  if (cached) return cached;

  const t = getFluentTheme(accent, scheme);
  cached = {
    '--win-accent-fill-rest': t.colorBrandBackground,
    '--win-accent-fill-hover': t.colorBrandBackgroundHover,
    '--win-accent-fill-pressed': t.colorBrandBackgroundPressed,
    '--win-accent-fill-selected': t.colorBrandBackgroundSelected,
    '--win-accent-fill-subtle': t.colorBrandBackground2,
    '--win-accent-fill-subtle-hover': t.colorBrandBackground2Hover,
    '--win-accent-text-primary': t.colorBrandForeground1,
    '--win-accent-text-secondary': t.colorBrandForeground2,
    '--win-accent-text-link': t.colorBrandForegroundLink,
    '--win-accent-on-fill': t.colorNeutralForegroundOnBrand,
    '--win-accent-stroke': t.colorBrandStroke1,
    '--win-accent-stroke-subtle': t.colorBrandStroke2,
    '--win-status-success-bg': t.colorStatusSuccessBackground1,
    '--win-status-success-fg': t.colorStatusSuccessForeground1,
    '--win-status-success-border': t.colorStatusSuccessBorder1,
    '--win-status-warning-bg': t.colorStatusWarningBackground1,
    '--win-status-warning-fg': t.colorStatusWarningForeground1,
    '--win-status-warning-border': t.colorStatusWarningBorder1,
    '--win-status-danger-bg': t.colorStatusDangerBackground1,
    '--win-status-danger-fg': t.colorStatusDangerForeground1,
    '--win-status-danger-border': t.colorStatusDangerBorder1,
    '--win-status-info-bg': t.colorNeutralBackground3,
    '--win-status-info-fg': t.colorBrandForeground1,
    '--win-status-info-border': t.colorNeutralStroke2,
    '--win-shadow-card': t.shadow2,
    '--win-shadow-flyout': t.shadow16,
    '--win-shadow-dialog': t.shadow64,
    '--win-ease-enter': t.curveDecelerateMax,
    '--win-ease-exit': t.curveAccelerateMax,
    '--win-ease-standard': t.curveEasyEase,
    '--win-duration-fast': t.durationFaster,
    '--win-duration-normal': t.durationNormal,
    '--win-duration-slow': t.durationGentle,
  };
  cssVarCache.set(key, cached);
  return cached;
}

/**
 * Applies @fluentui/tokens derived CSS variables on :root when WinUI is active,
 * and clears them cleanly when another appearance theme is selected.
 */
export function syncWinuiFluentTokens(
  theme: AppearanceTheme,
  accent: AccentColor,
  scheme: ResolvedColorScheme,
  root?: HTMLElement,
): void {
  const target =
    root
    ?? (typeof document === 'undefined' ? undefined : document.documentElement);
  if (!target) return;

  const nextKey = theme === 'winui' ? `winui:${accent}:${scheme}` : 'none';
  if (nextKey === lastAppliedKey) return;
  lastAppliedKey = nextKey;

  if (theme !== 'winui') {
    for (const name of WINUI_FLUENT_CSS_VAR_NAMES) {
      target.style.removeProperty(name);
    }
    return;
  }

  const vars = getWinuiFluentCssVars(accent, scheme);
  for (const name of WINUI_FLUENT_CSS_VAR_NAMES) {
    target.style.setProperty(name, vars[name]);
  }
}
