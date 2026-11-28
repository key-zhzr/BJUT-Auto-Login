declare module '*?raw' {
  const content: string;
  export default content;
}

interface AndroidBridgeApi {
  takeQuickAction?(): string | null;
  updateWidgetStatus?(status: string): void;
  setClipboardText(text: string): boolean;
  getClipboardText(): string;
  refreshNotificationSettings?(): void;
  getAutoLoginPausedUntil?(): number | string;
  requestForegroundPermissions(): void;
  requestBackgroundPermissions(): void;
  requestBatteryOptimizations(): void;
  startKeepAliveService(): void;
  stopKeepAliveService(): void;
  exitApplication?(): void;
  getPermissionHealth?(): string;
  openPermissionSettings?(permission: string): void;
  clearServiceLogs?(): void;
  exportLogs?(): boolean;
  updateKeepAliveStatus?(payload: string): void;
  shareExportFile?(path: string, title: string): boolean;
  openAlipay?(url: string): boolean;
  openWechat?(url: string): boolean;
  getSafeAreaInsets?(): string;
  setSystemBarsStyle?(isDark: boolean, bgHex: string): void;
  setLongPressAllowed?(allowed: boolean): void;
}

interface NavigatorConnection extends EventTarget {
  type?: string;
  effectiveType?: string;
}

interface Navigator {
  readonly connection?: NavigatorConnection;
}

interface Window {
  __nativeQuickAction?: (action: string) => void;
  __TAURI__?: unknown;
  AndroidBridge?: AndroidBridgeApi;
  __showResumeMask?: (hiddenForMs?: number) => void;
  __nativeNetworkChanged?: (source?: string) => void;
  __nativeNotificationAction?: (action: 'check' | 'pause' | 'resume') => Promise<void>;
  __handleAndroidBack?: () => boolean;
  __nativeKeepAlive?: () => void;
  __nativeKeyboardChanged?: (visible: boolean, availableHeight: number) => void;
  __nativeSafeAreaInsets?: (top: number, right: number, bottom: number, left: number) => void;
  triggerAutoLogin?: () => void;
}
