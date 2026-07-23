import { invoke } from '@tauri-apps/api/core';

const userAgent = navigator.userAgent;

export const IS_ANDROID = userAgent.toLowerCase().includes('android');
export const IS_WINDOWS = userAgent.includes('Windows');

export async function writeTextToClipboard(text: string): Promise<void> {
  if (window.AndroidBridge) {
    const copied = window.AndroidBridge.setClipboardText(text);
    if (copied === false) throw new Error('Android 剪贴板写入失败');
  } else if (window.__TAURI__) {
    await invoke('write_clipboard', { text });
  } else {
    await navigator.clipboard.writeText(text);
  }
}

export async function readTextFromClipboard(): Promise<string> {
  if (window.AndroidBridge) {
    return window.AndroidBridge.getClipboardText();
  }
  if (window.__TAURI__) {
    return invoke<string>('read_clipboard');
  }
  return navigator.clipboard.readText();
}
