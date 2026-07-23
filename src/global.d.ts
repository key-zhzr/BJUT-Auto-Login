interface AndroidBridgeApi {
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
}

interface NavigatorConnection extends EventTarget {
  type?: string;
  effectiveType?: string;
}

interface Navigator {
  readonly connection?: NavigatorConnection;
}

interface Window {
  __TAURI__?: unknown;
  AndroidBridge?: AndroidBridgeApi;
  __showResumeMask?: () => void;
  __nativeNetworkChanged?: (source?: string) => void;
  __nativeNotificationAction?: (action: 'check' | 'pause' | 'resume') => Promise<void>;
  __handleAndroidBack?: () => boolean;
  __nativeKeepAlive?: () => void;
  triggerAutoLogin?: () => void;
}
