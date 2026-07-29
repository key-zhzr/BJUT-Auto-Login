export type AppearanceTheme = 'basic' | 'apple27' | 'winui';
export type AppTheme = AppearanceTheme;
export type AppearanceColorMode = 'system' | 'dark' | 'light';
export type ResolvedColorScheme = 'dark' | 'light';

export type AccentColor =
  | 'blue'
  | 'violet'
  | 'cyan'
  | 'green'
  | 'orange'
  | 'rose';

export interface AppearanceOption<T extends string> {
  value: T;
  label: string;
  description: string;
}

export interface AccentColorOption extends AppearanceOption<AccentColor> {
  color: string;
  hover: string;
  rgb: string;
  contrast: '#ffffff' | '#08111f';
}

export interface AppearanceSelection {
  theme: AppearanceTheme;
  accent: AccentColor;
  colorMode: AppearanceColorMode;
  colorScheme: ResolvedColorScheme;
}

export const DEFAULT_APPEARANCE_THEME: AppearanceTheme = 'basic';
export const DEFAULT_ACCENT_COLOR: AccentColor = 'blue';
export const DEFAULT_COLOR_MODE: AppearanceColorMode = 'system';

export const APPEARANCE_THEME_OPTIONS: readonly AppearanceOption<AppearanceTheme>[] = [
  {
    value: 'basic',
    label: 'Basic',
    description: '沿用 BJUT-AL 原有视觉，并适配系统深色与浅色外观。',
  },
  {
    value: 'apple27',
    label: 'Apple OS 27',
    description: '内容使用标准材质，导航与控件采用自适应 Liquid Glass。',
  },
  {
    value: 'winui',
    label: 'WinUI',
    description: 'Mica 作为窗口底层，Acrylic 只用于菜单与临时浮层。',
  },
] as const;

export const APPEARANCE_COLOR_MODE_OPTIONS: readonly AppearanceOption<AppearanceColorMode>[] = [
  {
    value: 'system',
    label: '跟随系统',
    description: '随系统外观自动切换深色与浅色。',
  },
  {
    value: 'dark',
    label: '深色',
    description: '始终使用深色界面。',
  },
  {
    value: 'light',
    label: '浅色',
    description: '始终使用浅色界面。',
  },
] as const;

export const ACCENT_COLOR_OPTIONS: readonly AccentColorOption[] = [
  {
    value: 'blue',
    label: '蓝色',
    description: '清晰、稳定的默认强调色。',
    color: '#3b82f6',
    hover: '#2563eb',
    rgb: '59, 130, 246',
    contrast: '#ffffff',
  },
  {
    value: 'violet',
    label: '紫罗兰',
    description: '更柔和的冷色强调。',
    color: '#8b7cf6',
    hover: '#7565e8',
    rgb: '139, 124, 246',
    contrast: '#ffffff',
  },
  {
    value: 'cyan',
    label: '青色',
    description: '明快的科技感强调。',
    color: '#06b6d4',
    hover: '#0891b2',
    rgb: '6, 182, 212',
    contrast: '#08111f',
  },
  {
    value: 'green',
    label: '绿色',
    description: '自然、低干扰的强调色。',
    color: '#22c55e',
    hover: '#16a34a',
    rgb: '34, 197, 94',
    contrast: '#08111f',
  },
  {
    value: 'orange',
    label: '橙色',
    description: '温暖且醒目的强调色。',
    color: '#f59e0b',
    hover: '#d97706',
    rgb: '245, 158, 11',
    contrast: '#08111f',
  },
  {
    value: 'rose',
    label: '玫红',
    description: '鲜明而不过度刺眼的强调色。',
    color: '#f43f5e',
    hover: '#e11d48',
    rgb: '244, 63, 94',
    contrast: '#ffffff',
  },
] as const;

const THEME_ALIASES: Readonly<Record<string, AppearanceTheme>> = {
  basic: 'basic',
  default: 'basic',
  // Apple OS 26 was removed. Migrate existing selections to the maintained
  // Apple appearance instead of unexpectedly falling back to Basic.
  apple26: 'apple27',
  'apple-26': 'apple27',
  'apple-os-26': 'apple27',
  'apple os 26': 'apple27',
  apple27: 'apple27',
  'apple-27': 'apple27',
  'apple-os-27': 'apple27',
  'apple os 27': 'apple27',
  winui: 'winui',
  windows: 'winui',
  'windows-ui': 'winui',
};

const COLOR_MODE_ALIASES: Readonly<Record<string, AppearanceColorMode>> = {
  system: 'system',
  auto: 'system',
  follow: 'system',
  dark: 'dark',
  night: 'dark',
  light: 'light',
  day: 'light',
};

const ACCENT_BY_VALUE = new Map<AccentColor, AccentColorOption>(
  ACCENT_COLOR_OPTIONS.map((option) => [option.value, option]),
);

export function normalizeAppearanceTheme(value: unknown): AppearanceTheme {
  if (typeof value !== 'string') return DEFAULT_APPEARANCE_THEME;
  return THEME_ALIASES[value.trim().toLowerCase()] ?? DEFAULT_APPEARANCE_THEME;
}

export const normalizeAppTheme = normalizeAppearanceTheme;

export function normalizeAppearanceColorMode(value: unknown): AppearanceColorMode {
  if (typeof value !== 'string') return DEFAULT_COLOR_MODE;
  return COLOR_MODE_ALIASES[value.trim().toLowerCase()] ?? DEFAULT_COLOR_MODE;
}

export function normalizeAccentColor(value: unknown): AccentColor {
  if (typeof value !== 'string') return DEFAULT_ACCENT_COLOR;
  const normalized = value.trim().toLowerCase() as AccentColor;
  return ACCENT_BY_VALUE.has(normalized) ? normalized : DEFAULT_ACCENT_COLOR;
}

export function applyAppearanceTheme(
  value: unknown,
  root?: HTMLElement,
): AppearanceTheme {
  const theme = normalizeAppearanceTheme(value);
  const target =
    root ??
    (typeof document === 'undefined' ? undefined : document.documentElement);
  if (target) target.dataset.theme = theme;
  return theme;
}

export function applyAccentColor(
  value: unknown,
  root?: HTMLElement,
): AccentColor {
  const accent = normalizeAccentColor(value);
  const option = ACCENT_BY_VALUE.get(accent) ?? ACCENT_COLOR_OPTIONS[0];
  const target =
    root ??
    (typeof document === 'undefined' ? undefined : document.documentElement);

  if (target) {
    target.dataset.accent = accent;
    target.style.setProperty('--accent-color', option.color);
    target.style.setProperty('--accent-hover', option.hover);
    target.style.setProperty('--accent-rgb', option.rgb);
    target.style.setProperty('--accent-contrast', option.contrast);

    // Keep the existing stylesheet API working while newer rules use
    // accent-specific variables directly.
    target.style.setProperty('--primary-color', option.color);
    target.style.setProperty('--primary-hover', option.hover);
    target.style.setProperty('--primary-rgb', option.rgb);
  }

  return accent;
}

export function resolveColorScheme(
  modeValue: unknown,
  systemPrefersDark?: boolean,
): ResolvedColorScheme {
  const mode = normalizeAppearanceColorMode(modeValue);
  if (mode === 'dark' || mode === 'light') return mode;
  let prefersDark = systemPrefersDark ?? false;
  if (
    systemPrefersDark === undefined
    && typeof window !== 'undefined'
    && typeof window.matchMedia === 'function'
  ) {
    try {
      prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    } catch {
      prefersDark = false;
    }
  }
  return prefersDark ? 'dark' : 'light';
}

interface ColorSchemeMediaQuery {
  matches: boolean;
  addEventListener?: (type: 'change', listener: () => void) => void;
  removeEventListener?: (type: 'change', listener: () => void) => void;
  addListener?: (listener: () => void) => void;
  removeListener?: (listener: () => void) => void;
}

export function observeSystemColorScheme(
  callback: (scheme: ResolvedColorScheme) => void,
  matchMediaFactory?: (query: string) => ColorSchemeMediaQuery,
): () => void {
  const factory = matchMediaFactory
    ?? (typeof window !== 'undefined' && typeof window.matchMedia === 'function'
      ? (query: string) => window.matchMedia(query)
      : undefined);
  if (!factory) return () => {};

  let mediaQuery: ColorSchemeMediaQuery;
  try {
    mediaQuery = factory('(prefers-color-scheme: dark)');
  } catch {
    return () => {};
  }
  if (!mediaQuery || typeof mediaQuery.matches !== 'boolean') return () => {};

  const listener = () => callback(mediaQuery.matches ? 'dark' : 'light');
  if (typeof mediaQuery.addEventListener === 'function') {
    try {
      mediaQuery.addEventListener('change', listener);
      return () => mediaQuery.removeEventListener?.('change', listener);
    } catch {
      // Some Android WebViews expose EventTarget methods but throw when they
      // are used on MediaQueryList. Fall through to the legacy listener API.
    }
  }
  if (typeof mediaQuery.addListener === 'function') {
    // Android System WebView versions predating MediaQueryList EventTarget
    // support expose only the legacy listener API.
    try {
      mediaQuery.addListener(listener);
      return () => mediaQuery.removeListener?.(listener);
    } catch {
      return () => {};
    }
  }
  return () => {};
}

export function applyColorMode(
  value: unknown,
  root?: HTMLElement,
): { mode: AppearanceColorMode; scheme: ResolvedColorScheme } {
  const mode = normalizeAppearanceColorMode(value);
  const scheme = resolveColorScheme(mode);
  const target =
    root ??
    (typeof document === 'undefined' ? undefined : document.documentElement);

  if (target) {
    target.dataset.colorMode = mode;
    target.dataset.colorScheme = scheme;
    target.style.colorScheme = scheme;
  }

  return { mode, scheme };
}

function updateThemeColorMeta(root?: HTMLElement) {
  if (
    !root
    || typeof document === 'undefined'
    || typeof getComputedStyle !== 'function'
  ) return;
  const meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
  if (!meta) return;
  const themeBackground = getComputedStyle(root).getPropertyValue('--theme-bg').trim();
  if (themeBackground) meta.content = themeBackground;
}

export function applyAppearance(
  themeValue: unknown,
  accentValue: unknown,
  colorModeValue: unknown = DEFAULT_COLOR_MODE,
  root?: HTMLElement,
): AppearanceSelection {
  const target =
    root ??
    (typeof document === 'undefined' ? undefined : document.documentElement);
  const theme = applyAppearanceTheme(themeValue, target);
  const accent = applyAccentColor(accentValue, target);
  const { mode, scheme } = applyColorMode(colorModeValue, target);
  updateThemeColorMeta(target);
  return {
    theme,
    accent,
    colorMode: mode,
    colorScheme: scheme,
  };
}
