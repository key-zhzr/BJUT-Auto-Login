export type AppearanceTheme = 'basic' | 'apple26' | 'apple27' | 'winui';
export type AppTheme = AppearanceTheme;

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
}

export const DEFAULT_APPEARANCE_THEME: AppearanceTheme = 'basic';
export const DEFAULT_ACCENT_COLOR: AccentColor = 'blue';

export const APPEARANCE_THEME_OPTIONS: readonly AppearanceOption<AppearanceTheme>[] = [
  {
    value: 'basic',
    label: 'Basic',
    description: '沿用 BJUT-AL 当前的深色半透明界面。',
  },
  {
    value: 'apple26',
    label: 'Apple OS 26',
    description: '圆润、明亮且层次清晰的轻量玻璃材质。',
  },
  {
    value: 'apple27',
    label: 'Apple OS 27',
    description: '更深的空间层次、柔和高光与悬浮感。',
  },
  {
    value: 'winui',
    label: 'WinUI',
    description: '紧凑的 Mica/Acrylic 表面与清晰边界。',
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
  apple26: 'apple26',
  'apple-26': 'apple26',
  'apple-os-26': 'apple26',
  'apple os 26': 'apple26',
  apple27: 'apple27',
  'apple-27': 'apple27',
  'apple-os-27': 'apple27',
  'apple os 27': 'apple27',
  winui: 'winui',
  windows: 'winui',
  'windows-ui': 'winui',
};

const ACCENT_BY_VALUE = new Map<AccentColor, AccentColorOption>(
  ACCENT_COLOR_OPTIONS.map((option) => [option.value, option]),
);

export function normalizeAppearanceTheme(value: unknown): AppearanceTheme {
  if (typeof value !== 'string') return DEFAULT_APPEARANCE_THEME;
  return THEME_ALIASES[value.trim().toLowerCase()] ?? DEFAULT_APPEARANCE_THEME;
}

export const normalizeAppTheme = normalizeAppearanceTheme;

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

export function applyAppearance(
  themeValue: unknown,
  accentValue: unknown,
  root?: HTMLElement,
): AppearanceSelection {
  return {
    theme: applyAppearanceTheme(themeValue, root),
    accent: applyAccentColor(accentValue, root),
  };
}
