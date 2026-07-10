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
function getAccounts(): any[] {
  return accountsCache;
}
function saveAccounts(accs: any[]) {
  accountsCache = accs;
  syncConfigToRust();
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
  syncConfigToRust();
}

async function syncConfigToRust() {
  if (!(window as any).__TAURI__) return;
  try {
    const config = {
      accounts: getAccounts(),
      auto_login: localStorage.getItem('bjut_auto_login') === 'true',
      check_interval: parseInt(localStorage.getItem('bjut_check_interval') || '15', 10),
      check_interval_bg: parseInt(localStorage.getItem('bjut_check_interval_bg') || '60', 10),
      wifi_change_detect: localStorage.getItem('bjut_wifi_change_detect') !== 'false',
      log_level: localStorage.getItem('bjut_log_level') || 'info',
      whitelist: JSON.parse(localStorage.getItem('bjut_whitelist') || '[]'),
      blacklist: JSON.parse(localStorage.getItem('bjut_blacklist') || '[]')
    };
    await invoke('sync_config', { config });
  } catch (e) {
    console.error('Failed to sync config to Rust:', e);
  }
}

async function loadConfigFromRust() {
  if (!(window as any).__TAURI__) return;
  try {
    const config: any = await invoke('get_app_config');
    if (!config) return;

    accountsCache = config.accounts || [];
    localStorage.setItem('bjut_auto_login', config.auto_login.toString());
    localStorage.setItem('bjut_check_interval', config.check_interval.toString());
    localStorage.setItem('bjut_check_interval_bg', config.check_interval_bg.toString());
    localStorage.setItem('bjut_wifi_change_detect', config.wifi_change_detect.toString());
    localStorage.setItem('bjut_log_level', config.log_level);
    localStorage.setItem('bjut_whitelist', JSON.stringify(config.whitelist || []));
    localStorage.setItem('bjut_blacklist', JSON.stringify(config.blacklist || []));
    
    autoLoginEnabled = config.auto_login;
    checkInterval = config.check_interval;
    wifiChangeDetectEnabled = config.wifi_change_detect;
    
    settingAutoLogin.checked = autoLoginEnabled;
    settingWifiChangeDetect.checked = wifiChangeDetectEnabled;
    settingCheckInterval.value = checkInterval.toString();
    settingLogLevel.value = config.log_level;
    
    const settingCheckIntervalBg = document.getElementById('setting-check-interval-bg') as HTMLInputElement;
    if (settingCheckIntervalBg) {
      settingCheckIntervalBg.value = config.check_interval_bg.toString();
    }
    
    renderAccounts();
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

    // Load initial logs
    const initialLogs: any[] = await invoke('get_logs');
    logsContent.innerHTML = '';
    initialLogs.forEach(entry => {
      renderLogEntry(entry.module, entry.message, entry.type, entry.time);
    });

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
  
  const logsPageActive = logsContent.closest('.page')?.classList.contains('active') ?? false;
  const scroller = getLogsScroller();
  const wasAtBottom = !scroller || scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop <= 80;
  const timeStr = time || new Date().toLocaleString();
  const entry = document.createElement('div');
  entry.className = message.includes('=== SESSION START ===') ? 'log-entry log-session-divider' : 'log-entry';
  const timeElement = document.createElement('span');
  timeElement.className = 'log-time';
  timeElement.textContent = `[${timeStr}]`;
  const messageElement = document.createElement('span');
  messageElement.className = `log-${type}`;
  messageElement.textContent = `[${module}] ${message.replace('=== SESSION START ===', '启动会话')}`;
  entry.append(timeElement, messageElement);
  logsContent.appendChild(entry);
  
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
      div.innerHTML = `
        <span style="word-break: break-all;">${item}</span>
        <button class="btn-icon danger" style="padding: 0 0.5rem;" data-idx="${index}"><i data-lucide="trash-2"></i></button>
      `;
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

    // Toggle open
    this.trigger.addEventListener('click', (e) => {
      e.stopPropagation();
      // Close other dropdowns first
      document.querySelectorAll('.custom-select').forEach(el => {
        if (el !== this.element) el.classList.remove('open');
      });
      this.element.classList.toggle('open');
    });

    // Handle options click
    this.optionsContainer.addEventListener('click', (e) => {
      const option = (e.target as HTMLElement).closest('.custom-option') as HTMLElement;
      if (option) {
        const val = option.getAttribute('data-value') || '';
        this.value = val;
        this.element.classList.remove('open');
        this.onChangeCallbacks.forEach(cb => cb(val));
      }
    });

    // Close on click outside
    document.addEventListener('click', () => {
      this.element.classList.remove('open');
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
        selectedText = opt.textContent || '';
      } else {
        opt.classList.remove('selected');
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
      div.setAttribute('data-value', opt.value);
      div.textContent = opt.text;
      if (opt.value === this._value) {
        div.classList.add('selected');
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
const settingAutoLogin = document.getElementById('setting-auto-login') as HTMLInputElement;
const settingWifiChangeDetect = document.getElementById('setting-wifi-change-detect') as HTMLInputElement;
const settingAutostart = document.getElementById('setting-autostart') as HTMLInputElement;
const settingCheckInterval = document.getElementById('setting-check-interval') as HTMLInputElement;
let settingLogLevel: CustomSelect;
let overrideAccountSelect: CustomSelect;
let overrideMethodSelect: CustomSelect;
let settingUpdateChannel: CustomSelect;

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

// New state for split check loops
let lastKnownIp = '';
let wifiChangeTimer: number | null = null;
let wifiChangeDetectEnabled = localStorage.getItem('bjut_wifi_change_detect') !== 'false';
let connectivityTimer: number | null = null;
let secondsToNextCheck = 0;
let countdownInterval: number | null = null;
let isLoopSuspended = false;

// Initialize
function init() {
  // Instantiate Custom Selects
  overrideAccountSelect = new CustomSelect('override-account');
  overrideMethodSelect = new CustomSelect('override-method');
  settingUpdateChannel = new CustomSelect('setting-update-channel');
  settingLogLevel = new CustomSelect('setting-log-level');

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
  
  setupNavigation();
  setupEventListeners();
  renderAccounts();

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
          if ((window as any).AndroidBridge) {
            (window as any).AndroidBridge.requestForegroundPermissions();
          } else {
            await invoke('request_foreground_permissions');
          }
          log('系统', '已申请前台网络定位相关权限');
        } catch (e) {
          console.error('Failed to request foreground permissions:', e);
        }
        // Load the secure backend configuration before the first sync so accepting the
        // terms cannot overwrite an existing account set with the empty WebView cache.
        await loadConfigFromRust();
        await syncConfigToRust();
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
      if ((window as any).AndroidBridge) {
        if (autoLoginEnabled) {
          try {
            (window as any).AndroidBridge.startKeepAliveService();
          } catch (e) {
            console.error('Failed to start keep-alive service:', e);
          }
        }
      } else {
        invoke('request_foreground_permissions').catch(e => {
          console.error('Failed to request foreground permissions:', e);
        });
      }
      loadConfigFromRust().then(() => {
        listenToRustEvents();
        log('系统', '应用启动');
        if ((window as any).AndroidBridge && autoLoginEnabled) {
          log('系统', '后台保活服务已启动');
        }
      });
    } else {
      startWifiChangeCheckLoop();
      startConnectivityCheckLoop();
      log('系统', '应用启动');
    }
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
    });
  });
}

// Event Listeners
function setupEventListeners() {
  btnLogin.addEventListener('click', manualLogin);
  
  // Add modal toggle
  btnShowAdd.addEventListener('click', () => {
    addModal.classList.remove('hidden');
  });
  btnCancelAdd.addEventListener('click', () => {
    addModal.classList.add('hidden');
    addAccountForm.reset();
  });

  addAccountForm.addEventListener('submit', (e) => {
    e.preventDefault();
    const user = (document.getElementById('acc-username') as HTMLInputElement).value.trim();
    const pass = (document.getElementById('acc-password') as HTMLInputElement).value;
    
    if (user && pass) {
      const accounts = getAccounts();
      if (accounts.find((a:any) => a.user === user)) {
        customAlert('该账号已存在');
        return;
      }
      accounts.push({ user, pass, isDefault: accounts.length === 0 });
      saveAccounts(accounts);
      renderAccounts();
      addAccountForm.reset();
      addModal.classList.add('hidden');
      log('账号管理', `已添加账号: ${user}`);
    }
  });

  btnClearLogs.addEventListener('click', () => {
    logsContent.innerHTML = '';
    if ((window as any).__TAURI__) {
      invoke('clear_all_logs').catch(e => console.error(e));
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
    settingMacosDock.checked = dockEnabled;
    if ((window as any).__TAURI__) {
      invoke('set_dock_visible', { visible: dockEnabled }).catch(e => console.error(e));
    }
    settingMacosDock.addEventListener('change', async (e) => {
      const enabled = (e.target as HTMLInputElement).checked;
      localStorage.setItem('bjut_macos_dock', enabled.toString());
      log('设置', `已${enabled ? '启用' : '关闭'}在程序坞显示图标`);
      if ((window as any).__TAURI__) {
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

        log('系统', `发现新版本: ${latestRelease.tag_name}，匹配安装包 ${asset.name}`, 'success');
        const confirmed = await showUpdateDialog(latestRelease, target, asset);
        if (!confirmed) return;

        try {
          await invoke('download_and_install_update', {
            url: asset.browser_download_url,
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
          moreOptions: localStorage.getItem('bjut_more_options')
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

  editAccountForm.addEventListener('submit', (e) => {
    e.preventDefault();
    const index = parseInt(editAccIndex.value, 10);
    const user = editAccUsername.value.trim();
    const pass = editAccPassword.value;

    if (user && pass && !isNaN(index)) {
      const accounts = getAccounts();
      if (accounts.findIndex((a:any, i) => a.user === user && i !== index) !== -1) {
        customAlert('该账号名已存在');
        return;
      }
      accounts[index].user = user;
      accounts[index].pass = pass;
      saveAccounts(accounts);
      renderAccounts();
      editModal.classList.add('hidden');
      log('账号管理', `已修改账号: ${user}`);
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
          saveAccounts(accounts);
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
      saveAccounts(accounts);
      
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
        saveAccounts(accounts);
        renderAccounts();
        deleteModal.classList.add('hidden');
      }
    });
  }

  // Drag and Drop using SortableJS

  Sortable.create(accountsList, {
    handle: '.drag-handle',
    animation: 300,
    easing: "cubic-bezier(0.25, 1, 0.5, 1)",
    ghostClass: 'dragging',
    forceFallback: true,
    fallbackClass: 'dragging-fallback',
    fallbackOnBody: true,
    onEnd: (evt) => {
      const { oldIndex, newIndex, item } = evt;
      
      // Custom drop animation for the dropped item
      item.style.transition = 'none';
      item.style.transform = 'scale(1.05) translateY(-5px)';
      item.style.boxShadow = '0 10px 25px rgba(0,0,0,0.2)';
      item.style.zIndex = '100';
      
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          item.style.transition = 'transform 0.3s cubic-bezier(0.34, 1.56, 0.64, 1), box-shadow 0.3s ease';
          item.style.transform = 'scale(1) translateY(0)';
          item.style.boxShadow = '';
          
          setTimeout(() => {
            item.style.transition = '';
            item.style.transform = '';
            item.style.zIndex = '';
          }, 300);
        });
      });

      if (oldIndex !== undefined && newIndex !== undefined && oldIndex !== newIndex) {
        // Wait for all Sortable slide animations to completely finish before updating DOM texts
        // so we don't interrupt the transform transitions of sibling elements.
        setTimeout(() => {
          const accounts = getAccounts();
          const accItem = accounts.splice(oldIndex, 1)[0];
          accounts.splice(newIndex, 0, accItem);
          accounts.forEach((a:any, i:number) => a.isDefault = (i === 0));
          saveAccounts(accounts);
          
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
        }, 310);
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
        <div class="account-avatar">${avatarText}</div>
        <div class="account-user">
          <h4>${acc.user}</h4>
          <span class="account-badge ${acc.isDefault ? 'text-primary font-bold' : 'text-muted'}">${acc.isDefault ? '默认' : '备用'}</span>
        </div>
        <div class="account-mobile-actions">
          <button class="btn-icon action-edit" data-index="${index}" title="编辑"><i data-lucide="edit-2"></i></button>
          <button class="btn-icon action-default" style="${acc.isDefault ? 'color: var(--text-muted); cursor: not-allowed; opacity: 0.5;' : ''}" data-index="${index}" title="${acc.isDefault ? '已置顶' : '设为默认 (置顶)'}" ${acc.isDefault ? 'disabled' : ''}><i data-lucide="arrow-up-circle"></i></button>
        </div>
      </div>
      <div class="account-right">
        <div class="account-password">
          <span class="password-text" data-password="${acc.pass}" style="font-family: monospace; font-size: 0.9rem; color: var(--text-muted); display: inline-block; width: 7.5em; text-align: left;">*************</span>
          <button class="btn-icon action-toggle-password hide-password" style="padding: 0.2rem;" title="显示/隐藏密码"><i data-lucide="eye"></i></button>
        </div>
        <div class="account-desktop-actions">
          <button class="btn-icon action-edit" data-index="${index}" title="编辑"><i data-lucide="edit-2"></i></button>
          <button class="btn-icon action-default" style="${acc.isDefault ? 'color: var(--text-muted); cursor: not-allowed; opacity: 0.5;' : ''}" data-index="${index}" title="${acc.isDefault ? '已置顶' : '设为默认 (置顶)'}" ${acc.isDefault ? 'disabled' : ''}><i data-lucide="arrow-up-circle"></i></button>
        </div>
        <button class="btn-icon danger action-delete" data-index="${index}" title="删除"><i data-lucide="trash-2"></i></button>
      </div>
    `;
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

let campusSubnets: Set<string> | null = null;
async function loadSubnets() {
  if (!campusSubnets) {
    try {
      const res = await fetch('/src/assets/subnets.json');
      if (res.ok) {
        const data = await res.json();
        campusSubnets = new Set(data);
      } else {
        campusSubnets = new Set();
      }
    } catch (e) {
      campusSubnets = new Set();
    }
  }
}

async function checkNetworkSecurity(): Promise<boolean> {
  if (!(window as any).__TAURI__) return true; 

  try {
    const netInfo: { ssid: string, bssid: string, ip: string } = await invoke('get_network_info');
    if (!netInfo || (!netInfo.ssid && !netInfo.ip)) return true;
    
    await loadSubnets();
    const isBjutWifi = netInfo.ssid.includes('bjut-wifi') || netInfo.ssid.includes('bjut_sushe');
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
    return true; // Fail open if native not implemented yet
  }
}

// Start
init();
