/*
 * Runs before the stylesheets paint so the loading mask uses the same theme as
 * the persisted app selection. Keep the value maps in sync with appearance.ts.
 */
(() => {
  'use strict';
  const root = document.documentElement;
  const themeAliases = {
    basic: 'basic',
    default: 'basic',
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
  const modeAliases = {
    system: 'system',
    auto: 'system',
    follow: 'system',
    dark: 'dark',
    night: 'dark',
    light: 'light',
    day: 'light',
  };
  const accents = {
    blue: ['#3b82f6', '#2563eb', '59, 130, 246', '#ffffff'],
    violet: ['#8b7cf6', '#7565e8', '139, 124, 246', '#ffffff'],
    cyan: ['#06b6d4', '#0891b2', '6, 182, 212', '#08111f'],
    green: ['#22c55e', '#16a34a', '34, 197, 94', '#08111f'],
    orange: ['#f59e0b', '#d97706', '245, 158, 11', '#08111f'],
    rose: ['#f43f5e', '#e11d48', '244, 63, 94', '#ffffff'],
  };

  let theme = 'basic';
  let mode = 'system';
  let accent = 'blue';
  try {
    const storedTheme = (localStorage.getItem('bjut_theme') || '').trim().toLowerCase();
    const storedMode = (localStorage.getItem('bjut_color_mode') || '').trim().toLowerCase();
    const storedAccent = (localStorage.getItem('bjut_accent_color') || '').trim().toLowerCase();
    theme = themeAliases[storedTheme] || 'basic';
    mode = modeAliases[storedMode] || 'system';
    accent = accents[storedAccent] ? storedAccent : 'blue';
  } catch (_) {
    // Preferences are optional; the defaults still prevent a missing palette.
  }

  let scheme = mode;
  if (mode === 'system') {
    try {
      scheme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    } catch (_) {
      scheme = 'light';
    }
  }

  root.dataset.theme = theme;
  root.dataset.colorMode = mode;
  root.dataset.colorScheme = scheme;
  root.dataset.accent = accent;
  root.style.colorScheme = scheme;

  const accentValues = accents[accent];
  root.style.setProperty('--accent-color', accentValues[0]);
  root.style.setProperty('--accent-hover', accentValues[1]);
  root.style.setProperty('--accent-rgb', accentValues[2]);
  root.style.setProperty('--accent-contrast', accentValues[3]);
  root.style.setProperty('--primary-color', accentValues[0]);
  root.style.setProperty('--primary-hover', accentValues[1]);
  root.style.setProperty('--primary-rgb', accentValues[2]);

  const meta = document.querySelector('meta[name="theme-color"]');
  const lightBackground = theme === 'apple27' ? '#f2f2f7' : theme === 'winui' ? '#f3f3f3' : '#eef2f7';
  const darkBackground = theme === 'basic' ? '#0f172a' : '#000000';
  if (meta) meta.content = scheme === 'dark' ? darkBackground : lightBackground;
})();
