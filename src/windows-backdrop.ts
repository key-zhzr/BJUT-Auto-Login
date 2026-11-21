import { invoke } from '@tauri-apps/api/core';
import type { AppearanceTheme, ResolvedColorScheme } from './appearance';
import { IS_WINDOWS } from './platform';

interface WindowsBackdropStatus {
  supported: boolean;
  active: boolean;
}

/**
 * Syncs native Windows 11 DWM Mica on the Tauri window when WinUI is active,
 * clearing it cleanly when switching back to another appearance.
 */
export async function syncWindowsBackdrop(
  theme: AppearanceTheme,
  colorScheme: ResolvedColorScheme,
): Promise<boolean> {
  if (typeof document === 'undefined') return false;
  if (!window.__TAURI__ || !IS_WINDOWS) {
    document.body.classList.remove('has-native-system-backdrop');
    return false;
  }

  try {
    const status = await invoke<WindowsBackdropStatus>('sync_windows_backdrop', {
      theme,
      colorScheme,
    });
    const active = theme === 'winui' && Boolean(status?.active);
    document.body.classList.toggle('has-native-system-backdrop', active);
    return active;
  } catch {
    document.body.classList.remove('has-native-system-backdrop');
    return false;
  }
}
