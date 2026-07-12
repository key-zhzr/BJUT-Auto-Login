import {
  Activity, AlertCircle, ArrowDownToLine, ArrowUpCircle, BarChart2, Check, CheckCircle, ChevronDown,
  ClipboardCopy, ClipboardPaste, Clock, Copy, createIcons, Edit2, Eye, FileText, GripVertical,
  LayoutDashboard, Loader, LogIn, Minus, Plus, RefreshCw, Settings, ShieldAlert,
  ShieldCheck, Square, Trash2, User, Users, Wifi, WifiOff, X,
} from 'lucide';
import Sortable from 'sortablejs';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { openUrl } from '@tauri-apps/plugin-opener';
import { marked } from 'marked';
import DOMPurify from 'dompurify';

const icons = {
  Activity, AlertCircle, ArrowDownToLine, ArrowUpCircle, BarChart2, Check, CheckCircle, ChevronDown,
  ClipboardCopy, ClipboardPaste, Clock, Copy, Edit2, Eye, FileText, GripVertical,
  LayoutDashboard, Loader, LogIn, Minus, Plus, RefreshCw, Settings, ShieldAlert,
  ShieldCheck, Square, Trash2, User, Users, Wifi, WifiOff, X,
};

if (navigator.userAgent.toLowerCase().includes('android')) {
  document.body.classList.add('is-android');
}

let loadingMaskTimer: number | null = null;
let appLaunchRevealed = false;
let macosDockPolicyReady: Promise<void> = Promise.resolve();

function setLoadingMaskVisible(visible: boolean, text = '正在启动 BJUT-AL…') {
  const mask = document.getElementById('app-loading-mask');
  const label = document.getElementById('app-loading-text');
  if (!mask) return;
  if (label) label.textContent = text;
  mask.classList.toggle('is-visible', visible);
  mask.setAttribute('aria-hidden', visible ? 'false' : 'true');
}

function showResumeMask() {
  if (!appLaunchRevealed) return;
  if (loadingMaskTimer !== null) window.clearTimeout(loadingMaskTimer);
  setLoadingMaskVisible(true, '正在恢复网络状态…');
  loadingMaskTimer = window.setTimeout(() => {
    setLoadingMaskVisible(false);
    loadingMaskTimer = null;
  }, 260);
}

async function finishAppLaunch() {
  if (appLaunchRevealed) return;
  await macosDockPolicyReady;
  await new Promise(resolve => window.setTimeout(resolve, 40));
  appLaunchRevealed = true;
  setLoadingMaskVisible(false);
  if ((window as any).__TAURI__) {
    await invoke('frontend_ready').catch(error => console.error('Failed to reveal main window:', error));
  }
}

(window as any).__showResumeMask = showResumeMask;
let documentWasHidden = document.hidden;
document.addEventListener('visibilitychange', () => {
  if (documentWasHidden && !document.hidden) showResumeMask();
  documentWasHidden = document.hidden;
});

if (navigator.userAgent.includes('Mac OS X')) {
  document.body.classList.add('is-macos');
  
  // macOS WebLock workaround: hold a persistent shared WebLock to prevent WKWebView process suspension
  if (navigator.locks && navigator.locks.request) {
    navigator.locks.request('prevent-app-nap-lock', { mode: 'shared' }, () => {
      return new Promise(() => {
        // Never resolve, holding the lock permanently in the background
      });
    }).catch(() => {});
  }
}

document.getElementById('titlebar-minimize')?.addEventListener('click', () => {
  try { getCurrentWindow().minimize(); } catch(err) {}
});
document.getElementById('titlebar-maximize')?.addEventListener('click', async () => {
  try {
    await getCurrentWindow().toggleMaximize();
  } catch(err) {}
});
document.getElementById('titlebar-close')?.addEventListener('click', () => {
  try { getCurrentWindow().close(); } catch(err) {}
});


// Credentials live only in memory in the WebView. Rust persists them in the OS credential store.
let accountsCache: any[] = [];
let configSyncQueue: Promise<void> = Promise.resolve();
let missingPasswordWarningShown = false;
const LEGACY_ACCOUNTS_KEY = 'bjut_accounts';
const LEGACY_XOR_KEY = 'bjut-al-secret-key-2026';
const LEGACY_MIGRATION_PENDING_KEY = 'bjut_accounts_migration_pending';

function readLegacyAccounts(): any[] | null {
  const raw = localStorage.getItem(LEGACY_ACCOUNTS_KEY);
  if (raw === null) return null;
  try {
    const json = raw.trim().startsWith('[')
      ? raw
      : Array.from(atob(raw), (character, index) => String.fromCharCode(
          character.charCodeAt(0) ^ LEGACY_XOR_KEY.charCodeAt(index % LEGACY_XOR_KEY.length),
        )).join('');
    const parsed = JSON.parse(json);
    if (!Array.isArray(parsed)) return null;
    return parsed
      .map(account => ({
        ...account,
        user: account?.user ?? account?.username,
        pass: account?.pass ?? account?.password,
      }))
      .filter(account => account && typeof account.user === 'string' && typeof account.pass === 'string')
      .map((account, index) => ({
        user: account.user,
        pass: account.pass,
        isDefault: account.isDefault ?? account.is_default ?? index === 0,
        isDisabled: account.isDisabled ?? account.is_disabled ?? false,
      }));
  } catch (error) {
    console.warn('Unable to decode legacy account storage:', error);
    return null;
  }
}

function mergeLegacyAccounts(current: any[], legacy: any[]): { accounts: any[], changed: boolean } {
  let changed = false;
  const accounts = current.map(account => ({ ...account }));
  legacy.forEach(legacyAccount => {
    const currentAccount = accounts.find(account => account.user === legacyAccount.user);
    if (!currentAccount) {
      accounts.push({
        ...legacyAccount,
        isDefault: accounts.some(account => account.isDefault) ? false : legacyAccount.isDefault,
      });
      changed = true;
    } else if (!currentAccount.pass && legacyAccount.pass) {
      currentAccount.pass = legacyAccount.pass;
      changed = true;
    }
  });
  if (accounts.length > 0 && !accounts.some(account => account.isDefault)) {
    accounts[0].isDefault = true;
    changed = true;
  }
  return { accounts, changed };
}

function secureStorageContainsLegacyCredentials(current: any[], legacy: any[]): boolean {
  const legacyWithPasswords = legacy.filter(account => account.pass);
  return legacyWithPasswords.length > 0 && legacyWithPasswords.every(legacyAccount =>
    current.some(account => account.user === legacyAccount.user && account.pass === legacyAccount.pass),
  );
}

function hasLegacyCredentialConflict(current: any[], legacy: any[]): boolean {
  return legacy.some(legacyAccount => {
    if (!legacyAccount.pass) return false;
    const currentAccount = current.find(account => account.user === legacyAccount.user);
    return Boolean(currentAccount?.pass) && currentAccount.pass !== legacyAccount.pass;
  });
}

async function credentialSnapshotFingerprint(accounts: any[]): Promise<string> {
  const snapshot = accounts.map(account => ({
    user: String(account.user ?? ''),
    pass: String(account.pass ?? ''),
  }));
  const digest = await crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(JSON.stringify(snapshot)),
  );
  return bytesToBase64(new Uint8Array(digest));
}

function warnAboutMissingPasswords(storageStatus = 'missing') {
  const missingPasswordUsers = accountsCache
    .filter(account => !account.pass)
    .map(account => account.user);
  if (missingPasswordUsers.length === 0 || missingPasswordWarningShown) return;

  missingPasswordWarningShown = true;
  const storageUnavailable = storageStatus === 'error';
  log(
    '配置',
    storageUnavailable
      ? `安全存储暂时不可读，以下账号的密码未载入: ${missingPasswordUsers.join(', ')}`
      : `以下账号缺少可恢复的密码: ${missingPasswordUsers.join(', ')}`,
    'error',
  );
  window.setTimeout(() => {
    void customAlert(
      storageUnavailable
        ? `安全凭据存储暂时不可读，因此以下账号的密码没有载入：\n${missingPasswordUsers.join('\n')}\n\n应用已阻止空密码覆盖原数据。请检查应用数据目录权限或重启应用后重试；若仍无法恢复，再重新输入密码。`
        : `以下账号的旧密码副本已经不存在，无法自动恢复：\n${missingPasswordUsers.join('\n')}\n\n请逐个编辑账号并重新输入密码。`,
      storageUnavailable ? '暂时无法读取密码' : '需要重新输入密码',
    );
  }, 300);
}

function getAccounts(): any[] {
  return accountsCache;
}
function saveAccounts(accs: any[]): Promise<void> {
  accountsCache = accs;
  return syncConfigToRust();
}

function saveAccountsInBackground(accs: any[]) {
  void saveAccounts(accs).catch(async error => {
    console.error('Failed to persist accounts:', error);
    await loadConfigFromRust();
    renderAccounts();
    await customAlert(`账号保存失败，已恢复上次保存的内容：${String(error)}`);
  });
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  bytes.forEach(byte => { binary += String.fromCharCode(byte); });
  return btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
  return Uint8Array.from(atob(value), char => char.charCodeAt(0));
}

async function deriveExportKey(passphrase: string, salt: Uint8Array): Promise<CryptoKey> {
  const material = await crypto.subtle.importKey('raw', new TextEncoder().encode(passphrase), 'PBKDF2', false, ['deriveKey']);
  return crypto.subtle.deriveKey(
    { name: 'PBKDF2', salt, iterations: 250000, hash: 'SHA-256' },
    material,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt']
  );
}

async function encryptExport(data: unknown, passphrase: string): Promise<string> {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveExportKey(passphrase, salt);
  const plaintext = new TextEncoder().encode(JSON.stringify(data));
  const ciphertext = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, plaintext);
  return JSON.stringify({ version: 2, salt: bytesToBase64(salt), iv: bytesToBase64(iv), ciphertext: bytesToBase64(new Uint8Array(ciphertext)) });
}

async function decryptExport(value: string, passphrase: string): Promise<any> {
  const envelope = JSON.parse(value);
  if (envelope.version !== 2 || !envelope.salt || !envelope.iv || !envelope.ciphertext) {
    throw new Error('不是受支持的加密配置格式');
  }
  const key = await deriveExportKey(passphrase, base64ToBytes(envelope.salt));
  const plaintext = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: base64ToBytes(envelope.iv) },
    key,
    base64ToBytes(envelope.ciphertext)
  );
  return JSON.parse(new TextDecoder().decode(plaintext));
}

async function writeTextToClipboard(text: string): Promise<void> {
  if ((window as any).AndroidBridge) {
    const copied = (window as any).AndroidBridge.setClipboardText(text);
    if (copied === false) throw new Error('Android 剪贴板写入失败');
  } else if ((window as any).__TAURI__) {
    await invoke('write_clipboard', { text });
  } else {
    await navigator.clipboard.writeText(text);
  }
}

async function readTextFromClipboard(): Promise<string> {
  if ((window as any).AndroidBridge) {
    return (window as any).AndroidBridge.getClipboardText();
  }
  if ((window as any).__TAURI__) {
    return invoke<string>('read_clipboard');
  }
  return navigator.clipboard.readText();
}

interface GitHubReleaseAsset {
  name: string;
  browser_download_url: string;
  size: number;
}

interface GitHubRelease {
  tag_name: string;
  name: string | null;
  body: string | null;
  html_url: string;
  prerelease: boolean;
  draft: boolean;
  assets: GitHubReleaseAsset[];
}

interface UpdateTarget {
  platform: 'android' | 'ios' | 'windows' | 'macos' | 'linux';
  arch: string;
  format: string;
  currentVersion: string;
}

interface UpdateProgress {
  status: 'downloading' | 'installing';
  received?: number;
  total?: number;
  percent?: number | null;
}

interface AccountHealth {
  user: string;
  status: 'healthy' | 'cooling_down' | 'needs_attention' | 'degraded';
  consecutiveFailures: number;
  cooldownUntil: number | null;
  cooldownSeconds: number;
  lastSuccess: string | null;
  lastFailure: string | null;
  lastFailureReason: string | null;
  failureKind: string | null;
}

interface CredentialStorageHealth {
  status: string;
  backend: string;
  persistent: boolean;
  savedAccounts: number;
  missingPasswordAccounts: string[];
  message: string;
}

interface DiagnosticStep {
  id: string;
  label: string;
  status: 'success' | 'warning' | 'error' | 'skipped';
  message: string;
  durationMs: number;
}

interface DiagnosticReport {
  createdAt: string;
  overall: 'healthy' | 'auth_required' | 'no_network' | 'offline';
  summary: string;
  ssid: string;
  ip: string;
  steps: DiagnosticStep[];
}

interface NetworkProfile {
  id: string;
  name: string;
  enabled: boolean;
  ssid: string;
  bssid: string;
  login_type: string;
  account_order: string[];
  auto_login: boolean | null;
  auto_login_types: Record<string, boolean>;
  check_interval: number | null;
  check_interval_bg: number | null;
}

interface AppLogEntry {
  time: string;
  module: string;
  message: string;
  type: 'info' | 'error' | 'success' | 'debug';
}

interface PermissionHealthItem {
  id: string;
  label: string;
  granted: boolean;
  required: boolean;
  detail?: string;
}

function isVersionNewer(current: string, latest: string): boolean {
  const parseVersion = (value: string) => {
    const withoutBuild = value.replace(/^v/i, '').split('+', 1)[0];
    const [core, prerelease = ''] = withoutBuild.split('-', 2);
    return {
      core: core.split('.').map(part => Number.parseInt(part, 10) || 0),
      prerelease: prerelease ? prerelease.split('.') : [],
    };
  };
  const currentVersion = parseVersion(current);
  const latestVersion = parseVersion(latest);
  for (let index = 0; index < Math.max(currentVersion.core.length, latestVersion.core.length); index += 1) {
    const currentPart = currentVersion.core[index] || 0;
    const latestPart = latestVersion.core[index] || 0;
    if (latestPart > currentPart) return true;
    if (currentPart > latestPart) return false;
  }
  if (currentVersion.prerelease.length === 0) return false;
  if (latestVersion.prerelease.length === 0) return true;
  for (let index = 0; index < Math.max(currentVersion.prerelease.length, latestVersion.prerelease.length); index += 1) {
    const currentPart = currentVersion.prerelease[index];
    const latestPart = latestVersion.prerelease[index];
    if (currentPart === undefined) return true;
    if (latestPart === undefined) return false;
    if (currentPart === latestPart) continue;
    const currentNumber = /^\d+$/.test(currentPart) ? Number(currentPart) : null;
    const latestNumber = /^\d+$/.test(latestPart) ? Number(latestPart) : null;
    if (currentNumber !== null && latestNumber !== null) return latestNumber > currentNumber;
    if (currentNumber !== null) return false;
    if (latestNumber !== null) return true;
    return latestPart.localeCompare(currentPart) > 0;
  }
  return false;
}

function selectUpdateAsset(assets: GitHubReleaseAsset[], target: UpdateTarget): GitHubReleaseAsset | undefined {
  const expectedSuffix = (() => {
    switch (target.platform) {
      case 'android': return `_Android_${target.arch}.apk`;
      case 'windows': return `_Windows_${target.arch}.exe`;
      case 'macos': return `_macOS_${target.arch}.dmg`;
      case 'linux': return `_Linux_${target.arch}.${target.format}`;
      default: return '';
    }
  })().toLowerCase();
  if (!expectedSuffix) return undefined;
  return assets.find(asset => asset.name.toLowerCase().endsWith(expectedSuffix));
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '未知大小';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

async function renderReleaseNotes(markdown: string): Promise<string> {
  const rendered = await marked.parse(markdown || '本次发布未提供更新说明。', { gfm: true, breaks: true });
  return DOMPurify.sanitize(rendered, { USE_PROFILES: { html: true } });
}

function updateUpdateProgress(data: UpdateProgress) {
  const progressWrap = document.getElementById('update-progress-wrap');
  const progressBar = document.getElementById('update-progress-bar') as HTMLElement | null;
  const progressText = document.getElementById('update-progress-text');
  if (!progressWrap || !progressBar || !progressText) return;

  progressWrap.classList.remove('hidden');
  if (data.status === 'installing') {
    progressBar.style.width = '100%';
    progressText.textContent = '下载完成，正在启动系统安装程序…';
    return;
  }
  const percent = typeof data.percent === 'number' ? Math.max(0, Math.min(100, data.percent)) : null;
  progressBar.style.width = percent === null ? '35%' : `${percent}%`;
  const received = formatBytes(data.received || 0);
  const total = data.total ? ` / ${formatBytes(data.total)}` : '';
  progressText.textContent = `正在下载 ${received}${total}${percent === null ? '' : `（${percent.toFixed(0)}%）`}`;
}

async function showUpdateDialog(
  release: GitHubRelease,
  target: UpdateTarget,
  asset: GitHubReleaseAsset,
): Promise<boolean> {
  const modal = document.getElementById('update-modal');
  const notes = document.getElementById('update-release-notes');
  const cancelButton = document.getElementById('btn-update-cancel');
  const confirmButton = document.getElementById('btn-update-confirm') as HTMLButtonElement | null;
  const actions = document.getElementById('update-modal-actions');
  const progressWrap = document.getElementById('update-progress-wrap');
  const progressBar = document.getElementById('update-progress-bar') as HTMLElement | null;
  const progressText = document.getElementById('update-progress-text');
  if (!modal || !notes || !cancelButton || !confirmButton || !actions || !progressWrap || !progressBar || !progressText) {
    throw new Error('更新窗口初始化失败');
  }

  document.getElementById('update-modal-title')!.textContent = `发现新版本 ${release.tag_name}`;
  document.getElementById('update-modal-meta')!.textContent =
    `当前 v${target.currentVersion} · ${target.platform}/${target.arch} · ${asset.name} · ${formatBytes(asset.size)}`;
  notes.innerHTML = await renderReleaseNotes(release.body || '');
  notes.querySelectorAll<HTMLAnchorElement>('a[href]').forEach(anchor => {
    anchor.addEventListener('click', event => {
      event.preventDefault();
      const href = anchor.href;
      if (href.startsWith('https://')) openUrl(href).catch(() => {});
    });
  });
  actions.style.display = 'flex';
  confirmButton.disabled = false;
  progressWrap.classList.add('hidden');
  progressBar.style.width = '0';
  progressText.textContent = '准备下载…';
  modal.classList.remove('hidden');

  return new Promise(resolve => {
    const cleanup = () => {
      cancelButton.removeEventListener('click', onCancel);
      confirmButton.removeEventListener('click', onConfirm);
    };
    const onCancel = () => {
      cleanup();
      modal.classList.add('hidden');
      resolve(false);
    };
    const onConfirm = () => {
      cleanup();
      actions.style.display = 'none';
      progressWrap.classList.remove('hidden');
      resolve(true);
    };
    cancelButton.addEventListener('click', onCancel);
    confirmButton.addEventListener('click', onConfirm);
  });
}

function saveSetting(key: string, value: string) {
  localStorage.setItem(key, value);
  void syncConfigToRust().catch(error => {
    console.error(`Failed to persist setting ${key}:`, error);
    void customAlert(`设置保存失败：${String(error)}`);
  });
}

function syncConfigToRust(): Promise<void> {
  if (!(window as any).__TAURI__) return Promise.resolve();
  const balanceThreshold = parseFloat(localStorage.getItem('bjut_balance_alert_threshold') || '10');
  const flowThreshold = parseFloat(localStorage.getItem('bjut_flow_alert_threshold') || '5');
  // Capture an immutable snapshot now. Serializing writes prevents an older,
  // slower IPC call from overwriting a newer account edit.
  const config = {
    accounts: getAccounts().map(account => ({ ...account })),
    auto_login: localStorage.getItem('bjut_auto_login') === 'true',
    check_interval: parseInt(localStorage.getItem('bjut_check_interval') || '15', 10),
    check_interval_bg: parseInt(localStorage.getItem('bjut_check_interval_bg') || '60', 10),
    wifi_change_detect: localStorage.getItem('bjut_wifi_change_detect') !== 'false',
    log_level: localStorage.getItem('bjut_log_level') || 'info',
    whitelist: JSON.parse(localStorage.getItem('bjut_whitelist') || '[]'),
    blacklist: JSON.parse(localStorage.getItem('bjut_blacklist') || '[]'),
    network_profiles: networkProfilesCache.map(profile => ({ ...profile, account_order: [...profile.account_order] })),
    usage_alerts: localStorage.getItem('bjut_usage_alerts') !== 'false',
    balance_alert_threshold: Number.isFinite(balanceThreshold) ? balanceThreshold : 10,
    flow_alert_threshold: Number.isFinite(flowThreshold) ? flowThreshold : 5,
  };
  const operation = configSyncQueue
    .catch(() => {})
    .then(async () => {
      await invoke<void>('sync_config', { config });
      // While a recoverable legacy copy exists, record exactly what the
      // backend accepted. The next cold start must return this snapshot before
      // the legacy copy is deleted. Later account edits update the fingerprint,
      // so deleted/renamed accounts are not resurrected by the old data.
      if (localStorage.getItem(LEGACY_ACCOUNTS_KEY) !== null) {
        const fingerprint = await credentialSnapshotFingerprint(config.accounts);
        localStorage.setItem(LEGACY_MIGRATION_PENDING_KEY, fingerprint);
      }
    });
  configSyncQueue = operation;
  return operation;
}

async function loadConfigFromRust() {
  if (!(window as any).__TAURI__) return;
  try {
    const [config, credentialStorageStatus]: [any, string] = await Promise.all([
      invoke<any>('get_app_config'),
      invoke<string>('get_credential_storage_status'),
    ]);
    if (!config) return;

    const backendAccounts = config.accounts || [];
    const legacyAccounts = readLegacyAccounts();
    const legacyConflict = legacyAccounts !== null
      && hasLegacyCredentialConflict(backendAccounts, legacyAccounts);
    const pendingFingerprint = localStorage.getItem(LEGACY_MIGRATION_PENDING_KEY);
    const migrationWasPending = pendingFingerprint !== null;
    const backendFingerprint = migrationWasPending && credentialStorageStatus === 'available'
      ? await credentialSnapshotFingerprint(backendAccounts)
      : null;
    const migrationConfirmed = legacyAccounts !== null
      && migrationWasPending
      && credentialStorageStatus === 'available'
      && (pendingFingerprint === 'true'
        ? secureStorageContainsLegacyCredentials(backendAccounts, legacyAccounts)
        : backendFingerprint === pendingFingerprint);
    const backendIsAuthoritative = migrationWasPending && credentialStorageStatus === 'available';
    const merged = legacyAccounts === null || backendIsAuthoritative
      ? { accounts: backendAccounts, changed: false }
      : mergeLegacyAccounts(backendAccounts, legacyAccounts);
    accountsCache = backendIsAuthoritative ? backendAccounts : merged.accounts;
    localStorage.setItem('bjut_auto_login', config.auto_login.toString());
    localStorage.setItem('bjut_check_interval', config.check_interval.toString());
    localStorage.setItem('bjut_check_interval_bg', config.check_interval_bg.toString());
    localStorage.setItem('bjut_wifi_change_detect', config.wifi_change_detect.toString());
    localStorage.setItem('bjut_log_level', config.log_level);
    localStorage.setItem('bjut_whitelist', JSON.stringify(config.whitelist || []));
    localStorage.setItem('bjut_blacklist', JSON.stringify(config.blacklist || []));
    networkProfilesCache = (config.network_profiles || []).map((profile: NetworkProfile) => ({ ...profile }));
    localStorage.setItem('bjut_usage_alerts', String(config.usage_alerts !== false));
    localStorage.setItem('bjut_balance_alert_threshold', String(config.balance_alert_threshold ?? 10));
    localStorage.setItem('bjut_flow_alert_threshold', String(config.flow_alert_threshold ?? 5));
    
    autoLoginEnabled = config.auto_login;
    checkInterval = config.check_interval;
    wifiChangeDetectEnabled = config.wifi_change_detect;
    
    settingAutoLogin.checked = autoLoginEnabled;
    settingWifiChangeDetect.checked = wifiChangeDetectEnabled;
    settingCheckInterval.value = checkInterval.toString();
    settingLogLevel.value = config.log_level;
    settingUsageAlerts.checked = config.usage_alerts !== false;
    settingBalanceThreshold.value = String(config.balance_alert_threshold ?? 10);
    settingFlowThreshold.value = String(config.flow_alert_threshold ?? 5);
    renderNetworkProfiles();
    
    const settingCheckIntervalBg = document.getElementById('setting-check-interval-bg') as HTMLInputElement;
    if (settingCheckIntervalBg) {
      settingCheckIntervalBg.value = config.check_interval_bg.toString();
    }

    if (legacyAccounts !== null) {
      try {
        if (legacyAccounts.length === 0) {
          localStorage.removeItem(LEGACY_ACCOUNTS_KEY);
          localStorage.removeItem(LEGACY_MIGRATION_PENDING_KEY);
        } else if (migrationConfirmed) {
          localStorage.removeItem(LEGACY_ACCOUNTS_KEY);
          localStorage.removeItem(LEGACY_MIGRATION_PENDING_KEY);
          log('配置', '旧版账号迁移已在重启后验证，旧副本已清理', 'success');
        } else if (backendIsAuthoritative) {
          // Do not merge stale legacy accounts over a usable secure backend.
          // A mismatch is kept for explicit recovery instead of silently
          // undoing a password change, rename, or deletion.
          log('配置', '安全存储内容与待确认迁移快照不一致，已保留旧副本但不会自动覆盖当前账号', 'error');
        } else if (legacyConflict && credentialStorageStatus === 'available') {
          if (merged.changed) {
            await syncConfigToRust();
            localStorage.removeItem(LEGACY_MIGRATION_PENDING_KEY);
          }
          log('配置', '旧版与安全存储中的密码存在冲突，已保留旧副本且不会自动覆盖当前密码', 'error');
        } else {
          if (merged.changed) {
            await syncConfigToRust();
          } else if (credentialStorageStatus === 'available') {
            localStorage.setItem(
              LEGACY_MIGRATION_PENDING_KEY,
              await credentialSnapshotFingerprint(accountsCache),
            );
          }
          log(
            '配置',
            merged.changed
              ? '旧版账号已写入安全存储，将在下次启动验证后清理旧副本'
              : '仍在等待安全存储完成旧版账号迁移验证',
            'info',
          );
        }
      } catch (error) {
        // Keep the old value for a later retry. Never delete the only recoverable copy.
        console.error('Failed to migrate legacy accounts:', error);
      }
    }
    
    renderAccounts();
    warnAboutMissingPasswords(credentialStorageStatus);
    await Promise.all([refreshAccountHealth(), refreshCredentialStorageHealth()]);
  } catch (e) {
    console.error('Failed to load config from Rust:', e);
  }
}

async function listenToRustEvents() {
  if (!(window as any).__TAURI__) return;
  try {
    listen('countdown-tick', (event: any) => {
      const data = event.payload;
      const countdownText = document.getElementById('countdown-text');
      if (countdownText) {
        if (data.status === 'checking') {
          countdownText.textContent = '检测中...';
          isChecking = true;
        } else if (data.status === 'suspended') {
          countdownText.textContent = '已休眠';
          isChecking = false;
        } else if (data.status === 'ticking') {
          countdownText.textContent = data.seconds.toString();
          isChecking = false;
        }
      }
    });

    listen('network-state-change', (event: any) => {
      const data = event.payload;
      let state = NetworkState.Offline;
      if (data.state === 'Online') state = NetworkState.Online;
      else if (data.state === 'BjutCampus') state = NetworkState.BjutCampus;
      const loginType = data.loginType === 'Type1_221_98' ? LoginType.Type1_221_98
        : data.loginType === 'Type2_251_3' ? LoginType.Type2_251_3
        : data.loginType === 'Type3_172_30' ? LoginType.Type3_172_30
        : LoginType.Unknown;
      
      currentNetworkState = state;
      updateNetworkStatus(state, loginType);
      isChecking = false;
      if (state === NetworkState.Online) {
        updateUserInfo().catch(() => {});
      }

      const moreSsid = document.getElementById('more-ssid');
      const moreBssid = document.getElementById('more-bssid');
      const moreIp = document.getElementById('more-ip');
      if (moreSsid) moreSsid.textContent = data.ssid || '--';
      if (moreBssid) moreBssid.textContent = data.bssid || '--';
      if (moreIp) moreIp.textContent = data.ip || '--';
      if (data.ip) lastKnownIp = data.ip;

      const updateTimestamp = document.getElementById('update-timestamp');
      if (updateTimestamp) {
        updateTimestamp.textContent = data.timestamp || new Date().toLocaleString();
      }
    });

    listen('log-event', (event: any) => {
      const data = event.payload;
      renderLogEntry(data.module, data.message, data.type, data.time);
    });

    listen<UpdateProgress>('update-progress', event => {
      updateUpdateProgress(event.payload);
    });

    listen<AccountHealth[]>('account-health-change', event => {
      setAccountHealth(event.payload);
    });

    listen<{ index: number }>('preferred-account-change', event => {
      accountsCache = accountsCache.map((account, index) => ({
        ...account,
        isDefault: index === event.payload.index,
      }));
      renderAccounts();
    });

    // Load initial logs
    const initialLogs: any[] = await invoke('get_logs');
    logEntriesCache = initialLogs.map(entry => ({
      time: entry.time,
      module: entry.module,
      message: entry.message,
      type: entry.type,
    }));
    logsDirty = true;
    logFilterCount.textContent = `${logEntriesCache.length} 条`;

    // Load initial network state
    try {
      const currentState: any = await invoke('get_current_network_state');
      if (currentState) {
        let state = NetworkState.Offline;
        if (currentState.state === 'Online') state = NetworkState.Online;
        else if (currentState.state === 'BjutCampus') state = NetworkState.BjutCampus;
        const loginType = currentState.loginType === 'Type1_221_98' ? LoginType.Type1_221_98
          : currentState.loginType === 'Type2_251_3' ? LoginType.Type2_251_3
          : currentState.loginType === 'Type3_172_30' ? LoginType.Type3_172_30
          : LoginType.Unknown;
        currentNetworkState = state;
        updateNetworkStatus(state, loginType);
        if (state === NetworkState.Online) {
          updateUserInfo().catch(() => {});
        }

        const moreSsid = document.getElementById('more-ssid');
        const moreBssid = document.getElementById('more-bssid');
        const moreIp = document.getElementById('more-ip');
        if (moreSsid) moreSsid.textContent = currentState.ssid || '--';
        if (moreBssid) moreBssid.textContent = currentState.bssid || '--';
        if (moreIp) moreIp.textContent = currentState.ip || '--';
        if (currentState.ip) lastKnownIp = currentState.ip;

        const updateTimestamp = document.getElementById('update-timestamp');
        if (updateTimestamp) {
          updateTimestamp.textContent = currentState.timestamp || new Date().toLocaleString();
        }
      }
    } catch (err) {
      console.error('Failed to get current network state from Rust on start:', err);
    }

    // Load initial countdown status
    const cStatus: any = await invoke('get_countdown_status');
    const countdownText = document.getElementById('countdown-text');
    if (countdownText) {
      if (cStatus.status === 'checking') countdownText.textContent = '检测中...';
      else if (cStatus.status === 'suspended') countdownText.textContent = '已休眠';
      else countdownText.textContent = cStatus.seconds.toString();
    }

    // Report visibility background status
    const updateBgState = () => {
      const isAndroid = navigator.userAgent.toLowerCase().includes('android');
      const isBg = document.hidden || (!isAndroid && !document.hasFocus());
      invoke('set_background_state', { isBg }).catch(() => {});
    };
    document.addEventListener('visibilitychange', updateBgState);
    window.addEventListener('focus', updateBgState);
    window.addEventListener('blur', updateBgState);
    updateBgState();
  } catch (e) {
    console.error('Failed to listen to Rust events:', e);
  }
}

function getLogsScroller(): HTMLElement | null {
  const container = logsContent.parentElement;
  if (container && getComputedStyle(container).overflowY !== 'visible') return container;
  return document.querySelector<HTMLElement>('main');
}

function renderLogEntry(module: string, message: string, type: 'info' | 'error' | 'success' | 'debug' = 'info', time?: string) {
  const currentLevel = localStorage.getItem('bjut_log_level') || 'info';
  if (currentLevel === 'error' && type !== 'error') {
    return;
  }
  if (currentLevel === 'info' && type === 'debug') {
    return;
  }

  logEntriesCache.push({ time: time || new Date().toLocaleString(), module, message, type });
  if (logEntriesCache.length > 5000) logEntriesCache.splice(0, logEntriesCache.length - 5000);
  logsDirty = true;
  if (logsContent.closest('.page')?.classList.contains('active')) {
    renderFilteredLogs();
  } else {
    logFilterCount.textContent = `${logEntriesCache.length} 条`;
  }
}

function renderFilteredLogs() {
  const logsPageActive = logsContent.closest('.page')?.classList.contains('active') ?? false;
  const scroller = getLogsScroller();
  const wasAtBottom = !scroller || scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop <= 80;
  const query = logSearch?.value.trim().toLocaleLowerCase() || '';
  const level = logLevelFilter?.value || 'all';
  let lastSessionStart = -1;
  for (let index = logEntriesCache.length - 1; index >= 0; index -= 1) {
    if (logEntriesCache[index].message.includes('=== SESSION START ===')) {
      lastSessionStart = index;
      break;
    }
  }
  const sessionStart = logSessionFilter?.value === 'current' && lastSessionStart >= 0 ? lastSessionStart : 0;
  const visible = logEntriesCache.slice(sessionStart).filter(entry => {
    if (level !== 'all' && entry.type !== level) return false;
    if (!query) return true;
    return `${entry.time} ${entry.module} ${entry.message}`.toLocaleLowerCase().includes(query);
  });
  logsContent.innerHTML = '';
  visible.forEach(item => {
    const entry = document.createElement('div');
    entry.className = item.message.includes('=== SESSION START ===') ? 'log-entry log-session-divider' : 'log-entry';
    const timeElement = document.createElement('span');
    timeElement.className = 'log-time';
    timeElement.textContent = `[${item.time}]`;
    const messageElement = document.createElement('span');
    messageElement.className = `log-${item.type}`;
    messageElement.textContent = `[${item.module}] ${item.message.replace('=== SESSION START ===', '启动会话')}`;
    entry.append(timeElement, messageElement);
    logsContent.appendChild(entry);
  });
  if (logFilterCount) logFilterCount.textContent = `${visible.length} / ${logEntriesCache.length} 条`;
  logsDirty = false;
  requestAnimationFrame(() => {
    if (logsPageActive && scroller && wasAtBottom) {
      scroller.scrollTop = scroller.scrollHeight;
    }
  });
}

function customAlert(text: string, title = '提示'): Promise<void> {
  return new Promise(resolve => {
    const modal = document.getElementById('alert-modal');
    if (!modal) { alert(text); resolve(); return; }
    document.getElementById('alert-modal-title')!.textContent = title;
    document.getElementById('alert-modal-text')!.textContent = text;
    const btnOk = document.getElementById('btn-alert-ok')!;
    const cleanup = () => {
      modal.classList.add('hidden');
      btnOk.removeEventListener('click', onOk);
    };
    const onOk = () => { cleanup(); resolve(); };
    btnOk.addEventListener('click', onOk);
    modal.classList.remove('hidden');
  });
}
function customConfirm(text: string, title = '确认'): Promise<boolean> {
  return new Promise(resolve => {
    const modal = document.getElementById('confirm-modal');
    if (!modal) { resolve(confirm(text)); return; }
    document.getElementById('confirm-modal-title')!.textContent = title;
    document.getElementById('confirm-modal-text')!.textContent = text;
    const btnOk = document.getElementById('btn-confirm-ok')!;
    const btnCancel = document.getElementById('btn-confirm-cancel')!;
    const cleanup = () => {
      modal.classList.add('hidden');
      btnOk.removeEventListener('click', onOk);
      btnCancel.removeEventListener('click', onCancel);
    };
    const onOk = () => { cleanup(); resolve(true); };
    const onCancel = () => { cleanup(); resolve(false); };
    btnOk.addEventListener('click', onOk);
    btnCancel.addEventListener('click', onCancel);
    modal.classList.remove('hidden');
  });
}

function customPasswordPrompt(text: string, title = '配置密码'): Promise<string | null> {
  return new Promise(resolve => {
    const modal = document.getElementById('password-prompt-modal');
    const form = document.getElementById('password-prompt-form') as HTMLFormElement | null;
    const input = document.getElementById('password-prompt-input') as HTMLInputElement | null;
    const cancelButton = document.getElementById('btn-password-prompt-cancel');
    if (!modal || !form || !input || !cancelButton) {
      resolve(null);
      return;
    }

    document.getElementById('password-prompt-title')!.textContent = title;
    document.getElementById('password-prompt-text')!.textContent = text;
    input.value = '';

    const cleanup = () => {
      modal.classList.add('hidden');
      form.removeEventListener('submit', onSubmit);
      cancelButton.removeEventListener('click', onCancel);
    };
    const onSubmit = (event: Event) => {
      event.preventDefault();
      const value = input.value;
      if (!value) return;
      cleanup();
      resolve(value);
    };
    const onCancel = () => {
      cleanup();
      resolve(null);
    };

    form.addEventListener('submit', onSubmit);
    cancelButton.addEventListener('click', onCancel);
    modal.classList.remove('hidden');
    requestAnimationFrame(() => input.focus());
  });
}
function showListManageModal(title: string, list: string[], onSave: (list: string[]) => void) {
  const modal = document.getElementById('list-manage-modal');
  if (!modal) { alert(list.join('\n')); return; }
  document.getElementById('list-manage-title')!.textContent = title;
  const content = document.getElementById('list-manage-content')!;
  content.innerHTML = '';
  
  if (list.length === 0) {
    content.innerHTML = '<div style="color: var(--text-muted); padding: 0.5rem;">暂无数据</div>';
  } else {
    list.forEach((item, index) => {
      const div = document.createElement('div');
      div.style.display = 'flex';
      div.style.justifyContent = 'space-between';
      div.style.padding = '0.5rem';
      div.style.borderBottom = '1px solid var(--card-border)';
      const label = document.createElement('span');
      label.style.wordBreak = 'break-all';
      label.textContent = item;
      const removeButton = document.createElement('button');
      removeButton.className = 'btn-icon danger';
      removeButton.style.padding = '0 0.5rem';
      removeButton.dataset.idx = index.toString();
      removeButton.setAttribute('aria-label', '删除');
      removeButton.innerHTML = '<i data-lucide="trash-2"></i>';
      div.append(label, removeButton);
      content.appendChild(div);
    });
  }
  
  const closeBtn = document.getElementById('btn-list-manage-close')!;
  const cleanup = () => {
    modal.classList.add('hidden');
    content.removeEventListener('click', onClickList);
    closeBtn.removeEventListener('click', onClose);
  };
  const onClickList = (e: Event) => {
    const btn = (e.target as HTMLElement).closest('.danger');
    if (btn) {
      const idx = parseInt(btn.getAttribute('data-idx') || '0', 10);
      list.splice(idx, 1);
      onSave(list);
      showListManageModal(title, list, onSave);
    }
  };
  const onClose = () => { cleanup(); };
  content.addEventListener('click', onClickList);
  closeBtn.addEventListener('click', onClose);
  modal.classList.remove('hidden');
  createIcons({ icons });
}

enum NetworkState {
  Online,
  BjutCampus,
  Offline
}

enum LoginType {
  Type1_221_98,
  Type2_251_3,
  Type3_172_30,
  Unknown
}

class CustomSelect {
  element: HTMLElement;
  trigger: HTMLElement;
  triggerSpan: HTMLSpanElement;
  optionsContainer: HTMLElement;
  private _value: string = '';
  onChangeCallbacks: ((value: string) => void)[] = [];

  constructor(elementId: string) {
    this.element = document.getElementById(elementId)!;
    this.trigger = this.element.querySelector('.custom-select-trigger')!;
    this.triggerSpan = this.trigger.querySelector('span')!;
    this.optionsContainer = this.element.querySelector('.custom-select-options')!;
    this.trigger.setAttribute('role', 'combobox');
    this.trigger.setAttribute('aria-haspopup', 'listbox');
    this.trigger.setAttribute('aria-expanded', 'false');
    const accessibleLabel = this.element.getAttribute('aria-label');
    if (accessibleLabel) this.trigger.setAttribute('aria-label', accessibleLabel);
    this.optionsContainer.setAttribute('role', 'listbox');
    this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option').forEach(option => {
      option.setAttribute('role', 'option');
      option.setAttribute('aria-selected', String(option.classList.contains('selected')));
    });

    // Toggle open
    this.trigger.addEventListener('click', (e) => {
      e.stopPropagation();
      // Close other dropdowns first
      document.querySelectorAll('.custom-select').forEach(el => {
        if (el !== this.element) {
          el.classList.remove('open');
          el.querySelector('.custom-select-trigger')?.setAttribute('aria-expanded', 'false');
        }
      });
      this.element.classList.toggle('open');
      this.trigger.setAttribute('aria-expanded', String(this.element.classList.contains('open')));
    });

    this.trigger.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        this.element.classList.remove('open');
        this.trigger.setAttribute('aria-expanded', 'false');
        return;
      }
      if (event.key !== 'Enter' && event.key !== ' ' && event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
      event.preventDefault();
      if (!this.element.classList.contains('open')) {
        this.trigger.click();
        return;
      }
      const options = Array.from(this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option'));
      const selectedIndex = Math.max(0, options.findIndex(option => option.classList.contains('selected')));
      const nextIndex = event.key === 'ArrowUp'
        ? (selectedIndex - 1 + options.length) % options.length
        : (selectedIndex + 1) % options.length;
      if (event.key === 'Enter' || event.key === ' ') options[selectedIndex]?.click();
      else options[nextIndex]?.click();
    });

    // Handle options click
    this.optionsContainer.addEventListener('click', (e) => {
      const option = (e.target as HTMLElement).closest('.custom-option') as HTMLElement;
      if (option) {
        const val = option.getAttribute('data-value') || '';
        this.value = val;
        this.element.classList.remove('open');
        this.trigger.setAttribute('aria-expanded', 'false');
        this.onChangeCallbacks.forEach(cb => cb(val));
      }
    });

    // Close on click outside
    document.addEventListener('click', () => {
      this.element.classList.remove('open');
      this.trigger.setAttribute('aria-expanded', 'false');
    });

    // Initial value
    const selectedOption = this.optionsContainer.querySelector('.custom-option.selected') as HTMLElement;
    if (selectedOption) {
      this._value = selectedOption.getAttribute('data-value') || '';
      this.triggerSpan.textContent = selectedOption.textContent;
    }
  }

  get value(): string {
    return this._value;
  }

  set value(val: string) {
    this.setValue(val);
  }

  setValue(val: string) {
    this._value = val;
    let selectedText = '';
    this.optionsContainer.querySelectorAll('.custom-option').forEach(opt => {
      if (opt.getAttribute('data-value') === val) {
        opt.classList.add('selected');
        opt.setAttribute('aria-selected', 'true');
        selectedText = opt.textContent || '';
      } else {
        opt.classList.remove('selected');
        opt.setAttribute('aria-selected', 'false');
      }
    });
    this.triggerSpan.textContent = selectedText || val;
  }

  addEventListener(event: 'change', callback: (e: any) => void) {
    if (event === 'change') {
      this.onChangeCallbacks.push((val) => {
        callback({ target: { value: val } });
      });
    }
  }

  // Helper to set options dynamically (for override-account)
  setOptions(options: { value: string, text: string }[]) {
    this.optionsContainer.innerHTML = '';
    options.forEach(opt => {
      const div = document.createElement('div');
      div.className = 'custom-option';
      div.setAttribute('role', 'option');
      div.setAttribute('aria-selected', 'false');
      div.setAttribute('data-value', opt.value);
      div.textContent = opt.text;
      if (opt.value === this._value) {
        div.classList.add('selected');
        div.setAttribute('aria-selected', 'true');
        this.triggerSpan.textContent = opt.text;
      }
      this.optionsContainer.appendChild(div);
    });
    // If no option is selected, select the first one
    if (!this.optionsContainer.querySelector('.custom-option.selected') && options.length > 0) {
      this.setValue(options[0].value);
    }
  }
}

// UI Elements
const navItems = document.querySelectorAll('.nav-item');
const pages = document.querySelectorAll('.page');
const networkStatus = document.getElementById('network-status')!;
const networkDetail = document.getElementById('network-detail')!;
const networkIcon = document.getElementById('network-icon')!;
const btnLogin = document.getElementById('btn-login') as HTMLButtonElement;
const infoAccount = document.getElementById('info-account')!;
const infoBalance = document.getElementById('info-balance')!;
const infoFlow = document.getElementById('info-flow')!;
const accountsList = document.getElementById('accounts-list')!;
const addAccountForm = document.getElementById('add-account-form') as HTMLFormElement;
const logsContent = document.getElementById('logs-content')!;
const btnClearLogs = document.getElementById('btn-clear-logs')!;
const btnCopyLogs = document.getElementById('btn-copy-logs')!;
const btnScrollLogs = document.getElementById('btn-scroll-logs')!;
const btnRunDiagnostics = document.getElementById('btn-run-diagnostics') as HTMLButtonElement;
const btnCopyDiagnostics = document.getElementById('btn-copy-diagnostics') as HTMLButtonElement;
const btnResetAllHealth = document.getElementById('btn-reset-all-health') as HTMLButtonElement;
const accountHealthList = document.getElementById('account-health-list')!;
const diagnosticSteps = document.getElementById('diagnostic-steps')!;
const logSearch = document.getElementById('log-search') as HTMLInputElement;
const logFilterCount = document.getElementById('log-filter-count')!;
const btnDiagnosticBundle = document.getElementById('btn-diagnostic-bundle') as HTMLButtonElement;
const networkProfilesList = document.getElementById('network-profiles-list')!;
const networkProfileModal = document.getElementById('network-profile-modal')!;
const networkProfileForm = document.getElementById('network-profile-form') as HTMLFormElement;
const settingUsageAlerts = document.getElementById('setting-usage-alerts') as HTMLInputElement;
const settingBalanceThreshold = document.getElementById('setting-balance-threshold') as HTMLInputElement;
const settingFlowThreshold = document.getElementById('setting-flow-threshold') as HTMLInputElement;
const permissionHealthList = document.getElementById('permission-health-list')!;
const permissionHealthSummary = document.getElementById('permission-health-summary')!;
const btnRefreshPermissions = document.getElementById('btn-refresh-permissions') as HTMLButtonElement;
const settingAutoLogin = document.getElementById('setting-auto-login') as HTMLInputElement;
const settingWifiChangeDetect = document.getElementById('setting-wifi-change-detect') as HTMLInputElement;
const settingAutostart = document.getElementById('setting-autostart') as HTMLInputElement;
const settingCheckInterval = document.getElementById('setting-check-interval') as HTMLInputElement;
let settingLogLevel: CustomSelect;
let overrideAccountSelect: CustomSelect;
let overrideMethodSelect: CustomSelect;
let settingUpdateChannel: CustomSelect;
let logSessionFilter: CustomSelect;
let logLevelFilter: CustomSelect;
let networkProfileProtocolSelect: CustomSelect;
let networkProfileAccountSelect: CustomSelect;

// Add Modal
const addModal = document.getElementById('add-modal')!;
const btnShowAdd = document.getElementById('btn-show-add')!;
const btnCancelAdd = document.getElementById('btn-cancel-add')!;

// Edit Modal
const editModal = document.getElementById('edit-modal')!;
const editAccountForm = document.getElementById('edit-account-form') as HTMLFormElement;
const btnCancelEdit = document.getElementById('btn-cancel-edit')!;
const editAccIndex = document.getElementById('edit-acc-index') as HTMLInputElement;
const editAccUsername = document.getElementById('edit-acc-username') as HTMLInputElement;
const editAccPassword = document.getElementById('edit-acc-password') as HTMLInputElement;

// State

let currentNetworkState = NetworkState.Offline;
let autoLoginEnabled = localStorage.getItem('bjut_auto_login') === 'true';
let checkInterval = parseInt(localStorage.getItem('bjut_check_interval') || '15', 10);
let isLoggingIn = false;
let isChecking = false;
let accountHealthCache = new Map<string, AccountHealth>();
let lastDiagnosticReport: DiagnosticReport | null = null;
let networkProfilesCache: NetworkProfile[] = [];
let logEntriesCache: AppLogEntry[] = [];
let logsDirty = true;
let networkEventDebounce: number | null = null;

// New state for split check loops
let lastKnownIp = '';
let wifiChangeTimer: number | null = null;
let wifiChangeDetectEnabled = localStorage.getItem('bjut_wifi_change_detect') !== 'false';
let connectivityTimer: number | null = null;
let secondsToNextCheck = 0;
let countdownInterval: number | null = null;
let isLoopSuspended = false;

// Initialize
async function init() {
  // Instantiate Custom Selects
  overrideAccountSelect = new CustomSelect('override-account');
  overrideMethodSelect = new CustomSelect('override-method');
  settingUpdateChannel = new CustomSelect('setting-update-channel');
  settingLogLevel = new CustomSelect('setting-log-level');
  logSessionFilter = new CustomSelect('log-session-filter');
  logLevelFilter = new CustomSelect('log-level-filter');
  networkProfileProtocolSelect = new CustomSelect('network-profile-protocol');
  networkProfileAccountSelect = new CustomSelect('network-profile-account');

  createIcons({ icons });
  settingAutoLogin.checked = autoLoginEnabled;
  settingWifiChangeDetect.checked = wifiChangeDetectEnabled;
  settingCheckInterval.value = checkInterval.toString();

  // Handle autostart and quit element visibility and status
  const isAndroid = navigator.userAgent.toLowerCase().includes('android');
  if (!(window as any).__TAURI__ || isAndroid) {
    document.getElementById('setting-autostart-item')?.style.setProperty('display', 'none');
    document.getElementById('setting-quit-item')?.style.setProperty('display', 'none');
  } else {
    import('@tauri-apps/plugin-autostart').then(async ({ isEnabled }) => {
      try {
        settingAutostart.checked = await isEnabled();
      } catch (e) {
        console.warn('Failed to query autostart status:', e);
      }
    });
  }

  // Initialize selectors values
  settingLogLevel.value = localStorage.getItem('bjut_log_level') || 'info';
  settingUpdateChannel.value = localStorage.getItem('bjut_update_channel') || 'release';
  if ((window as any).__TAURI__) {
    void invoke<UpdateTarget>('get_update_target')
      .then(target => {
        const versionLabel = document.getElementById('app-version');
        if (versionLabel) versionLabel.textContent = `v${target.currentVersion}`;
      })
      .catch(error => console.warn('Failed to query application version:', error));
  }
  
  setupNavigation();
  setupEventListeners();
  setupEventDrivenNetworkDetection();
  renderAccounts();
  renderNetworkProfiles();

  // Register triggerAutoLogin globally for eval call from Rust
  (window as any).triggerAutoLogin = () => {
    log('系统', '收到系统底层网络连通事件触发 (Eval)');
    if (autoLoginEnabled && !isLoggingIn) {
      manualLogin();
    }
  };

  const tosAccepted = localStorage.getItem('bjut_tos_accepted') === 'true';
  if (!tosAccepted) {
    const tosModal = document.getElementById('tos-modal')!;
    tosModal.classList.remove('hidden');
    
    document.getElementById('btn-tos-agree')!.addEventListener('click', async () => {
      localStorage.setItem('bjut_tos_accepted', 'true');
      tosModal.classList.add('hidden');
      log('系统', '已同意用户协议与隐私政策');
      
      // Request foreground permissions
      if ((window as any).__TAURI__) {
        try {
          if (isAndroid && (window as any).AndroidBridge) {
            (window as any).AndroidBridge.requestForegroundPermissions();
          } else if (isAndroid) {
            await invoke('request_foreground_permissions');
          }
          if (isAndroid) log('系统', '已申请前台网络定位相关权限');
        } catch (e) {
          console.error('Failed to request foreground permissions:', e);
        }
        // Load the secure backend configuration before the first sync so accepting the
        // terms cannot overwrite an existing account set with the empty WebView cache.
        await loadConfigFromRust();
        await listenToRustEvents();
        log('系统', '应用启动');
      } else {
        startWifiChangeCheckLoop();
        startConnectivityCheckLoop();
        log('系统', '应用启动');
      }
    });
    
    document.getElementById('btn-tos-disagree')!.addEventListener('click', async () => {
      if ((window as any).__TAURI__) {
        try {
          getCurrentWindow().close();
        } catch (e) {
          window.close();
        }
      } else {
        window.close();
      }
    });
  } else {
    // Already accepted
    if ((window as any).__TAURI__) {
      if (isAndroid && (window as any).AndroidBridge) {
        if (autoLoginEnabled) {
          try {
            (window as any).AndroidBridge.startKeepAliveService();
          } catch (e) {
            console.error('Failed to start keep-alive service:', e);
          }
        }
      } else if (isAndroid) {
        invoke('request_foreground_permissions').catch(e => {
          console.error('Failed to request foreground permissions:', e);
        });
      }
      try {
        await loadConfigFromRust();
        await listenToRustEvents();
        log('系统', '应用启动');
        if ((window as any).AndroidBridge && autoLoginEnabled) {
          log('系统', '后台保活服务已启动');
        }
      } catch (error) {
        console.error('Failed to initialize persisted configuration:', error);
      }
    } else {
      startWifiChangeCheckLoop();
      startConnectivityCheckLoop();
      log('系统', '应用启动');
    }
  }
  await finishAppLaunch();
}

function formatHealthTime(value: string | null): string {
  if (!value) return '暂无记录';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function formatCooldown(seconds: number): string {
  if (seconds <= 0) return '可正常尝试';
  if (seconds < 60) return `${seconds} 秒后重试`;
  const minutes = Math.ceil(seconds / 60);
  return minutes < 60 ? `${minutes} 分钟后重试` : `${Math.ceil(minutes / 60)} 小时后重试`;
}

function accountHealthLabel(status: AccountHealth['status']): string {
  if (status === 'needs_attention') return '需处理';
  if (status === 'cooling_down') return '冷却中';
  if (status === 'degraded') return '待观察';
  return '正常';
}

function setAccountHealth(items: AccountHealth[]) {
  accountHealthCache = new Map(items.map(item => [item.user, item]));
  renderAccounts();
  renderAccountHealthPanel();
}

function renderAccountHealthPanel() {
  accountHealthList.innerHTML = '';
  const items = Array.from(accountHealthCache.values());
  if (items.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'diagnostic-empty';
    empty.textContent = '暂无账号';
    accountHealthList.appendChild(empty);
    return;
  }
  items.forEach(item => {
    const row = document.createElement('div');
    row.className = 'account-health-row';
    const info = document.createElement('div');
    info.className = 'account-health-info';
    const title = document.createElement('div');
    title.className = 'account-health-title';
    const user = document.createElement('strong');
    user.textContent = item.user;
    const badge = document.createElement('span');
    badge.className = `account-health-badge ${item.status}`;
    badge.textContent = accountHealthLabel(item.status);
    title.append(user, badge);
    const detail = document.createElement('small');
    const failure = item.lastFailureReason ? `；最近失败：${item.lastFailureReason}` : '';
    detail.textContent = `${formatCooldown(item.cooldownSeconds)}；连续失败 ${item.consecutiveFailures} 次${failure}`;
    detail.title = `最近成功：${formatHealthTime(item.lastSuccess)}；最近失败：${formatHealthTime(item.lastFailure)}`;
    info.append(title, detail);
    const reset = document.createElement('button');
    reset.className = 'btn btn-secondary btn-sm action-reset-health';
    reset.dataset.user = item.user;
    reset.textContent = '解除';
    reset.disabled = item.status === 'healthy' && item.consecutiveFailures === 0;
    row.append(info, reset);
    accountHealthList.appendChild(row);
  });
}

async function refreshAccountHealth() {
  if (!(window as any).__TAURI__) return;
  try {
    setAccountHealth(await invoke<AccountHealth[]>('get_account_health'));
  } catch (error) {
    console.error('Failed to load account health:', error);
  }
}

async function refreshCredentialStorageHealth() {
  if (!(window as any).__TAURI__) return;
  const backend = document.getElementById('credential-health-backend')!;
  const badge = document.getElementById('credential-health-badge')!;
  const message = document.getElementById('credential-health-message')!;
  const saved = document.getElementById('credential-saved-count')!;
  const missing = document.getElementById('credential-missing-count')!;
  const users = document.getElementById('credential-missing-users')!;
  try {
    const health = await invoke<CredentialStorageHealth>('get_credential_storage_health');
    backend.textContent = `${health.backend} · ${health.persistent ? '持久化存储' : '临时存储'}`;
    badge.className = `health-badge ${health.status === 'available' ? 'success' : health.status === 'missing' ? 'warning' : 'error'}`;
    badge.textContent = health.status === 'available' ? '正常' : health.status === 'missing' ? '需补录' : '异常';
    message.textContent = health.message;
    saved.textContent = String(health.savedAccounts);
    missing.textContent = String(health.missingPasswordAccounts.length);
    users.classList.toggle('hidden', health.missingPasswordAccounts.length === 0);
    users.textContent = health.missingPasswordAccounts.length
      ? `需要补录密码：${health.missingPasswordAccounts.join('、')}`
      : '';
  } catch (error) {
    backend.textContent = '无法读取安全存储状态';
    badge.className = 'health-badge error';
    badge.textContent = '异常';
    message.textContent = String(error);
  }
}

function renderDiagnosticReport(report: DiagnosticReport) {
  const summary = document.getElementById('diagnostic-summary')!;
  const badge = document.getElementById('diagnostic-summary-badge')!;
  const title = document.getElementById('diagnostic-summary-title')!;
  const meta = document.getElementById('diagnostic-summary-meta')!;
  const overallClass = report.overall === 'healthy' ? 'success'
    : report.overall === 'auth_required' ? 'warning' : 'error';
  const overallLabel = report.overall === 'healthy' ? '网络正常'
    : report.overall === 'auth_required' ? '需要认证'
      : report.overall === 'no_network' ? '无网络接口' : '无法联网';
  summary.className = `diagnostic-summary glass-card diagnostic-${overallClass}`;
  badge.className = `health-badge ${overallClass}`;
  badge.textContent = overallLabel;
  title.textContent = report.summary;
  meta.textContent = `${formatHealthTime(report.createdAt)} · SSID ${report.ssid || '--'} · IP ${report.ip || '--'}`;
  diagnosticSteps.innerHTML = '';
  report.steps.forEach(step => {
    const row = document.createElement('div');
    row.className = `diagnostic-step ${step.status}`;
    const marker = document.createElement('span');
    marker.className = 'diagnostic-step-marker';
    marker.textContent = step.status === 'success' ? '✓' : step.status === 'warning' ? '!' : step.status === 'skipped' ? '–' : '×';
    const content = document.createElement('div');
    content.className = 'diagnostic-step-info';
    const label = document.createElement('strong');
    label.textContent = step.label;
    const detail = document.createElement('small');
    detail.textContent = step.message;
    content.append(label, detail);
    const duration = document.createElement('span');
    duration.className = 'diagnostic-step-duration';
    duration.textContent = `${step.durationMs} ms`;
    row.append(marker, content, duration);
    diagnosticSteps.appendChild(row);
  });
  btnCopyDiagnostics.disabled = false;
}

function diagnosticReportText(report: DiagnosticReport): string {
  const maskedIp = report.ip
    ? report.ip.split('.').map((part, index) => index < 2 ? part : '*').join('.')
    : '--';
  const lines = [
    'BJUT-AL 网络诊断报告',
    `时间：${formatHealthTime(report.createdAt)}`,
    `结论：${report.summary}`,
    `SSID：${report.ssid || '--'}`,
    `IP：${maskedIp}`,
    '',
  ];
  report.steps.forEach(step => lines.push(`[${step.status}] ${step.label}（${step.durationMs} ms）：${step.message}`));
  lines.push('', '报告不包含账号密码。');
  return lines.join('\n');
}

async function runDiagnostics() {
  if (!(window as any).__TAURI__) {
    await customAlert('网络诊断仅在桌面或移动应用中可用。');
    return;
  }
  btnRunDiagnostics.disabled = true;
  btnCopyDiagnostics.disabled = true;
  btnRunDiagnostics.textContent = '诊断中…';
  document.getElementById('diagnostic-summary-title')!.textContent = '正在逐项检查网络链路…';
  try {
    lastDiagnosticReport = await invoke<DiagnosticReport>('run_network_diagnostics');
    renderDiagnosticReport(lastDiagnosticReport);
    await Promise.all([refreshAccountHealth(), refreshCredentialStorageHealth()]);
  } catch (error) {
    document.getElementById('diagnostic-summary-title')!.textContent = `诊断失败：${String(error)}`;
  } finally {
    btnRunDiagnostics.disabled = false;
    btnRunDiagnostics.textContent = '开始诊断';
  }
}

function renderNetworkProfiles() {
  networkProfilesList.innerHTML = '';
  if (networkProfilesCache.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'diagnostic-empty';
    empty.textContent = '暂无网络档案，将使用全局设置';
    networkProfilesList.appendChild(empty);
    return;
  }
  networkProfilesCache.forEach((profile, index) => {
    const row = document.createElement('div');
    row.className = `network-profile-row${profile.enabled ? '' : ' disabled'}`;
    const info = document.createElement('div');
    info.className = 'network-profile-info';
    const title = document.createElement('strong');
    title.textContent = profile.name;
    const details = document.createElement('small');
    const account = profile.account_order[0] || '沿用全局账号顺序';
    const network = profile.ssid || '校园有线';
    const typePolicy = profile.auto_login_types || {};
    const legacyAutoLogin = profile.auto_login !== false;
    const enabledTypes = [
      (typePolicy.type1 ?? legacyAutoLogin) ? '教学' : '',
      (typePolicy.type2 ?? legacyAutoLogin) ? '宿舍' : '',
      (typePolicy.type3 ?? legacyAutoLogin) ? '有线' : '',
    ].filter(Boolean).join('/');
    details.textContent = `${network} · ${profile.login_type === 'auto' ? '自动协议' : profile.login_type} · 自动登录 ${enabledTypes || '全关'} · ${account}`;
    info.append(title, details);
    const actions = document.createElement('div');
    actions.className = 'network-profile-actions';
    const toggle = document.createElement('button');
    toggle.className = 'btn btn-secondary btn-sm action-toggle-profile';
    toggle.dataset.index = String(index);
    toggle.textContent = profile.enabled ? '停用' : '启用';
    const edit = document.createElement('button');
    edit.className = 'btn btn-secondary btn-sm action-edit-profile';
    edit.dataset.index = String(index);
    edit.textContent = '编辑';
    const remove = document.createElement('button');
    remove.className = 'btn btn-log-danger btn-sm action-delete-profile';
    remove.dataset.index = String(index);
    remove.textContent = '删除';
    actions.append(toggle, edit, remove);
    row.append(info, actions);
    networkProfilesList.appendChild(row);
  });
}

function openNetworkProfileModal(index = -1) {
  const profile = index >= 0 ? networkProfilesCache[index] : null;
  (document.getElementById('network-profile-index') as HTMLInputElement).value = String(index);
  (document.getElementById('network-profile-name') as HTMLInputElement).value = profile?.name || '';
  (document.getElementById('network-profile-ssid') as HTMLInputElement).value = profile?.ssid
    || (document.getElementById('more-ssid')?.textContent?.replace(/^--$/, '') ?? '');
  (document.getElementById('network-profile-bssid') as HTMLInputElement).value = profile?.bssid || '';
  networkProfileProtocolSelect.value = profile?.login_type || 'auto';
  (document.getElementById('network-profile-interval') as HTMLInputElement).value = profile?.check_interval?.toString() || '';
  (document.getElementById('network-profile-interval-bg') as HTMLInputElement).value = profile?.check_interval_bg?.toString() || '';
  const legacyAutoLogin = profile?.auto_login !== false;
  const typePolicy = profile?.auto_login_types || {};
  (document.getElementById('network-profile-auto-type1') as HTMLInputElement).checked = typePolicy.type1 ?? legacyAutoLogin;
  (document.getElementById('network-profile-auto-type2') as HTMLInputElement).checked = typePolicy.type2 ?? legacyAutoLogin;
  (document.getElementById('network-profile-auto-type3') as HTMLInputElement).checked = typePolicy.type3 ?? legacyAutoLogin;
  networkProfileAccountSelect.setOptions([
    { value: '', text: '沿用全局顺序' },
    ...getAccounts().filter(account => !account.isDisabled).map(account => ({ value: account.user, text: account.user })),
  ]);
  networkProfileAccountSelect.value = profile?.account_order[0] || '';
  document.getElementById('network-profile-modal-title')!.textContent = profile ? '编辑网络档案' : '添加网络档案';
  networkProfileModal.classList.remove('hidden');
}

async function persistNetworkProfiles() {
  renderNetworkProfiles();
  await syncConfigToRust();
}

function setupEventDrivenNetworkDetection() {
  const notify = (source: string) => {
    if (!(window as any).__TAURI__) return;
    if (networkEventDebounce !== null) window.clearTimeout(networkEventDebounce);
    networkEventDebounce = window.setTimeout(() => {
      networkEventDebounce = null;
      void invoke('notify_network_change', { source });
    }, 500);
  };
  window.addEventListener('online', () => notify('浏览器 online'));
  window.addEventListener('offline', () => notify('浏览器 offline'));
  const connection = (navigator as any).connection;
  connection?.addEventListener?.('change', () => notify('系统连接属性'));
  (window as any).__nativeNetworkChanged = (source = 'Android NetworkCallback') => notify(source);
  (window as any).__nativeNotificationAction = async (action: 'check' | 'pause' | 'resume') => {
    if (!(window as any).__TAURI__) return;
    if (action === 'check') {
      await invoke('notify_network_change', { source: 'Android 常驻通知' });
    } else {
      await invoke('set_auto_login_pause', { minutes: action === 'pause' ? 60 : 0 });
    }
  };
  const pausedUntil = Number((window as any).AndroidBridge?.getAutoLoginPausedUntil?.() || 0);
  if (pausedUntil > Date.now()) {
    const remainingMinutes = Math.max(1, Math.ceil((pausedUntil - Date.now()) / 60000));
    void invoke('set_auto_login_pause', { minutes: remainingMinutes });
  }
}

async function readPermissionHealth(): Promise<PermissionHealthItem[]> {
  const androidBridge = (window as any).AndroidBridge;
  if (androidBridge?.getPermissionHealth) {
    const raw = androidBridge.getPermissionHealth();
    const parsed = JSON.parse(raw || '[]');
    return Array.isArray(parsed) ? parsed : [];
  }
  const items: PermissionHealthItem[] = [];
  try {
    const notification = await import('@tauri-apps/plugin-notification');
    items.push({
      id: 'notifications',
      label: '系统通知',
      granted: await notification.isPermissionGranted(),
      required: false,
      detail: '用于登录结果和用量提醒',
    });
  } catch {
    items.push({ id: 'notifications', label: '系统通知', granted: false, required: false });
  }
  try {
    const storage = await invoke<CredentialStorageHealth>('get_credential_storage_health');
    items.push({
      id: 'credentialStorage',
      label: '安全凭据存储',
      granted: storage.status === 'available' || storage.status === 'missing',
      required: true,
      detail: storage.backend,
    });
  } catch {
    items.push({ id: 'credentialStorage', label: '安全凭据存储', granted: false, required: true });
  }
  return items;
}

async function refreshPermissionHealth() {
  btnRefreshPermissions.disabled = true;
  permissionHealthSummary.textContent = '正在检查系统权限…';
  try {
    const items = await readPermissionHealth();
    permissionHealthList.innerHTML = '';
    const missingRequired = items.filter(item => item.required && !item.granted).length;
    const missingOptional = items.filter(item => !item.required && !item.granted).length;
    permissionHealthSummary.className = `permission-health-summary ${missingRequired ? 'error' : missingOptional ? 'warning' : 'success'}`;
    permissionHealthSummary.textContent = missingRequired
      ? `${missingRequired} 项必要权限需要处理`
      : missingOptional
        ? `必要权限正常，${missingOptional} 项增强权限未开启`
        : '所有权限状态正常';
    items.forEach(item => {
      const row = document.createElement('div');
      row.className = 'permission-health-row';
      const info = document.createElement('div');
      const title = document.createElement('strong');
      title.textContent = item.label;
      const detail = document.createElement('small');
      detail.textContent = item.detail || (item.required ? '应用核心功能需要此权限' : '可选增强权限');
      info.append(title, detail);
      const actions = document.createElement('div');
      actions.className = 'permission-health-actions';
      const badge = document.createElement('span');
      badge.className = `health-badge ${item.granted ? 'success' : item.required ? 'error' : 'warning'}`;
      badge.textContent = item.granted ? '正常' : item.required ? '缺失' : '未开启';
      actions.appendChild(badge);
      if (!item.granted && item.id !== 'credentialStorage') {
        const button = document.createElement('button');
        button.className = 'btn btn-secondary btn-sm action-permission-settings';
        button.dataset.permission = item.id;
        button.textContent = '处理';
        actions.appendChild(button);
      }
      row.append(info, actions);
      permissionHealthList.appendChild(row);
    });
  } catch (error) {
    permissionHealthSummary.className = 'permission-health-summary error';
    permissionHealthSummary.textContent = `权限检查失败：${String(error)}`;
  } finally {
    btnRefreshPermissions.disabled = false;
  }
}

// Navigation
function setupNavigation() {
  navItems.forEach(item => {
    item.addEventListener('click', () => {
      navItems.forEach(n => n.classList.remove('active'));
      pages.forEach(p => p.classList.remove('active'));
      
      item.classList.add('active');
      const target = item.getAttribute('data-target');
      document.getElementById(target!)?.classList.add('active');
      if (target === 'diagnostics') {
        void Promise.all([refreshAccountHealth(), refreshCredentialStorageHealth()]);
      }
      if (target === 'logs' && logsDirty) renderFilteredLogs();
      if (target === 'settings') void refreshPermissionHealth();
    });
  });
}

// Event Listeners
function setupEventListeners() {
  btnLogin.addEventListener('click', manualLogin);

  btnRefreshPermissions.addEventListener('click', () => void refreshPermissionHealth());
  permissionHealthList.addEventListener('click', async event => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('.action-permission-settings');
    const permission = button?.dataset.permission;
    if (!permission) return;
    const androidBridge = (window as any).AndroidBridge;
    if (androidBridge?.openPermissionSettings) {
      androidBridge.openPermissionSettings(permission);
      return;
    }
    if (permission === 'notifications') {
      try {
        const notification = await import('@tauri-apps/plugin-notification');
        await notification.requestPermission();
      } finally {
        await refreshPermissionHealth();
      }
    }
  });
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden && document.getElementById('settings')?.classList.contains('active')) {
      window.setTimeout(() => void refreshPermissionHealth(), 250);
    }
  });

  btnRunDiagnostics.addEventListener('click', () => void runDiagnostics());
  btnCopyDiagnostics.addEventListener('click', async () => {
    if (!lastDiagnosticReport) return;
    try {
      await writeTextToClipboard(diagnosticReportText(lastDiagnosticReport));
      await customAlert('诊断报告已复制到剪贴板。');
    } catch (error) {
      await customAlert(`复制诊断报告失败：${String(error)}`);
    }
  });
  btnResetAllHealth.addEventListener('click', async () => {
    if (!(window as any).__TAURI__) return;
    btnResetAllHealth.setAttribute('disabled', '');
    try {
      await invoke('reset_account_health', { user: null });
      await refreshAccountHealth();
      log('账号管理', '已解除全部账号的失败熔断');
    } finally {
      btnResetAllHealth.removeAttribute('disabled');
    }
  });
  accountHealthList.addEventListener('click', async event => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('.action-reset-health');
    if (!button?.dataset.user || !(window as any).__TAURI__) return;
    button.disabled = true;
    try {
      await invoke('reset_account_health', { user: button.dataset.user });
      await refreshAccountHealth();
      log('账号管理', `已解除账号 ${button.dataset.user} 的失败熔断`);
    } catch (error) {
      await customAlert(`解除失败：${String(error)}`);
      button.disabled = false;
    }
  });

  document.getElementById('btn-add-network-profile')!.addEventListener('click', () => openNetworkProfileModal());
  document.getElementById('btn-cancel-network-profile')!.addEventListener('click', () => {
    networkProfileModal.classList.add('hidden');
    networkProfileForm.reset();
  });
  networkProfileForm.addEventListener('submit', async event => {
    event.preventDefault();
    const index = parseInt((document.getElementById('network-profile-index') as HTMLInputElement).value, 10);
    const selectedAccount = networkProfileAccountSelect.value;
    const intervalValue = (document.getElementById('network-profile-interval') as HTMLInputElement).value;
    const intervalBgValue = (document.getElementById('network-profile-interval-bg') as HTMLInputElement).value;
    const autoLoginTypes = {
      type1: (document.getElementById('network-profile-auto-type1') as HTMLInputElement).checked,
      type2: (document.getElementById('network-profile-auto-type2') as HTMLInputElement).checked,
      type3: (document.getElementById('network-profile-auto-type3') as HTMLInputElement).checked,
    };
    const profile: NetworkProfile = {
      id: index >= 0 ? networkProfilesCache[index].id : `profile-${Date.now()}`,
      name: (document.getElementById('network-profile-name') as HTMLInputElement).value.trim(),
      enabled: index >= 0 ? networkProfilesCache[index].enabled : true,
      ssid: (document.getElementById('network-profile-ssid') as HTMLInputElement).value.trim(),
      bssid: (document.getElementById('network-profile-bssid') as HTMLInputElement).value.trim(),
      login_type: networkProfileProtocolSelect.value,
      account_order: selectedAccount ? [selectedAccount] : [],
      auto_login: Object.values(autoLoginTypes).some(Boolean),
      auto_login_types: autoLoginTypes,
      check_interval: intervalValue ? Math.max(5, parseInt(intervalValue, 10)) : null,
      check_interval_bg: intervalBgValue ? Math.max(5, parseInt(intervalBgValue, 10)) : null,
    };
    if (!profile.name || (!profile.ssid && profile.login_type !== 'wired')) {
      await customAlert('请输入档案名称；SSID 留空时应选择“校园有线”协议。');
      return;
    }
    if (index >= 0) networkProfilesCache[index] = profile;
    else networkProfilesCache.push(profile);
    try {
      await persistNetworkProfiles();
      networkProfileModal.classList.add('hidden');
      networkProfileForm.reset();
      log('网络档案', `已保存档案：${profile.name}`);
    } catch (error) {
      await customAlert(`保存网络档案失败：${String(error)}`);
    }
  });
  networkProfilesList.addEventListener('click', async event => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('button[data-index]');
    if (!button) return;
    const index = parseInt(button.dataset.index || '-1', 10);
    if (index < 0 || index >= networkProfilesCache.length) return;
    if (button.classList.contains('action-edit-profile')) {
      openNetworkProfileModal(index);
      return;
    }
    if (button.classList.contains('action-toggle-profile')) {
      networkProfilesCache[index].enabled = !networkProfilesCache[index].enabled;
    } else if (button.classList.contains('action-delete-profile')) {
      if (!await customConfirm(`确定删除网络档案“${networkProfilesCache[index].name}”吗？`)) return;
      networkProfilesCache.splice(index, 1);
    }
    await persistNetworkProfiles().catch(error => customAlert(`保存失败：${String(error)}`));
  });

  settingUsageAlerts.addEventListener('change', () => saveSetting('bjut_usage_alerts', String(settingUsageAlerts.checked)));
  settingBalanceThreshold.addEventListener('change', () => {
    const value = Math.max(0, parseFloat(settingBalanceThreshold.value) || 0);
    settingBalanceThreshold.value = String(value);
    saveSetting('bjut_balance_alert_threshold', String(value));
  });
  settingFlowThreshold.addEventListener('change', () => {
    const value = Math.max(0, parseFloat(settingFlowThreshold.value) || 0);
    settingFlowThreshold.value = String(value);
    saveSetting('bjut_flow_alert_threshold', String(value));
  });
  
  // Add modal toggle
  btnShowAdd.addEventListener('click', () => {
    addModal.classList.remove('hidden');
  });
  btnCancelAdd.addEventListener('click', () => {
    addModal.classList.add('hidden');
    addAccountForm.reset();
  });

  addAccountForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    const user = (document.getElementById('acc-username') as HTMLInputElement).value.trim();
    const pass = (document.getElementById('acc-password') as HTMLInputElement).value;
    
    if (user && pass) {
      const previousAccounts = getAccounts();
      if (previousAccounts.find((a:any) => a.user === user)) {
        customAlert('该账号已存在');
        return;
      }
      const nextAccounts = [
        ...previousAccounts.map(account => ({ ...account })),
        { user, pass, isDefault: previousAccounts.length === 0 },
      ];
      const submitButton = addAccountForm.querySelector<HTMLButtonElement>('button[type="submit"]');
      if (submitButton) submitButton.disabled = true;
      try {
        await saveAccounts(nextAccounts);
        renderAccounts();
        addAccountForm.reset();
        addModal.classList.add('hidden');
        log('账号管理', `已添加账号: ${user}`);
      } catch (error) {
        accountsCache = previousAccounts;
        renderAccounts();
        await customAlert(`添加账号失败，未保存任何更改：${String(error)}`);
      } finally {
        if (submitButton) submitButton.disabled = false;
      }
    }
  });

  btnClearLogs.addEventListener('click', () => {
    logEntriesCache = [];
    logsContent.innerHTML = '';
    renderFilteredLogs();
    if ((window as any).__TAURI__) {
      invoke('clear_all_logs').catch(e => console.error(e));
    }
  });

  logSearch.addEventListener('input', renderFilteredLogs);
  logSessionFilter.addEventListener('change', renderFilteredLogs);
  logLevelFilter.addEventListener('change', renderFilteredLogs);

  btnDiagnosticBundle.addEventListener('click', async () => {
    if (!(window as any).__TAURI__) return;
    btnDiagnosticBundle.disabled = true;
    try {
      const bundle = await invoke<string>('create_diagnostic_bundle');
      await writeTextToClipboard(bundle);
      await customAlert('脱敏诊断包已复制到剪贴板。账号、密码、本地 IP 与 BSSID 不会包含在诊断包中。');
    } catch (error) {
      await customAlert(`生成诊断包失败：${String(error)}`);
    } finally {
      btnDiagnosticBundle.disabled = false;
    }
  });

  btnCopyLogs.addEventListener('click', async () => {
    try {
      const text = (window as any).__TAURI__
        ? await invoke<string>('get_log_text')
        : logsContent.textContent || '';
      if (!text.trim()) {
        await customAlert('当前没有可复制的日志。');
        return;
      }
      await writeTextToClipboard(text);
      await customAlert('最近几次启动的日志已复制到剪贴板。');
    } catch (error) {
      await customAlert(`复制日志失败：${String(error)}`);
    }
  });

  btnScrollLogs.addEventListener('click', () => {
    const scroller = getLogsScroller();
    if (scroller) scroller.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' });
    else logsContent.scrollIntoView({ block: 'end', behavior: 'smooth' });
  });

  settingAutoLogin.addEventListener('change', async (e) => {
    autoLoginEnabled = (e.target as HTMLInputElement).checked;
    saveSetting('bjut_auto_login', autoLoginEnabled.toString());
    log('设置', `自动登录已${autoLoginEnabled ? '开启' : '关闭'}`);
    
    if ((window as any).__TAURI__) {
      if (autoLoginEnabled) {
        if ((window as any).AndroidBridge) {
          customAlert('【安卓后台保活提示】\n已开启后台自动登录！应用将请求“始终允许”后台定位权限与通知权限，并拉起后台保活服务，以保证断网自动重连稳定性。\n\n另外，建议您授权“忽略电池优化”。');
          
          try {
            (window as any).AndroidBridge.requestBackgroundPermissions();
            (window as any).AndroidBridge.requestBatteryOptimizations();
            (window as any).AndroidBridge.startKeepAliveService();
            log('系统', '已开启后台保活服务，并申请后台权限');
          } catch (e) {
            console.error('Failed to request background services:', e);
          }
        }
      } else {
        if ((window as any).AndroidBridge) {
          try {
            (window as any).AndroidBridge.stopKeepAliveService();
            log('系统', '已停止后台保活服务');
          } catch (e) {
            console.error('Failed to stop background service:', e);
          }
        }
      }
    }
  });

  settingCheckInterval.addEventListener('change', (e) => {
    const val = parseInt((e.target as HTMLInputElement).value, 10);
    if (val >= 5) {
      checkInterval = val;
      saveSetting('bjut_check_interval', checkInterval.toString());
      log('设置', `前台检测间隔设置为 ${val} 秒`);
      startConnectivityCheckLoop();
    }
  });

  if (settingWifiChangeDetect) {
    settingWifiChangeDetect.addEventListener('change', (e) => {
      wifiChangeDetectEnabled = (e.target as HTMLInputElement).checked;
      saveSetting('bjut_wifi_change_detect', wifiChangeDetectEnabled.toString());
      log('设置', `Wi-Fi 变更检测已${wifiChangeDetectEnabled ? '开启' : '关闭'}`);
      if (wifiChangeDetectEnabled) {
        startWifiChangeCheckLoop();
      } else {
        if (wifiChangeTimer) {
          clearTimeout(wifiChangeTimer);
          wifiChangeTimer = null;
        }
      }
    });
  }

  if (settingAutostart) {
    settingAutostart.addEventListener('change', async (e) => {
      const enabled = (e.target as HTMLInputElement).checked;
      if ((window as any).__TAURI__) {
        try {
          const { enable, disable } = await import('@tauri-apps/plugin-autostart');
          if (enabled) {
            await enable();
            log('设置', '已启用开机自启动');
          } else {
            await disable();
            log('设置', '已停用开机自启动');
          }
        } catch (err) {
          console.error('Failed to change autostart status:', err);
          log('设置', `设置开机自启动失败: ${String(err)}`);
          settingAutostart.checked = !enabled; // revert
        }
      }
    });
  }

  const settingCheckIntervalBg = document.getElementById('setting-check-interval-bg') as HTMLInputElement;
  if (settingCheckIntervalBg) {
    let checkIntervalBg = parseInt(localStorage.getItem('bjut_check_interval_bg') || '60', 10);
    settingCheckIntervalBg.value = checkIntervalBg.toString();
    settingCheckIntervalBg.addEventListener('change', (e) => {
      const val = parseInt((e.target as HTMLInputElement).value, 10);
      if (val >= 5) {
        checkIntervalBg = val;
        saveSetting('bjut_check_interval_bg', checkIntervalBg.toString());
        log('设置', `后台检测间隔设置为 ${val} 秒`);
        startConnectivityCheckLoop();
      }
    });
  }

  // Advanced Settings Events
  const settingMacosDock = document.getElementById('setting-macos-dock') as HTMLInputElement;
  if (settingMacosDock) {
    const dockEnabled = localStorage.getItem('bjut_macos_dock') !== 'false';
    const isMacos = navigator.userAgent.includes('Mac OS X');
    settingMacosDock.checked = dockEnabled;
    // Regular is already the default policy. Re-applying it while the hidden
    // cold-start window is being created can make macOS deactivate the window.
    if ((window as any).__TAURI__ && isMacos && !dockEnabled) {
      macosDockPolicyReady = invoke<void>('set_dock_visible', { visible: false })
        .catch(error => console.error('Failed to initialize macOS dock policy:', error));
    }
    settingMacosDock.addEventListener('change', async (e) => {
      const enabled = (e.target as HTMLInputElement).checked;
      localStorage.setItem('bjut_macos_dock', enabled.toString());
      log('设置', `已${enabled ? '启用' : '关闭'}在程序坞显示图标`);
      if ((window as any).__TAURI__ && isMacos) {
        try {
          await invoke('set_dock_visible', { visible: enabled });
        } catch (err) {
          console.error('Failed to toggle macOS dock icon:', err);
        }
      }
    });
  }

  const settingMoreOptions = document.getElementById('setting-more-options') as HTMLInputElement;
  const dashboardOverrideOptions = document.getElementById('dashboard-override-options');
  
  if (settingMoreOptions && dashboardOverrideOptions) {
    const isMoreOptionsEnabled = localStorage.getItem('bjut_more_options') === 'true';
    settingMoreOptions.checked = isMoreOptionsEnabled;
    dashboardOverrideOptions.style.display = isMoreOptionsEnabled ? 'block' : 'none';
    
    settingMoreOptions.addEventListener('change', (e) => {
      const enabled = (e.target as HTMLInputElement).checked;
      localStorage.setItem('bjut_more_options', enabled.toString());
      dashboardOverrideOptions.style.display = enabled ? 'block' : 'none';
      log('设置', `更多控制台选项已${enabled ? '开启' : '关闭'}`);
    });
  }

  if (overrideAccountSelect) {
    overrideAccountSelect.addEventListener('change', (e) => {
      if (e.target.value === 'add') {
        overrideAccountSelect.value = 'auto'; // Reset
        addModal.classList.remove('hidden');
      }
    });
  }

  if (settingLogLevel) {
    settingLogLevel.addEventListener('change', (e) => {
      const level = e.target.value;
      saveSetting('bjut_log_level', level);
      log('设置', `日志详细等级已设置为 ${level.toUpperCase()}`);
    });
  }

  const btnManualUpdate = document.getElementById('btn-manual-update');
  if (btnManualUpdate) {
    btnManualUpdate.addEventListener('click', async () => {
      if (isChecking) return;
      isChecking = true;
      const btnIcon = btnManualUpdate.querySelector('i');
      if (btnIcon) btnIcon.style.animation = 'spin 0.8s linear infinite';
      
      log('网络', '手动触发网络连通性检测...', 'info');
      
      const safetyTimeout = setTimeout(() => {
        isChecking = false;
        if (btnIcon) btnIcon.style.animation = '';
      }, 10000);

      if ((window as any).__TAURI__) {
        try {
          await invoke('trigger_manual_check');
        } catch (err) {
          console.error('Failed to trigger manual check in Rust:', err);
          isChecking = false;
          clearTimeout(safetyTimeout);
          if (btnIcon) btnIcon.style.animation = '';
        }
      } else {
        try {
          await checkNetwork();
        } finally {
          isChecking = false;
          clearTimeout(safetyTimeout);
          if (btnIcon) btnIcon.style.animation = '';
        }
      }
    });
  }

  if (settingUpdateChannel) {
    settingUpdateChannel.addEventListener('change', (e) => {
      const channel = e.target.value;
      localStorage.setItem('bjut_update_channel', channel);
      log('设置', `更新通道已设置为 ${channel === 'release' ? '正式版' : '预览版'}`);
    });
  }

  const btnQuitApp = document.getElementById('btn-quit-app');
  if (btnQuitApp) {
    btnQuitApp.addEventListener('click', async () => {
      if (await customConfirm('确定要退出应用吗？这将彻底关闭后台网络自动登录服务。')) {
        if ((window as any).__TAURI__) {
          try {
            await invoke('exit_app');
          } catch (e) {
            console.error('Failed to exit app:', e);
          }
        } else {
          window.close();
        }
      }
    });
  }

  const btnCheckUpdate = document.getElementById('btn-check-update') as HTMLButtonElement | null;
  if (btnCheckUpdate) {
    btnCheckUpdate.addEventListener('click', async () => {
      const channel = settingUpdateChannel.value;
      log('系统', `正在检查更新 (通道: ${channel === 'release' ? '正式版' : '预览版'})...`);
      btnCheckUpdate.disabled = true;
      const originalText = btnCheckUpdate.textContent || '检查更新';
      btnCheckUpdate.textContent = '检查中…';

      try {
        if (!(window as any).__TAURI__) {
          throw new Error('仅应用内支持自动更新');
        }
        const target = await invoke<UpdateTarget>('get_update_target');
        const response = await fetch('https://api.github.com/repos/key-zhzr/BJUT-Auto-Login/releases?per_page=10', {
          headers: { Accept: 'application/vnd.github+json' },
        });
        if (!response.ok) {
          throw new Error(`GitHub API 返回 HTTP ${response.status}`);
        }

        const releases = await response.json() as GitHubRelease[];
        if (!Array.isArray(releases) || releases.length === 0) {
          await customAlert('暂无更新版本发布');
          log('系统', '检查更新完毕 (暂无发布版本)');
          return;
        }

        const targetReleases = releases.filter(release => !release.draft && (channel !== 'release' || !release.prerelease));
        if (targetReleases.length === 0) {
          await customAlert('暂无符合当前通道的更新版本');
          log('系统', '检查更新完毕 (当前通道暂无新版本)');
          return;
        }

        const latestRelease = targetReleases[0];
        if (!isVersionNewer(target.currentVersion, latestRelease.tag_name)) {
          await customAlert(`当前已是最新版本 (v${target.currentVersion})！`);
          log('系统', `检查更新完毕，当前版本 v${target.currentVersion} 已是最新`);
          return;
        }

        const asset = selectUpdateAsset(latestRelease.assets, target);
        if (!asset) {
          log('系统', `版本 ${latestRelease.tag_name} 未提供 ${target.platform}/${target.arch}/${target.format} 安装包`, 'error');
          await customAlert(
            `发现 ${latestRelease.tag_name}，但该版本没有适用于当前设备（${target.platform}/${target.arch}/${target.format}）的安装包。`,
            '没有匹配的安装包',
          );
          return;
        }
        const signedManifest = latestRelease.assets.find(item => item.name === 'latest.json');
        if (target.platform !== 'android' && !signedManifest) {
          log('系统', `版本 ${latestRelease.tag_name} 缺少 Tauri 签名更新清单`, 'error');
          await customAlert('该版本没有签名更新清单，应用已拒绝自动安装。请从 Releases 页面手动下载。', '无法验证更新签名');
          return;
        }

        log('系统', `发现新版本: ${latestRelease.tag_name}，匹配安装包 ${asset.name}`, 'success');
        const confirmed = await showUpdateDialog(latestRelease, target, asset);
        if (!confirmed) return;

        try {
          await invoke('download_and_install_update', {
            url: target.platform === 'android'
              ? asset.browser_download_url
              : signedManifest!.browser_download_url,
            fileName: asset.name,
          });
          const progressText = document.getElementById('update-progress-text');
          if (progressText) progressText.textContent = '系统安装程序已启动，请按系统提示完成安装。';
          log('系统', `更新包 ${asset.name} 已下载，系统安装程序已启动`, 'success');
          window.setTimeout(() => document.getElementById('update-modal')?.classList.add('hidden'), 2500);
        } catch (error) {
          document.getElementById('update-modal')?.classList.add('hidden');
          throw error;
        }
      } catch (err) {
        console.error('Update check failed:', err);
        await customAlert(`检查或安装更新失败：${String(err)}`);
        log('系统', `检查更新失败: ${String(err)}`, 'error');
      } finally {
        btnCheckUpdate.disabled = false;
        btnCheckUpdate.textContent = originalText;
      }
    });
  }

  const btnManageWhitelist = document.getElementById('btn-manage-whitelist');
  if (btnManageWhitelist) {
    btnManageWhitelist.addEventListener('click', () => {
      const w = JSON.parse(localStorage.getItem('bjut_whitelist') || '[]');
      showListManageModal('信任的 WiFi (白名单)', w, (newList) => saveSetting('bjut_whitelist', JSON.stringify(newList)));
    });
  }

  const btnManageBlacklist = document.getElementById('btn-manage-blacklist');
  if (btnManageBlacklist) {
    btnManageBlacklist.addEventListener('click', () => {
      const b = JSON.parse(localStorage.getItem('bjut_blacklist') || '[]');
      showListManageModal('拒绝的 WiFi (黑名单)', b, (newList) => saveSetting('bjut_blacklist', JSON.stringify(newList)));
    });
  }

  
  const btnExportConfig = document.getElementById('btn-export-config');
  const btnImportConfig = document.getElementById('btn-import-config');
  
  if (btnExportConfig) {
    btnExportConfig.addEventListener('click', async () => {
      try {
        const passphrase = await customPasswordPrompt('为导出的配置设置密码，请妥善保管。', '导出配置');
        if (!passphrase) return;
        const config = {
          accounts: getAccounts(),
          autoLogin: localStorage.getItem('bjut_auto_login'),
          checkInterval: localStorage.getItem('bjut_check_interval'),
          checkIntervalBg: localStorage.getItem('bjut_check_interval_bg'),
          whitelist: localStorage.getItem('bjut_whitelist'),
          blacklist: localStorage.getItem('bjut_blacklist'),
          moreOptions: localStorage.getItem('bjut_more_options'),
          networkProfiles: networkProfilesCache,
          usageAlerts: localStorage.getItem('bjut_usage_alerts'),
          balanceAlertThreshold: localStorage.getItem('bjut_balance_alert_threshold'),
          flowAlertThreshold: localStorage.getItem('bjut_flow_alert_threshold'),
        };
        const encrypted = await encryptExport(config, passphrase);
        await writeTextToClipboard(encrypted);
        customAlert('配置已使用你设置的密码加密并复制到剪贴板。');
      } catch (e) {
        console.error('Export config failed:', e);
        customAlert('导出失败：' + String(e));
      }
    });
  }

  if (btnImportConfig) {
    btnImportConfig.addEventListener('click', async () => {
      try {
        const text = await readTextFromClipboard();

        if (!text) {
          customAlert('剪贴板为空');
          return;
        }
        const confirmResult = await customConfirm('导入配置将覆盖当前设置和账号，是否继续？');
        if (!confirmResult) return;
        
        const passphrase = await customPasswordPrompt('输入导出该配置时设置的密码。', '导入配置');
        if (!passphrase) return;
        const config = await decryptExport(text.trim(), passphrase);
        if (config.accounts) {
          accountsCache = config.accounts;
        }
        if (config.autoLogin !== undefined && config.autoLogin !== null) localStorage.setItem('bjut_auto_login', config.autoLogin);
        if (config.checkInterval !== undefined && config.checkInterval !== null) localStorage.setItem('bjut_check_interval', config.checkInterval);
        if (config.checkIntervalBg !== undefined && config.checkIntervalBg !== null) localStorage.setItem('bjut_check_interval_bg', config.checkIntervalBg);
        if (config.whitelist) localStorage.setItem('bjut_whitelist', config.whitelist);
        if (config.blacklist) localStorage.setItem('bjut_blacklist', config.blacklist);
        if (config.moreOptions !== undefined && config.moreOptions !== null) localStorage.setItem('bjut_more_options', config.moreOptions);
        if (Array.isArray(config.networkProfiles)) networkProfilesCache = config.networkProfiles;
        if (config.usageAlerts !== undefined && config.usageAlerts !== null) localStorage.setItem('bjut_usage_alerts', config.usageAlerts);
        if (config.balanceAlertThreshold !== undefined && config.balanceAlertThreshold !== null) localStorage.setItem('bjut_balance_alert_threshold', config.balanceAlertThreshold);
        if (config.flowAlertThreshold !== undefined && config.flowAlertThreshold !== null) localStorage.setItem('bjut_flow_alert_threshold', config.flowAlertThreshold);
        
        syncConfigToRust().then(() => {
          customAlert('导入成功，请刷新以应用更改！');
          setTimeout(() => location.reload(), 1500);
        }).catch((err) => {
          customAlert('导入同步失败：' + String(err));
        });
      } catch (e) {
        customAlert('导入失败：' + String(e));
      }
    });
  }

  // Password visibility toggle
  document.querySelectorAll('.toggle-password').forEach(btn => {
    btn.addEventListener('click', (e) => {
      const targetId = (e.currentTarget as HTMLElement).getAttribute('data-target');
      const input = document.getElementById(targetId!) as HTMLInputElement;
      if (input.type === 'password') {
        input.type = 'text';
        btn.classList.remove('hide-password');
      } else {
        input.type = 'password';
        btn.classList.add('hide-password');
      }
    });
  });

  // Edit Modal events
  btnCancelEdit.addEventListener('click', () => {
    editModal.classList.add('hidden');
  });

  editAccountForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    const index = parseInt(editAccIndex.value, 10);
    const user = editAccUsername.value.trim();
    const pass = editAccPassword.value;

    if (user && pass && !isNaN(index)) {
      const previousAccounts = getAccounts();
      if (!previousAccounts[index]) {
        await customAlert('要修改的账号不存在，请刷新后重试。');
        return;
      }
      if (previousAccounts.findIndex((a:any, i) => a.user === user && i !== index) !== -1) {
        customAlert('该账号名已存在');
        return;
      }
      const nextAccounts = previousAccounts.map((account, accountIndex) => accountIndex === index
        ? { ...account, user, pass }
        : { ...account });
      const submitButton = editAccountForm.querySelector<HTMLButtonElement>('button[type="submit"]');
      if (submitButton) submitButton.disabled = true;
      try {
        await saveAccounts(nextAccounts);
        renderAccounts();
        editModal.classList.add('hidden');
        log('账号管理', `已修改并保存账号: ${user}`);
      } catch (error) {
        accountsCache = previousAccounts;
        renderAccounts();
        await customAlert(`修改账号失败，未保存任何更改：${String(error)}`);
      } finally {
        if (submitButton) submitButton.disabled = false;
      }
    }
  });

  // Account List Event Delegation
  accountsList.addEventListener('click', (e) => {
    const target = e.target as HTMLElement;
    
    // Toggle account disabled state
    const avatar = target.closest('.account-avatar');
    if (avatar) {
      const parent = avatar.closest('.account-item');
      if (parent) {
        const index = parseInt(parent.getAttribute('data-index') || '-1', 10);
        if (index !== -1) {
          const accounts = getAccounts();
          accounts[index].isDisabled = !accounts[index].isDisabled;
          saveAccountsInBackground(accounts);
          if (accounts[index].isDisabled) {
            parent.classList.add('disabled');
            log('账号管理', `已禁用账号: ${accounts[index].user}`);
          } else {
            parent.classList.remove('disabled');
            log('账号管理', `已启用账号: ${accounts[index].user}`);
          }
          updateOverrideOptions();
        }
      }
      return;
    }

    const btn = target.closest('button');
    if (!btn) return;
    
    const index = parseInt(btn.getAttribute('data-index') || '-1', 10);
    
    // Toggle password visibility
    if (btn.classList.contains('action-toggle-password')) {
      const parent = btn.closest('div');
      const textSpan = parent?.querySelector('.password-text') as HTMLElement;
      if (textSpan) {
        if (textSpan.textContent === '*************') {
          textSpan.textContent = textSpan.getAttribute('data-password') || '';
          btn.classList.remove('hide-password');
        } else {
          textSpan.textContent = '*************';
          btn.classList.add('hide-password');
        }
      }
      return;
    }

    if (index === -1) return;

    if (btn.classList.contains('action-delete')) {
      const deleteModal = document.getElementById('delete-modal');
      const deleteText = document.getElementById('delete-modal-text');
      if (deleteModal && deleteText) {
        const accounts = getAccounts();
        deleteModal.setAttribute('data-delete-index', index.toString());
        deleteText.textContent = `确定要删除账号 ${accounts[index].user} 吗？`;
        deleteModal.classList.remove('hidden');
      }
    } else if (btn.classList.contains('action-default')) {
      // First: Get current rects
      const firstRects = new Map<HTMLElement, DOMRect>();
      Array.from(accountsList.children).forEach(child => {
        firstRects.set(child as HTMLElement, child.getBoundingClientRect());
      });
      
      const accounts = getAccounts();
      const account = accounts.splice(index, 1)[0];
      accounts.unshift(account);
      accounts.forEach((a:any, i:any) => a.isDefault = (i === 0));
      saveAccountsInBackground(accounts);
      
      // Move DOM element to top
      const itemToMove = accountsList.children[index] as HTMLElement;
      accountsList.prepend(itemToMove);
      
      // Update attributes manually
      Array.from(accountsList.children).forEach((el, i) => {
        el.setAttribute('data-index', i.toString());
        el.querySelectorAll('button[data-index]').forEach(b => b.setAttribute('data-index', i.toString()));
        
        const badge = el.querySelector('.account-badge');
        if (badge) {
          badge.textContent = i === 0 ? '默认' : '备用';
          badge.className = `account-badge ${i === 0 ? 'text-primary font-bold' : 'text-muted'}`;
        }
        
        el.querySelectorAll('.action-default').forEach(defaultBtn => {
          const b = defaultBtn as HTMLButtonElement;
          if (i === 0) {
            b.disabled = true;
            b.style.color = 'var(--text-muted)';
            b.style.cursor = 'not-allowed';
            b.style.opacity = '0.5';
            b.setAttribute('title', '已置顶');
          } else {
            b.disabled = false;
            b.style.color = '';
            b.style.cursor = 'pointer';
            b.style.opacity = '1';
            b.setAttribute('title', '设为默认 (置顶)');
          }
        });
      });
      
      // Last, Invert, Play
      Array.from(accountsList.children).forEach(child => {
        const el = child as HTMLElement;
        const first = firstRects.get(el);
        if (!first) return;
        const last = el.getBoundingClientRect();
        
        const dx = first.left - last.left;
        const dy = first.top - last.top;
        
        if (dx !== 0 || dy !== 0) {
          el.animate([
            { transform: `translate(${dx}px, ${dy}px)` },
            { transform: 'translate(0, 0)' }
          ], {
            duration: 400,
            easing: 'cubic-bezier(0.34, 1.56, 0.64, 1)'
          });
        }
      });
      
      log('账号管理', `已将账号 ${account.user} 置顶`);
    } else if (btn.classList.contains('action-edit')) {
      const accounts = getAccounts();
      editAccIndex.value = index.toString();
      editAccUsername.value = accounts[index].user;
      editAccPassword.value = accounts[index].pass;
      editModal.classList.remove('hidden');
    }
  });

  // Delete Modal Events
  const btnCancelDelete = document.getElementById('btn-cancel-delete');
  const btnConfirmDelete = document.getElementById('btn-confirm-delete');
  const deleteModal = document.getElementById('delete-modal');
  
  if (btnCancelDelete && btnConfirmDelete && deleteModal) {
    btnCancelDelete.addEventListener('click', () => {
      deleteModal.classList.add('hidden');
    });
    
    btnConfirmDelete.addEventListener('click', () => {
      const idxStr = deleteModal.getAttribute('data-delete-index');
      if (idxStr) {
        const index = parseInt(idxStr, 10);
        const accounts = getAccounts();
        log('账号管理', `已删除账号 ${accounts[index].user}`);
        accounts.splice(index, 1);
        if (accounts.length > 0 && !accounts.find((a:any) => a.isDefault)) {
          accounts[0].isDefault = true;
        }
        saveAccountsInBackground(accounts);
        renderAccounts();
        deleteModal.classList.add('hidden');
      }
    });
  }

  // Drag and Drop using SortableJS

  let lastFallbackRect: DOMRect | null = null;
  let fallbackCaptureFrame: number | null = null;
  const captureFallbackRect = () => {
    const fallback = document.querySelector<HTMLElement>('.dragging-fallback');
    if (fallback) lastFallbackRect = fallback.getBoundingClientRect();
    fallbackCaptureFrame = requestAnimationFrame(captureFallbackRect);
  };
  Sortable.create(accountsList, {
    handle: '.drag-handle',
    animation: 300,
    easing: "cubic-bezier(0.25, 1, 0.5, 1)",
    ghostClass: 'dragging',
    forceFallback: true,
    fallbackClass: 'dragging-fallback',
    fallbackOnBody: true,
    onStart: () => {
      lastFallbackRect = null;
      if (fallbackCaptureFrame !== null) cancelAnimationFrame(fallbackCaptureFrame);
      fallbackCaptureFrame = requestAnimationFrame(captureFallbackRect);
    },
    onMove: () => {
      const fallback = document.querySelector<HTMLElement>('.dragging-fallback');
      if (fallback) lastFallbackRect = fallback.getBoundingClientRect();
      return true;
    },
    onEnd: (evt) => {
      const { oldIndex, newIndex, item } = evt;

      if (fallbackCaptureFrame !== null) cancelAnimationFrame(fallbackCaptureFrame);
      fallbackCaptureFrame = null;
      const activeFallback = document.querySelector<HTMLElement>('.dragging-fallback');
      const releaseRect = activeFallback?.getBoundingClientRect() || lastFallbackRect;
      lastFallbackRect = null;
      
      const finalRect = item.getBoundingClientRect();
      if (releaseRect) {
        // Create a clone to animate the fly-back natively bypassing overflow: hidden
        const clone = item.cloneNode(true) as HTMLElement;
        clone.classList.remove('dragging');
        clone.classList.add('dragging-fallback-clone');
        clone.style.position = 'fixed';
        clone.style.left = '0';
        clone.style.top = '0';
        clone.style.width = `${releaseRect.width}px`;
        clone.style.height = `${releaseRect.height}px`;
        clone.style.margin = '0';
        
        document.body.appendChild(clone);
        
        // Hide the real item while the clone is flying back
        item.style.opacity = '0';
        
        const fromX = releaseRect.left;
        const fromY = releaseRect.top;
        const toX = finalRect.left;
        const toY = finalRect.top;
        
        const animation = clone.animate([
          { transform: `translate3d(${fromX}px, ${fromY}px, 0) scale(1.02)`, boxShadow: '0 12px 28px rgba(0,0,0,0.32)' },
          { transform: `translate3d(${toX}px, ${toY - 2}px, 0) scale(1.006)`, offset: 0.78, boxShadow: '0 5px 14px rgba(0,0,0,0.18)' },
          { transform: `translate3d(${toX}px, ${toY}px, 0) scale(1)`, boxShadow: '0 0 0 rgba(0,0,0,0)' },
        ], {
          duration: 360,
          easing: 'cubic-bezier(0.22, 1, 0.36, 1)',
        });
        
        const cleanup = () => {
          clone.remove();
          item.style.opacity = '';
        };
        animation.onfinish = cleanup;
        animation.oncancel = cleanup;
      }

      if (oldIndex !== undefined && newIndex !== undefined && oldIndex !== newIndex) {
        // Update metadata after Sortable has committed the new DOM order.
        requestAnimationFrame(() => {
          const accounts = getAccounts();
          const accItem = accounts.splice(oldIndex, 1)[0];
          accounts.splice(newIndex, 0, accItem);
          accounts.forEach((a:any, i:number) => a.isDefault = (i === 0));
          saveAccountsInBackground(accounts);
          
          // Update DOM in-place
          const domItems = accountsList.querySelectorAll('.account-item');
          domItems.forEach((el, index) => {
            el.setAttribute('data-index', index.toString());
            el.querySelectorAll('button[data-index]').forEach(b => b.setAttribute('data-index', index.toString()));
            
            const badge = el.querySelector('.account-badge');
            if (badge) {
              badge.className = index === 0 ? 'account-badge text-primary font-bold' : 'account-badge text-muted';
              badge.textContent = index === 0 ? '默认' : '备用';
            }
            
            el.querySelectorAll('.action-default').forEach(defaultBtn => {
              const b = defaultBtn as HTMLButtonElement;
              if (index === 0) {
                b.style.color = 'var(--text-muted)';
                b.style.cursor = 'not-allowed';
                b.style.opacity = '0.5';
                b.title = '已置顶';
                b.disabled = true;
              } else {
                b.style.color = '';
                b.style.cursor = '';
                b.style.opacity = '';
                b.title = '设为默认 (置顶)';
                b.disabled = false;
              }
            });
          });
          
          log('账号管理', '账号顺序已更新，最高优先级将作为默认账号');
        });
      }
    }
  });
}

// Logging
function log(module: string, message: string, type: 'info' | 'error' | 'success' | 'debug' = 'info') {
  if ((window as any).__TAURI__) {
    invoke('log_from_js', { module, message, logType: type }).catch(() => {});
    return;
  }
  renderLogEntry(module, message, type);
}

// Accounts
function renderAccounts() {
  const accounts = getAccounts();
  accountsList.innerHTML = '';
  if (accounts.length === 0) {
    accountsList.innerHTML = '<div style="color: var(--text-muted); padding: 1rem;">暂无账号，请添加。</div>';
    updateOverrideOptions();
    return;
  }
  
  accounts.forEach((acc, index) => {
    const item = document.createElement('div');
    item.className = 'account-item glass-card' + (acc.isDisabled ? ' disabled' : '');
    item.setAttribute('data-index', index.toString());
    const avatarText = acc.user.length >= 2 ? acc.user.slice(-2) : acc.user;
    
    item.innerHTML = `
      <div class="account-left">
        <div class="drag-handle"><i data-lucide="grip-vertical"></i></div>
        <div class="account-avatar"></div>
        <div class="account-user">
          <h4></h4>
          <span class="account-badge ${acc.isDefault ? 'text-primary font-bold' : 'text-muted'}">${acc.isDefault ? '默认' : '备用'}</span>
          <div class="account-health-inline">
            <span class="account-health-badge healthy">正常</span>
            <small>暂无失败记录</small>
          </div>
        </div>
        <div class="account-mobile-actions">
          <button class="btn-icon action-edit" data-index="${index}" title="编辑"><i data-lucide="edit-2"></i></button>
          <button class="btn-icon action-default" style="${acc.isDefault ? 'color: var(--text-muted); cursor: not-allowed; opacity: 0.5;' : ''}" data-index="${index}" title="${acc.isDefault ? '已置顶' : '设为默认 (置顶)'}" ${acc.isDefault ? 'disabled' : ''}><i data-lucide="arrow-up-circle"></i></button>
        </div>
      </div>
      <div class="account-right">
        <div class="account-password">
          <span class="password-text${acc.pass ? '' : ' password-missing'}" style="font-family: monospace; font-size: 0.9rem; color: var(--text-muted); display: inline-block; width: 7.5em; text-align: left;"></span>
          <button class="btn-icon action-toggle-password hide-password" style="padding: 0.2rem;" title="显示/隐藏密码"${acc.pass ? '' : ' disabled'}><i data-lucide="eye"></i></button>
        </div>
        <div class="account-desktop-actions">
          <button class="btn-icon action-edit" data-index="${index}" title="编辑"><i data-lucide="edit-2"></i></button>
          <button class="btn-icon action-default" style="${acc.isDefault ? 'color: var(--text-muted); cursor: not-allowed; opacity: 0.5;' : ''}" data-index="${index}" title="${acc.isDefault ? '已置顶' : '设为默认 (置顶)'}" ${acc.isDefault ? 'disabled' : ''}><i data-lucide="arrow-up-circle"></i></button>
        </div>
        <button class="btn-icon danger action-delete" data-index="${index}" title="删除"><i data-lucide="trash-2"></i></button>
      </div>
    `;
    item.querySelector('.account-avatar')!.textContent = avatarText;
    item.querySelector('.account-user h4')!.textContent = acc.user;
    const passwordText = item.querySelector('.password-text') as HTMLElement;
    passwordText.dataset.password = acc.pass;
    passwordText.textContent = acc.pass ? '*************' : '未保存密码';
    const health = accountHealthCache.get(acc.user);
    if (health) {
      const healthBadge = item.querySelector('.account-health-badge') as HTMLElement;
      const healthDetail = item.querySelector('.account-health-inline small') as HTMLElement;
      healthBadge.className = `account-health-badge ${health.status}`;
      healthBadge.textContent = accountHealthLabel(health.status);
      healthDetail.textContent = health.consecutiveFailures > 0
        ? `${formatCooldown(health.cooldownSeconds)} · 失败 ${health.consecutiveFailures} 次`
        : '暂无失败记录';
      healthDetail.title = health.lastFailureReason || '';
    }
    accountsList.appendChild(item);
  });
  createIcons({ icons });
  updateOverrideOptions();
}

function updateOverrideOptions() {
  const accounts = getAccounts();
  if (!overrideAccountSelect) return;
  const opts = [{ value: 'auto', text: '自动' }];
  accounts.forEach((acc, i) => {
    if (!acc.isDisabled) {
      opts.push({ value: i.toString(), text: `账号${i + 1} (${acc.user})` });
    }
  });
  opts.push({ value: 'add', text: '添加账号...' });
  overrideAccountSelect.setOptions(opts);
}

// Split Network Check Loops
async function isAppInBackground(): Promise<boolean> {
  if (!(window as any).__TAURI__) {
    return document.hidden;
  }
  try {
    const win = getCurrentWindow();
    const isVisible = await win.isVisible();
    const isMinimized = await win.isMinimized();
    return !isVisible || isMinimized;
  } catch (e) {
    return document.hidden;
  }
}

function startWifiChangeCheckLoop() {
  if ((window as any).__TAURI__) return;
  if (wifiChangeTimer) {
    clearTimeout(wifiChangeTimer);
    wifiChangeTimer = null;
  }
  if (!wifiChangeDetectEnabled) return;

  const tick = async () => {
    if ((window as any).__TAURI__) {
      try {
        const currentIp: string = await invoke('get_local_ip');
        log('网络', `[DEBUG] 执行 Wi-Fi 变更检测。当前 IP: ${currentIp || '未分配'} (上次 IP: ${lastKnownIp || '空'})`, 'debug');
        if (currentIp) {
          if (lastKnownIp && currentIp !== lastKnownIp) {
            log('网络', `检测到局域网 IP 发生变更: ${lastKnownIp} -> ${currentIp}，重新检测网络环境...`);
            isLoopSuspended = false;
            await checkNetwork();
          }
          lastKnownIp = currentIp;
        }
      } catch (e) {
        console.warn('Failed in Wi-Fi change check:', e);
      }
    }
    wifiChangeTimer = setTimeout(tick, 3000) as any;
  };

  tick();
}

// Native keep-alive hook for Android: called from Kotlin Handler every 10s
// to counteract Chromium's internal background timer throttling.
(window as any).__nativeKeepAlive = () => {
  if ((window as any).__TAURI__) return;
  // Re-kick the Wi-Fi change detection loop if its timer died
  if (wifiChangeDetectEnabled && !wifiChangeTimer) {
    startWifiChangeCheckLoop();
  }
  // Re-kick the connectivity countdown if its interval died
  if (!countdownInterval) {
    startConnectivityCheckLoop();
  }
};

function startConnectivityCheckLoop() {
  if ((window as any).__TAURI__) return;
  if (connectivityTimer) {
    clearTimeout(connectivityTimer);
    connectivityTimer = null;
  }
  if (countdownInterval) {
    clearInterval(countdownInterval);
    countdownInterval = null;
  }

  const runCheck = async () => {
    await checkNetwork();
  };

  countdownInterval = setInterval(async () => {
    if (isLoggingIn || isChecking) return;

    const isBg = await isAppInBackground();

    // Auto-resume when coming back to foreground
    if (!isBg && isLoopSuspended) {
      log('网络', '检测到已返回前台，恢复连通性检测...', 'info');
      isLoopSuspended = false;
      runCheck();
      return;
    }

    if (isLoopSuspended) {
      updateCountdownUI();
      return;
    }

    const intervalFg = parseInt(localStorage.getItem('bjut_check_interval') || '15', 10);
    const intervalBg = parseInt(localStorage.getItem('bjut_check_interval_bg') || '60', 10);
    const maxInterval = isBg ? intervalBg : intervalFg;

    if (secondsToNextCheck > maxInterval) {
      secondsToNextCheck = maxInterval;
      updateCountdownUI();
    }

    if (secondsToNextCheck > 0) {
      secondsToNextCheck--;
      updateCountdownUI();
      if (secondsToNextCheck === 0) {
        log('网络', `[DEBUG] 倒计时归零，触发自动网络连通性检测`, 'debug');
        runCheck();
      }
    }
  }, 1000) as any;

  runCheck();
}

function updateCountdownUI() {
  const countdownText = document.getElementById('countdown-text');
  if (countdownText) {
    if (isChecking) {
      countdownText.textContent = '检测中...';
    } else if (isLoopSuspended) {
      countdownText.textContent = '已休眠';
    } else {
      countdownText.textContent = secondsToNextCheck.toString();
    }
  }
}

async function checkNetwork() {
  if (!(window as any).__TAURI__) return;
  try {
    await invoke('trigger_manual_check');
  } catch (error) {
    console.error('Failed to request native network check:', error);
  }
}

function updateNetworkStatus(state: NetworkState, type?: LoginType) {
  currentNetworkState = state;
  networkIcon.className = 'status-icon';
  
  if (state === NetworkState.Online) {
    networkStatus.textContent = '互联网已连接';
    networkDetail.textContent = '网络畅通无阻';
    networkIcon.classList.add('success');
    networkIcon.innerHTML = '<i data-lucide="check-circle"></i>';
    btnLogin.disabled = true;
  } else if (state === NetworkState.BjutCampus) {
    networkStatus.textContent = '检测到校园网';
    networkDetail.textContent = `需要认证 (登录类型: ${type})`;
    networkIcon.classList.add('warning');
    networkIcon.innerHTML = '<i data-lucide="alert-circle"></i>';
    btnLogin.disabled = false;
  } else {
    networkStatus.textContent = '网络断开或非校园网';
    networkDetail.textContent = '无法访问互联网和校园网登录页';
    networkIcon.classList.add('error');
    networkIcon.innerHTML = '<i data-lucide="wifi-off"></i>';
    btnLogin.disabled = true;
    
    // Clear user info
    infoAccount.textContent = '未登录';
    infoBalance.textContent = '--';
    infoFlow.textContent = '--';
  }
  createIcons({ icons });
}

async function updateUserInfo() {
  const info: { account: string, balance: string, flow: string } | null = await invoke('get_user_info', { localIp: lastKnownIp || null });
  if (info) {
    infoAccount.textContent = info.account;
    infoBalance.textContent = info.balance;
    infoFlow.textContent = info.flow;
  } else {
    infoAccount.textContent = '--';
    infoBalance.textContent = '--';
    infoFlow.textContent = '--';
  }
}

async function manualLogin() {
  if (currentNetworkState !== NetworkState.BjutCampus) {
    customAlert('当前无需登录或未连接校园网');
    return;
  }
  
  if (getAccounts().length === 0) {
    customAlert('请先在账号管理中添加账号');
    return;
  }

  isLoggingIn = true;
  btnLogin.disabled = true;
  btnLogin.innerHTML = '<i data-lucide="loader"></i> 安全检查中...';
  createIcons({ icons });
  
  const isSafe = await checkNetworkSecurity();
  if (!isSafe) {
    log('安全', '已取消登录：安全检查未通过', 'error');
    isLoggingIn = false;
    btnLogin.disabled = false;
    btnLogin.innerHTML = '<i data-lucide="log-in"></i> 立即登录';
    createIcons({ icons });
    return;
  }
  
  btnLogin.innerHTML = '<i data-lucide="loader"></i> 登录中...';
  createIcons({ icons });
  
  let overrideAcc = overrideAccountSelect?.value || 'auto';
  let overrideMethod = overrideMethodSelect?.value || 'auto';
  
  const accountIndex = overrideAcc !== 'auto' && overrideAcc !== 'add' ? parseInt(overrideAcc, 10) : null;
  try {
    const result: { success: boolean, message: string } = await invoke('manual_login', {
      accountIndex: Number.isNaN(accountIndex) ? null : accountIndex,
      loginTypeOverride: overrideMethod === 'auto' ? null : overrideMethod
    });
    if (!result.success) {
      log('登录', `登录失败: ${result.message}`, 'error');
      btnLogin.disabled = false;
      btnLogin.innerHTML = '<i data-lucide="log-in"></i> 立即登录';
      return;
    }
    btnLogin.innerHTML = '<i data-lucide="check"></i> 已连接';
    updateNetworkStatus(NetworkState.Online);
    setTimeout(updateUserInfo, 2000);
  } catch (error) {
    log('登录', `登录请求失败: ${String(error)}`, 'error');
    btnLogin.disabled = false;
    btnLogin.innerHTML = '<i data-lucide="log-in"></i> 立即登录';
  } finally {
    createIcons({ icons });
    isLoggingIn = false;
  }
}

async function checkNetworkSecurity(): Promise<boolean> {
  if (!(window as any).__TAURI__) return true; 

  try {
    const netInfo: { ssid: string, bssid: string, ip: string } = await invoke('get_network_info');
    if (!netInfo || (!netInfo.ssid && !netInfo.ip)) {
      log('安全', '无法获取当前网络身份，已阻止发送账号密码', 'error');
      return false;
    }
    
    const normalizedSsid = netInfo.ssid.trim().toLowerCase().replaceAll('_', '-');
    const isBjutWifi = normalizedSsid === 'bjut-wifi' || normalizedSsid === 'bjut-sushe';
    let isSafe = false;
    
    // Check vlan
    if (netInfo.ip) {
      const parts = netInfo.ip.split('.');
      if (parts.length === 4) {
        const p1 = parseInt(parts[0], 10);
        const p2 = parseInt(parts[1], 10);
        
        if (p1 === 10) {
          if ((p2 >= 17 && p2 <= 27) || p2 === 121 || p2 === 126 || p2 === 226) {
            isSafe = true;
          }
        } else if (p1 === 172) {
          if (p2 >= 17 && p2 <= 27) {
            isSafe = true;
          }
        }
      }
    }
    
    if (isBjutWifi && isSafe) return true;
    if (!isBjutWifi && netInfo.ssid !== '<unknown ssid>') {
      isSafe = false; // direct unsafe if completely unmatching
    }
    if (isSafe) return true;
    
    const netKey = `${netInfo.ssid}|${netInfo.bssid}`;
    const whitelist: string[] = JSON.parse(localStorage.getItem('bjut_whitelist') || '[]');
    const blacklist: string[] = JSON.parse(localStorage.getItem('bjut_blacklist') || '[]');
    
    if (whitelist.includes(netKey)) return true;
    if (blacklist.includes(netKey)) return false;
    
    // Prompt
    return new Promise(resolve => {
      const modal = document.getElementById('security-modal')!;
      document.getElementById('sec-ssid')!.textContent = netInfo.ssid;
      document.getElementById('sec-bssid')!.textContent = netInfo.bssid;
      document.getElementById('sec-ip')!.textContent = netInfo.ip;
      
      const cleanup = () => {
        modal.classList.add('hidden');
        btnCancel.removeEventListener('click', onCancel);
        btnCancelBlack.removeEventListener('click', onCancelBlack);
        btnTrustOnce.removeEventListener('click', onTrustOnce);
        btnTrustWhite.removeEventListener('click', onTrustWhite);
      };
      
      const btnCancel = document.getElementById('btn-sec-cancel')!;
      const onCancel = () => { cleanup(); resolve(false); };
      btnCancel.addEventListener('click', onCancel);
      
      const btnCancelBlack = document.getElementById('btn-sec-cancel-black')!;
      const onCancelBlack = () => {
        blacklist.push(netKey);
        localStorage.setItem('bjut_blacklist', JSON.stringify(blacklist));
        log('安全', `已将 ${netInfo.ssid} 加入黑名单`, 'info');
        cleanup(); resolve(false);
      };
      btnCancelBlack.addEventListener('click', onCancelBlack);
      
      const btnTrustOnce = document.getElementById('btn-sec-trust-once')!;
      const onTrustOnce = () => { cleanup(); resolve(true); };
      btnTrustOnce.addEventListener('click', onTrustOnce);
      
      const btnTrustWhite = document.getElementById('btn-sec-trust-white')!;
      const onTrustWhite = () => {
        whitelist.push(netKey);
        localStorage.setItem('bjut_whitelist', JSON.stringify(whitelist));
        log('安全', `已将 ${netInfo.ssid} 加入白名单`, 'info');
        cleanup(); resolve(true);
      };
      btnTrustWhite.addEventListener('click', onTrustWhite);
      
      modal.classList.remove('hidden');
    });
  } catch (e) {
    console.error('Security check error', e);
    log('安全', '网络安全检查失败，已阻止发送账号密码', 'error');
    return false;
  }
}

// Start
void init().catch(async error => {
  console.error('Application initialization failed:', error);
  await finishAppLaunch();
  await customAlert(`应用初始化失败：${String(error)}`);
});
