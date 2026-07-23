import {
  Activity, AlertCircle, ArrowDownToLine, ArrowLeft, ArrowRight, ArrowUpCircle, BarChart2, Check, CheckCircle, ChevronDown, ChevronUp,
  ChevronLeft, ChevronRight, ClipboardCopy, ClipboardPaste, Clock, Copy, createIcons, CreditCard, Download, Edit2, ExternalLink, Eye, FileText, GripVertical,
  Fingerprint, History, Home, LayoutDashboard, Loader, LogIn, Minus, MonitorSmartphone, Plus, Power, Smartphone,
  QrCode, ReceiptText, RefreshCw, Search, Settings, ShieldAlert, ShieldCheck, Square, Trash2, User, Users, Wifi,
  WalletCards, WifiOff, X,
} from 'lucide';
import Sortable from 'sortablejs';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  credentialSnapshotFingerprint, hasLegacyCredentialConflict, LEGACY_ACCOUNTS_KEY,
  LEGACY_MIGRATION_PENDING_KEY, mergeLegacyAccounts, readLegacyAccounts,
} from './account-migration';
import { decryptExport, encryptExport } from './config-crypto';
import { CustomSelect } from './custom-select';
import { IS_ANDROID, IS_WINDOWS, readTextFromClipboard, writeTextToClipboard } from './platform';
import { formatBytes, isVersionNewer, renderReleaseNotes, selectUpdateAsset } from './update-utils';
import type {
  AccountView, ActiveAlipayPayment, ActiveWechatPayment, AlipayRechargePreview,
  AlipayRechargeResult, AppLogEntry, BackendConfig, BillingActionRequest,
  BillingActionResult, BillingCenterData, BillingPasswordPolicy, BillingQuestionAnswer,
  BillingLoginRecord, BillingOnlineSession, BillingOverview, BillingPackageOption,
  BillingRecordKind, BillingRecordQuery, BillingRecordQueryState, BillingRecordResult,
  BillingServiceState, BillingTable, CountdownPayload, CredentialStorageHealth,
  DiagnosticReport, DiscoveredCampusAccount, GitHubRelease, GitHubReleaseAsset,
  NetworkStatePayload,
  NetworkProfile, RechargeBalanceSnapshot, RechargePreview, RechargeResult,
  RecoverableRecharge, UpdateProgress, UpdateTarget, UserInfo, WechatPaymentStatus,
  WechatRechargePreview, WechatRechargeResult, AccountHealth,
} from './models';

const icons = {
  Activity, AlertCircle, ArrowDownToLine, ArrowLeft, ArrowRight, ArrowUpCircle, BarChart2, Check, CheckCircle, ChevronDown, ChevronUp,
  ChevronLeft, ChevronRight, ClipboardCopy, ClipboardPaste, Clock, Copy, CreditCard, Download, Edit2, ExternalLink, Eye, FileText, GripVertical,
  Fingerprint, History, Home, LayoutDashboard, Loader, LogIn, Minus, MonitorSmartphone, Plus, Power, Smartphone,
  QrCode, ReceiptText, RefreshCw, Search, Settings, ShieldAlert, ShieldCheck, Square, Trash2, User, Users, Wifi,
  WalletCards, WifiOff, X,
};

function renderIcons(root: Element | Document | DocumentFragment = document) {
  createIcons({ icons, root });
}

if (IS_ANDROID) {
  document.body.classList.add('is-android');
} else {
  document.body.classList.add('is-desktop');
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

function clearTransientWebviewPasswords() {
  document.querySelectorAll<HTMLElement>('.password-text').forEach(element => {
    if (element.textContent !== '*************') element.textContent = '*************';
  });
  document.querySelectorAll('.action-toggle-password').forEach(button => button.classList.add('hide-password'));
  document.querySelectorAll<HTMLInputElement>('input[type="password"]').forEach(input => {
    input.value = '';
  });
}

async function finishAppLaunch() {
  if (appLaunchRevealed) return;
  await macosDockPolicyReady;
  await new Promise(resolve => window.setTimeout(resolve, 40));
  appLaunchRevealed = true;
  setLoadingMaskVisible(false);
  if (window.__TAURI__) {
    await invoke('frontend_ready').catch(error => console.error('Failed to reveal main window:', error));
  }
}

window.__showResumeMask = showResumeMask;
let documentWasHidden = document.hidden;
document.addEventListener('visibilitychange', () => {
  if (document.hidden) clearTransientWebviewPasswords();
  if (documentWasHidden && !document.hidden) {
    showResumeMask();
    scheduleAlipayAutomaticCompletionCheck();
    scheduleWechatAutomaticCompletionCheck();
  }
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


// Passwords are normally absent from this cache. A password may enter a form
// for a single edit/display operation and is erased immediately afterwards.
let accountsCache: AccountView[] = [];
const passwordRevealTimers = new WeakMap<HTMLElement, number>();
let whitelistCache: string[] = [];
let blacklistCache: string[] = [];
let vpnMaximumUntil = 0;
let vpnMaximumRollbackTimer: number | null = null;
let configSyncQueue: Promise<void> = Promise.resolve();
let missingPasswordWarningShown = false;
function warnAboutMissingPasswords(storageStatus = 'missing') {
  const missingPasswordUsers = accountsCache
    .filter(account => !account.hasPassword && !account.pass)
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

function getAccounts(): AccountView[] {
  return accountsCache;
}
async function saveAccounts(accs: AccountView[]): Promise<void> {
  accountsCache = accs;
  try {
    await syncConfigToRust();
  } finally {
    accs.forEach(account => {
      account.hasPassword = account.hasPassword || Boolean(account.pass);
      account.pass = '';
    });
    accountsCache = accs;
  }
}

function saveAccountsInBackground(accs: AccountView[]) {
  void saveAccounts(accs).catch(async error => {
    console.error('Failed to persist accounts:', error);
    await loadConfigFromRust();
    renderAccounts();
    await customAlert(`账号保存失败，已恢复上次保存的内容：${String(error)}`);
  });
}

function scheduleCurrentCampusAccountDiscovery() {
  if (!window.__TAURI__ || currentNetworkState !== NetworkState.Online) return;
  if (campusAccountDiscoveryPromise) return;
  const now = Date.now();
  if (now - lastCampusAccountDiscoveryAt < 60_000) return;
  lastCampusAccountDiscoveryAt = now;
  campusAccountDiscoveryPromise = discoverAndOfferCurrentCampusAccount()
    .catch(error => console.debug('Current campus account discovery skipped:', error))
    .finally(() => { campusAccountDiscoveryPromise = null; });
}

async function discoverAndOfferCurrentCampusAccount() {
  const candidate = await invoke<DiscoveredCampusAccount | null>('discover_current_campus_account');
  if (!candidate) return;
  if (!candidate.user || !candidate.token) return;
  if (accountsCache.some(account => account.user === candidate.user)) {
    await invoke('reject_discovered_campus_account', { token: candidate.token }).catch(() => undefined);
    localStorage.removeItem(FIRST_LAUNCH_ACCOUNT_DISCOVERY_KEY);
    return;
  }
  if (sessionStorage.getItem(DISMISSED_DISCOVERED_ACCOUNT_KEY) === candidate.user) {
    await invoke('reject_discovered_campus_account', { token: candidate.token }).catch(() => undefined);
    return;
  }

  const firstLaunch = localStorage.getItem(FIRST_LAUNCH_ACCOUNT_DISCOVERY_KEY) === 'true';
  const confirmed = await customConfirm(
    `${firstLaunch ? '首次联网时' : '本次联网时'}检测到当前已登录的校园网账号 ${candidate.user}，但它不在账号列表中。\n\n是否从当前计费系统会话读取凭据，并保存到 App 的安全凭据存储？`,
    '添加当前校园网账号',
  );
  localStorage.removeItem(FIRST_LAUNCH_ACCOUNT_DISCOVERY_KEY);
  if (!confirmed) {
    sessionStorage.setItem(DISMISSED_DISCOVERED_ACCOUNT_KEY, candidate.user);
    await invoke('reject_discovered_campus_account', { token: candidate.token }).catch(() => undefined);
    return;
  }

  try {
    await invoke<void>('accept_discovered_campus_account', {
      user: candidate.user,
      token: candidate.token,
    });
    await loadConfigFromRust();
    sessionStorage.removeItem(DISMISSED_DISCOVERED_ACCOUNT_KEY);
    renderAccounts();
    await customAlert(`账号 ${candidate.user} 已保存到安全凭据存储。`, '账号已添加');
  } catch (error) {
    await loadConfigFromRust();
    renderAccounts();
    await customAlert(`当前账号保存失败，账号列表未更改：${String(error)}`, '添加账号失败');
  }
}

interface PermissionHealthItem {
  id: string;
  label: string;
  granted: boolean;
  required: boolean;
  detail?: string;
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

function renderAndroidNotificationModeDescription() {
  const separated = settingAndroidNotificationMode?.value === 'separate';
  androidNotificationModeDescription.textContent = separated
    ? '常驻通知只负责保活且不再更新；状态与结果改用可清除的独立通知'
    : '常驻通知同时显示后台检测与自动登录状态';
}

function refreshAndroidNotificationSettings() {
  if (!IS_ANDROID) return;
  try {
    window.AndroidBridge?.refreshNotificationSettings?.();
  } catch (error) {
    console.error('Failed to refresh Android notification settings:', error);
  }
}

function saveAndroidNotificationSetting(key: string, value: string) {
  localStorage.setItem(key, value);
  void syncConfigToRust()
    .then(refreshAndroidNotificationSettings)
    .catch(error => {
      console.error(`Failed to persist Android notification setting ${key}:`, error);
      void customAlert(`通知设置保存失败：${String(error)}`);
    });
}

function saveUsageAlertSetting(enabled: boolean) {
  settingUsageAlerts.checked = enabled;
  settingAndroidNotifyUsageAlerts.checked = enabled;
  saveAndroidNotificationSetting('bjut_usage_alerts', String(enabled));
}

function scheduleVpnMaximumRollback() {
  if (vpnMaximumRollbackTimer !== null) window.clearTimeout(vpnMaximumRollbackTimer);
  vpnMaximumRollbackTimer = null;
  if (!vpnMaximumUntil) return;
  const delay = Math.max(0, vpnMaximumUntil * 1000 - Date.now());
  vpnMaximumRollbackTimer = window.setTimeout(() => {
    vpnMaximumRollbackTimer = null;
    vpnMaximumUntil = 0;
    localStorage.setItem('bjut_vpn_compatibility', 'high');
    settingVpnCompatibility.value = 'high';
    void syncConfigToRust();
    log('安全', '最高 VPN 兼容模式已到期，自动回退为高兼容 HTTPS 模式', 'info');
    void customAlert('最高 VPN 兼容模式已自动关闭，当前已恢复为“高兼容（HTTPS + 固定地址）”。', '安全模式已回退');
  }, Math.min(delay, 2_147_000_000));
}

function syncConfigToRust(): Promise<void> {
  if (!window.__TAURI__) return Promise.resolve();
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
    vpn_compatibility: localStorage.getItem('bjut_vpn_compatibility') || 'high',
    vpn_maximum_until: vpnMaximumUntil || null,
    whitelist: [...whitelistCache],
    blacklist: [...blacklistCache],
    network_profiles: networkProfilesCache.map(profile => ({ ...profile, account_order: [...profile.account_order] })),
    usage_alerts: localStorage.getItem('bjut_usage_alerts') !== 'false',
    balance_alert_threshold: Number.isFinite(balanceThreshold) ? balanceThreshold : 10,
    flow_alert_threshold: Number.isFinite(flowThreshold) ? flowThreshold : 5,
    android_notification_mode: localStorage.getItem('bjut_android_notification_mode') === 'separate' ? 'separate' : 'combined',
    android_notify_network_status: localStorage.getItem('bjut_android_notify_network_status') !== 'false',
    android_notify_login_results: localStorage.getItem('bjut_android_notify_login_results') !== 'false',
    android_notify_background_errors: localStorage.getItem('bjut_android_notify_background_errors') !== 'false',
  };
  const operation = configSyncQueue
    .catch(() => {})
    .then(async () => {
      await invoke<void>('sync_config', { config });
      // Record the original recoverable snapshot once. The next cold start
      // asks Rust to hash the actual secure credentials and only deletes the
      // legacy copy when both fingerprints match.
      const legacyAccounts = readLegacyAccounts();
      if (legacyAccounts !== null && localStorage.getItem(LEGACY_MIGRATION_PENDING_KEY) === null) {
        const fingerprint = await credentialSnapshotFingerprint(legacyAccounts);
        localStorage.setItem(LEGACY_MIGRATION_PENDING_KEY, fingerprint);
      }
    })
    .finally(() => {
      config.accounts.forEach(account => { account.pass = ''; });
      accountsCache = accountsCache.map(account => ({
        ...account,
        pass: '',
        hasPassword: account.hasPassword || Boolean(account.pass),
      }));
    });
  configSyncQueue = operation;
  return operation;
}

async function loadConfigFromRust() {
  if (!window.__TAURI__) return;
  try {
    const [config, credentialStorageStatus] = await Promise.all([
      invoke<BackendConfig>('get_app_config'),
      invoke<string>('get_credential_storage_status'),
    ]);
    if (!config) return;

    const backendAccounts: AccountView[] = (config.accounts || []).map((account: Partial<AccountView>) => ({
      user: String(account.user || ''),
      pass: '',
      hasPassword: account.hasPassword === true,
      isDefault: account.isDefault === true,
      isDisabled: account.isDisabled === true,
    }));
    const legacyAccounts = readLegacyAccounts();
    const legacyConflict = legacyAccounts !== null
      && hasLegacyCredentialConflict(backendAccounts, legacyAccounts);
    const pendingFingerprint = localStorage.getItem(LEGACY_MIGRATION_PENDING_KEY);
    const migrationWasPending = pendingFingerprint !== null;
    const secureBackendCoversLegacy = legacyAccounts !== null
      && legacyAccounts
        .filter(account => account.pass)
        .every(account => backendAccounts.some(saved => saved.user === account.user && saved.hasPassword));
    const migrationFingerprintMatches = credentialStorageStatus === 'available'
      && legacyAccounts !== null
      && pendingFingerprint !== null
      && await invoke<boolean>('verify_legacy_credential_fingerprint', {
        users: legacyAccounts.map(account => account.user),
        fingerprint: pendingFingerprint,
      }).catch(() => false);
    const migrationConfirmed = credentialStorageStatus === 'available'
      && secureBackendCoversLegacy
      && migrationFingerprintMatches;
    const backendIsAuthoritative = credentialStorageStatus === 'available'
      && migrationWasPending;
    const merged = legacyAccounts === null || backendIsAuthoritative
      ? { accounts: backendAccounts, changed: false }
      : mergeLegacyAccounts(backendAccounts, legacyAccounts);
    accountsCache = (backendIsAuthoritative ? backendAccounts : merged.accounts).map(account => ({
      ...account,
      pass: account.pass || '',
      hasPassword: account.hasPassword === true || Boolean(account.pass),
      isDefault: account.isDefault === true,
      isDisabled: account.isDisabled === true,
    }));
    localStorage.setItem('bjut_auto_login', config.auto_login.toString());
    localStorage.setItem('bjut_check_interval', config.check_interval.toString());
    localStorage.setItem('bjut_check_interval_bg', config.check_interval_bg.toString());
    localStorage.setItem('bjut_wifi_change_detect', config.wifi_change_detect.toString());
    localStorage.setItem('bjut_log_level', config.log_level);
    const loadedVpnMode = config.vpn_compatibility || 'high';
    vpnMaximumUntil = Number(config.vpn_maximum_until || 0);
    localStorage.setItem('bjut_vpn_compatibility', loadedVpnMode);
    if (loadedVpnMode === 'maximum' && vpnMaximumUntil <= Date.now() / 1000) {
      vpnMaximumUntil = 0;
      localStorage.setItem('bjut_vpn_compatibility', 'high');
      config.vpn_compatibility = 'high';
      config.vpn_maximum_until = null;
      await syncConfigToRust();
    }
    const legacyWhitelist = JSON.parse(localStorage.getItem('bjut_whitelist') || '[]');
    const legacyBlacklist = JSON.parse(localStorage.getItem('bjut_blacklist') || '[]');
    whitelistCache = Array.isArray(config.whitelist) && config.whitelist.length
      ? [...config.whitelist]
      : Array.isArray(legacyWhitelist) ? legacyWhitelist : [];
    blacklistCache = Array.isArray(config.blacklist) && config.blacklist.length
      ? [...config.blacklist]
      : Array.isArray(legacyBlacklist) ? legacyBlacklist : [];
    if (localStorage.getItem('bjut_whitelist') !== null || localStorage.getItem('bjut_blacklist') !== null) {
      await syncConfigToRust();
      localStorage.removeItem('bjut_whitelist');
      localStorage.removeItem('bjut_blacklist');
      log('配置', 'Wi-Fi 信任规则已迁移到加密存储', 'success');
    }
    networkProfilesCache = (config.network_profiles || []).map((profile: NetworkProfile) => ({ ...profile }));
    localStorage.setItem('bjut_usage_alerts', String(config.usage_alerts !== false));
    localStorage.setItem('bjut_balance_alert_threshold', String(config.balance_alert_threshold ?? 10));
    localStorage.setItem('bjut_flow_alert_threshold', String(config.flow_alert_threshold ?? 5));
    const androidNotificationMode = config.android_notification_mode === 'separate' ? 'separate' : 'combined';
    localStorage.setItem('bjut_android_notification_mode', androidNotificationMode);
    localStorage.setItem('bjut_android_notify_network_status', String(config.android_notify_network_status !== false));
    localStorage.setItem('bjut_android_notify_login_results', String(config.android_notify_login_results !== false));
    localStorage.setItem('bjut_android_notify_background_errors', String(config.android_notify_background_errors !== false));
    
    autoLoginEnabled = config.auto_login;
    checkInterval = config.check_interval;
    wifiChangeDetectEnabled = config.wifi_change_detect;
    
    settingAutoLogin.checked = autoLoginEnabled;
    settingWifiChangeDetect.checked = wifiChangeDetectEnabled;
    settingCheckInterval.value = checkInterval.toString();
    settingLogLevel.value = config.log_level;
    settingVpnCompatibility.value = localStorage.getItem('bjut_vpn_compatibility') || 'high';
    scheduleVpnMaximumRollback();
    settingUsageAlerts.checked = config.usage_alerts !== false;
    settingAndroidNotifyUsageAlerts.checked = config.usage_alerts !== false;
    settingBalanceThreshold.value = String(config.balance_alert_threshold ?? 10);
    settingFlowThreshold.value = String(config.flow_alert_threshold ?? 5);
    settingAndroidNotificationMode.value = androidNotificationMode;
    settingAndroidNotifyNetworkStatus.checked = config.android_notify_network_status !== false;
    settingAndroidNotifyLoginResults.checked = config.android_notify_login_results !== false;
    settingAndroidNotifyBackgroundErrors.checked = config.android_notify_background_errors !== false;
    renderAndroidNotificationModeDescription();
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
              await credentialSnapshotFingerprint(legacyAccounts),
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
  if (!window.__TAURI__) return;
  try {
    listen<CountdownPayload>('countdown-tick', event => {
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

    listen<NetworkStatePayload>('network-state-change', event => {
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
        scheduleCurrentCampusAccountDiscovery();
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

    listen<AppLogEntry>('log-event', event => {
      const data = event.payload;
      renderLogEntry(data.module, data.message, data.type, data.time);
    });

    listen<UpdateProgress>('update-progress', event => {
      updateUpdateProgress(event.payload);
    });

    listen<{ message: string; percent?: number }>('billing-center-progress', event => {
      if (!billingCenterLoading) return;
      const message = event.payload?.message?.trim();
      if (!message) return;
      updateBillingRefreshProgress(event.payload.percent ?? 0, true);
      billingCenterMessage.textContent = message;
      syncBillingCenterMessageVisibility();
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
    const initialLogs = await invoke<AppLogEntry[]>('get_logs');
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
      const currentState = await invoke<NetworkStatePayload>('get_current_network_state');
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
          scheduleCurrentCampusAccountDiscovery();
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
    const cStatus = await invoke<CountdownPayload>('get_countdown_status');
    const countdownText = document.getElementById('countdown-text');
    if (countdownText) {
      if (cStatus.status === 'checking') countdownText.textContent = '检测中...';
      else if (cStatus.status === 'suspended') countdownText.textContent = '已休眠';
      else countdownText.textContent = cStatus.seconds.toString();
    }

    // Report visibility background status
    const updateBgState = () => {
      const isBg = document.hidden || (!IS_ANDROID && !document.hasFocus());
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

const ANDROID_LOG_RENDER_BATCH = 360;
let renderedLogLimit = IS_ANDROID ? ANDROID_LOG_RENDER_BATCH : Number.MAX_SAFE_INTEGER;
let logRenderFrame: number | null = null;
let logFilterSignature = '';
let logHistoryLoadPending = false;

function logsAreAtBottom(scroller: HTMLElement | null): boolean {
  return !scroller || scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop <= 80;
}

function scheduleFilteredLogsRender() {
  if (logRenderFrame !== null) return;
  logRenderFrame = requestAnimationFrame(() => {
    logRenderFrame = null;
    renderFilteredLogs();
  });
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
    const scroller = getLogsScroller();
    if (logsAreAtBottom(scroller)) scheduleFilteredLogsRender();
  } else {
    logFilterCount.textContent = `${logEntriesCache.length} 条`;
  }
}

function renderFilteredLogs(options: { resetWindow?: boolean; preserveScrollAnchor?: boolean } = {}) {
  const logsPageActive = logsContent.closest('.page')?.classList.contains('active') ?? false;
  const scroller = getLogsScroller();
  const wasAtBottom = logsAreAtBottom(scroller);
  const previousScrollHeight = scroller?.scrollHeight ?? 0;
  const previousScrollTop = scroller?.scrollTop ?? 0;
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
  const nextFilterSignature = `${logSessionFilter?.value || 'all'}\u0000${level}\u0000${query}`;
  if (options.resetWindow || nextFilterSignature !== logFilterSignature) {
    renderedLogLimit = IS_ANDROID ? ANDROID_LOG_RENDER_BATCH : Number.MAX_SAFE_INTEGER;
    logFilterSignature = nextFilterSignature;
  }
  const renderStart = IS_ANDROID ? Math.max(0, visible.length - renderedLogLimit) : 0;
  const rendered = visible.slice(renderStart);
  const fragment = document.createDocumentFragment();
  if (renderStart > 0) {
    const sentinel = document.createElement('div');
    sentinel.className = 'log-history-sentinel';
    sentinel.dataset.remaining = String(renderStart);
    sentinel.textContent = `继续上滑加载更早的 ${renderStart} 条日志`;
    fragment.appendChild(sentinel);
  }
  rendered.forEach(item => {
    const entry = document.createElement('div');
    entry.className = item.message.includes('=== SESSION START ===') ? 'log-entry log-session-divider' : 'log-entry';
    const timeElement = document.createElement('span');
    timeElement.className = 'log-time';
    timeElement.textContent = `[${item.time}]`;
    const messageElement = document.createElement('span');
    messageElement.className = `log-${item.type}`;
    messageElement.textContent = `[${item.module}] ${item.message.replace('=== SESSION START ===', '启动会话')}`;
    entry.append(timeElement, messageElement);
    fragment.appendChild(entry);
  });
  logsContent.replaceChildren(fragment);
  if (logFilterCount) {
    logFilterCount.textContent = renderStart > 0
      ? `${rendered.length} / ${visible.length} 条`
      : `${visible.length} / ${logEntriesCache.length} 条`;
  }
  logsDirty = false;
  requestAnimationFrame(() => {
    if (!logsPageActive || !scroller) return;
    if (options.preserveScrollAnchor) {
      scroller.scrollTop = scroller.scrollHeight - previousScrollHeight + previousScrollTop;
    } else if (wasAtBottom) {
      scroller.scrollTop = scroller.scrollHeight;
    }
  });
}

function loadOlderLogsNearTop() {
  if (!IS_ANDROID || logHistoryLoadPending) return;
  const scroller = getLogsScroller();
  const sentinel = logsContent.querySelector<HTMLElement>('.log-history-sentinel');
  if (!scroller || !sentinel || scroller.scrollTop > 56) return;
  const remaining = Number(sentinel.dataset.remaining || 0);
  if (remaining <= 0) return;
  logHistoryLoadPending = true;
  requestAnimationFrame(() => {
    renderedLogLimit += Math.min(ANDROID_LOG_RENDER_BATCH, remaining);
    renderFilteredLogs({ preserveScrollAnchor: true });
    logHistoryLoadPending = false;
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
      input.value = '';
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
  renderIcons(content);
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

// UI Elements
const navItems = document.querySelectorAll('.nav-item');
const navPageLinks = Array.from(document.querySelectorAll<HTMLButtonElement>('[data-nav-page-target]'));
const pages = document.querySelectorAll('.page');
const networkStatus = document.getElementById('network-status')!;
const networkDetail = document.getElementById('network-detail')!;
const networkIcon = document.getElementById('network-icon')!;
const btnLogin = document.getElementById('btn-login') as HTMLButtonElement;
const infoAccount = document.getElementById('info-account')!;
const infoAccountLabel = document.getElementById('info-account-label')!;
const infoBalance = document.getElementById('info-balance')!;
const infoFlow = document.getElementById('info-flow')!;
const btnOpenBilling = document.getElementById('btn-open-billing') as HTMLButtonElement;
const btnCloseBilling = document.getElementById('btn-close-billing') as HTMLButtonElement;
const btnRefreshBillingCenter = document.getElementById('btn-refresh-billing-center') as HTMLButtonElement;
const billingRefreshLabel = document.getElementById('billing-refresh-label')!;
const btnToggleBillingMauth = document.getElementById('btn-toggle-billing-mauth') as HTMLButtonElement;
const billingCenterMessage = document.getElementById('billing-center-message')!;
const billingCenterSubtitle = document.getElementById('billing-center-subtitle')!;
const billingCenterAccount = document.getElementById('billing-center-account')!;
const billingCenterBalance = document.getElementById('billing-center-balance')!;
const billingCenterFlow = document.getElementById('billing-center-flow')!;
const billingCenterStatus = document.getElementById('billing-center-status')!;
const billingMauthBadge = document.getElementById('billing-mauth-badge')!;
const billingOnlineCount = document.getElementById('billing-online-count')!;
const billingOnlineList = document.getElementById('billing-online-list')!;
const billingHistoryCount = document.getElementById('billing-history-count')!;
const billingHistoryList = document.getElementById('billing-history-list')!;
const billingHistoryPanel = document.getElementById('billing-history-panel') as HTMLDetailsElement;
const billingRecordTotal = document.getElementById('billing-record-total')!;
const billingRecordRange = document.getElementById('billing-record-range')!;
const billingRecordSummary = document.getElementById('billing-record-summary')!;
const billingRecordsList = document.getElementById('billing-records-list')!;
const btnExportBillingRecords = document.getElementById('btn-export-billing-records') as HTMLButtonElement;
const btnExportAllBillingRecords = document.getElementById('btn-export-all-billing-records') as HTMLButtonElement;
const billingRecordDateFilter = document.getElementById('billing-record-date-filter')!;
const billingRecordYearFilter = document.getElementById('billing-record-year-filter')!;
const billingRecordStartDate = document.getElementById('billing-record-start-date') as HTMLInputElement;
const billingRecordEndDate = document.getElementById('billing-record-end-date') as HTMLInputElement;
const btnQueryBillingRecords = document.getElementById('btn-query-billing-records') as HTMLButtonElement;
const btnBillingRecordPrev = document.getElementById('btn-billing-record-prev') as HTMLButtonElement;
const btnBillingRecordNext = document.getElementById('btn-billing-record-next') as HTMLButtonElement;
const billingRecordPageLabel = document.getElementById('billing-record-page-label')!;
const billingServiceStatusBadge = document.getElementById('billing-service-status-badge')!;
const billingServiceReason = document.getElementById('billing-service-reason')!;
const billingServicePackage = document.getElementById('billing-service-package')!;
const billingServiceSettlement = document.getElementById('billing-service-settlement')!;
const billingServiceSpend = document.getElementById('billing-service-spend')!;
const billingServiceLimit = document.getElementById('billing-service-limit')!;
const btnBillingStopNow = document.getElementById('btn-billing-stop-now') as HTMLButtonElement;
const btnBillingReopenNow = document.getElementById('btn-billing-reopen-now') as HTMLButtonElement;
const billingPackageOptions = document.getElementById('billing-package-options')!;
const btnBillingPackage = document.getElementById('btn-billing-package') as HTMLButtonElement;
const btnBillingCancelPackage = document.getElementById('btn-billing-cancel-package') as HTMLButtonElement;
const billingConsumeLimit = document.getElementById('billing-consume-limit') as HTMLInputElement;
const btnBillingConsumeLimit = document.getElementById('btn-billing-consume-limit') as HTMLButtonElement;
const billingDeviceCount = document.getElementById('billing-device-count')!;
const billingDeviceList = document.getElementById('billing-device-list')!;
const billingRechargeState = document.getElementById('billing-recharge-state')!;
const billingRechargeMethodTitle = document.getElementById('billing-recharge-method-title')!;
const billingRechargeMethodDescription = document.getElementById('billing-recharge-method-description')!;
const billingRechargeForm = document.getElementById('billing-recharge-form') as HTMLFormElement;
const billingRechargeAccount = document.getElementById('billing-recharge-account') as HTMLInputElement;
const billingRechargeCustomTarget = document.getElementById('billing-recharge-custom-target')!;
const billingRechargeAmount = document.getElementById('billing-recharge-amount') as HTMLInputElement;
const btnBillingRecharge = document.getElementById('btn-billing-recharge') as HTMLButtonElement;
const billingRechargeProgress = document.getElementById('billing-recharge-progress')!;
const billingRechargeProgressText = document.getElementById('billing-recharge-progress-text')!;
const billingRechargeProgressPercent = document.getElementById('billing-recharge-progress-percent')!;
const billingRechargeProgressBar = document.getElementById('billing-recharge-progress-bar')!;
const billingRechargePreview = document.getElementById('billing-recharge-preview')!;
const billingRechargePayer = document.getElementById('billing-recharge-payer')!;
const billingRechargeCardBalance = document.getElementById('billing-recharge-card-balance')!;
const billingRechargeTargetStatus = document.getElementById('billing-recharge-target-status')!;
const billingRechargeTargetBalance = document.getElementById('billing-recharge-target-balance')!;
type RechargeMethod = 'campus-card' | 'alipay' | 'wechat';
const billingRechargeMethodButtons = Array.from(
  document.querySelectorAll<HTMLButtonElement>('[data-recharge-method-target]'),
);
const btnBillingAlipayShowPayment = document.getElementById('btn-billing-alipay-show-payment') as HTMLButtonElement;
const btnBillingWechatContinue = document.getElementById('btn-billing-wechat-continue') as HTMLButtonElement;
const alipayPaymentModal = document.getElementById('alipay-payment-modal')!;
const alipayPaymentQr = document.getElementById('alipay-payment-qr') as HTMLCanvasElement;
const alipayPaymentQrShell = document.getElementById('alipay-payment-qr-shell')!;
const alipayPaymentQrFallback = document.getElementById('alipay-payment-qr-fallback')!;
const alipayPaymentPayer = document.getElementById('alipay-payment-payer')!;
const alipayPaymentAmount = document.getElementById('alipay-payment-amount')!;
const alipayPaymentTarget = document.getElementById('alipay-payment-target')!;
const alipayPaymentModalStatus = document.getElementById('alipay-payment-modal-status')!;
const btnAlipayPaymentClose = document.getElementById('btn-alipay-payment-close') as HTMLButtonElement;
const btnAlipayPaymentCloseIcon = document.getElementById('btn-alipay-payment-close-icon') as HTMLButtonElement;
const btnAlipayPaymentCopy = document.getElementById('btn-alipay-payment-copy') as HTMLButtonElement;
const btnAlipayPaymentOpen = document.getElementById('btn-alipay-payment-open') as HTMLButtonElement;
const btnAlipayPaymentComplete = document.getElementById('btn-alipay-payment-complete') as HTMLButtonElement;
const billingBindMac = document.getElementById('billing-bind-mac') as HTMLInputElement;
const btnBillingBindMac = document.getElementById('btn-billing-bind-mac') as HTMLButtonElement;
const billingPasswordForm = document.getElementById('billing-password-form') as HTMLFormElement;
const billingPasswordPolicy = document.getElementById('billing-password-policy')!;
const billingOldPassword = document.getElementById('billing-old-password') as HTMLInputElement;
const billingNewPassword = document.getElementById('billing-new-password') as HTMLInputElement;
const billingConfirmPassword = document.getElementById('billing-confirm-password') as HTMLInputElement;
const btnBillingPassword = document.getElementById('btn-billing-password') as HTMLButtonElement;
const billingQuestionsForm = document.getElementById('billing-questions-form') as HTMLFormElement;
const billingQuestionPassword = document.getElementById('billing-question-password') as HTMLInputElement;
const btnBillingQuestions = document.getElementById('btn-billing-questions') as HTMLButtonElement;
type BillingWorkbenchSection = 'overview' | 'records' | 'services' | 'recharge' | 'devices';
const billingWorkbenchSections: BillingWorkbenchSection[] = ['overview', 'records', 'services', 'recharge', 'devices'];
const billingWorkbenchSectionSubtitles: Record<BillingWorkbenchSection, string> = {
  overview: '账户状态、在线会话与近期上网记录',
  records: '用量、账单与各类业务办理记录',
  services: '停复机、消费保护与套餐预约',
  recharge: '校园卡转入、支付宝与微信充值',
  devices: '无感认证设备与统一认证安全设置',
};
const billingSectionNavButtons = Array.from(
  document.querySelectorAll<HTMLButtonElement>('[data-billing-section-target]'),
);
const billingSectionShortcuts = Array.from(
  document.querySelectorAll<HTMLButtonElement>('[data-billing-section-shortcut]'),
);
const billingSectionPanels = Array.from(
  document.querySelectorAll<HTMLElement>('[data-billing-section]'),
);
const accountsList = document.getElementById('accounts-list')!;
const addAccountForm = document.getElementById('add-account-form') as HTMLFormElement;
const logsContent = document.getElementById('logs-content')!;
const btnClearLogs = document.getElementById('btn-clear-logs')!;
const btnExportLogs = document.getElementById('btn-export-logs')!;
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
const settingAndroidNotifyNetworkStatus = document.getElementById('setting-android-notify-network-status') as HTMLInputElement;
const settingAndroidNotifyLoginResults = document.getElementById('setting-android-notify-login-results') as HTMLInputElement;
const settingAndroidNotifyUsageAlerts = document.getElementById('setting-android-notify-usage-alerts') as HTMLInputElement;
const settingAndroidNotifyBackgroundErrors = document.getElementById('setting-android-notify-background-errors') as HTMLInputElement;
const androidNotificationModeDescription = document.getElementById('android-notification-mode-description')!;
const permissionHealthGroup = document.getElementById('permission-health-group')!;
const permissionHealthList = document.getElementById('permission-health-list')!;
const permissionHealthSummary = document.getElementById('permission-health-summary')!;
const btnTogglePermissions = document.getElementById('btn-toggle-permissions') as HTMLButtonElement;
const permissionHealthToggleLabel = document.getElementById('permission-health-toggle-label')!;
const btnRefreshPermissions = document.getElementById('btn-refresh-permissions') as HTMLButtonElement;
const settingAutoLogin = document.getElementById('setting-auto-login') as HTMLInputElement;
const settingWifiChangeDetect = document.getElementById('setting-wifi-change-detect') as HTMLInputElement;
const settingAutostart = document.getElementById('setting-autostart') as HTMLInputElement;
const settingCheckInterval = document.getElementById('setting-check-interval') as HTMLInputElement;
let settingLogLevel: CustomSelect;
let overrideAccountSelect: CustomSelect;
let overrideMethodSelect: CustomSelect;
let settingUpdateChannel: CustomSelect;
let settingVpnCompatibility: CustomSelect;
let settingAndroidNotificationMode: CustomSelect;
let logSessionFilter: CustomSelect;
let logLevelFilter: CustomSelect;
let networkProfileProtocolSelect: CustomSelect;
let networkProfileAccountSelect: CustomSelect;
let billingAccountSelect: CustomSelect;
let billingRechargeCardAccountSelect: CustomSelect;
let billingRechargeTargetAccountSelect: CustomSelect;
let billingRecordKindSelect: CustomSelect;
let billingRecordYearSelect: CustomSelect;
let billingRecordPageSizeSelect: CustomSelect;
let billingQuestionSelects: CustomSelect[] = [];
let billingCenterData: BillingCenterData | null = null;
let activeRechargeMethod: RechargeMethod = 'campus-card';
let activeAlipayPayment: ActiveAlipayPayment | null = null;
let activeWechatPayment: ActiveWechatPayment | null = null;
let alipayPaymentModalReturnFocus: HTMLElement | null = null;
let alipayCompletionBusy = false;
let alipayExternalHandoffAt = 0;
let alipayAutomaticCheckTimer: number | null = null;
let alipayAutomaticCheckCount = 0;
let wechatCompletionBusy = false;
let wechatExternalHandoffAt = 0;
let wechatLastAutomaticCheckAt = 0;
let wechatAutomaticCheckTimer: number | null = null;
let wechatAutomaticCheckCount = 0;
let billingRecordQueryStates: Partial<Record<BillingRecordKind, BillingRecordQueryState>> = {};
let billingRecordQueryBusy = false;
let billingCenterLoading = false;
let selectedBillingPackageId = '';
let effectiveNextBillingPackageId = '';
let effectiveNextBillingPackageName = '';
let hasDistinctBillingPackageReservation = false;
let activeBillingWorkbenchSection: BillingWorkbenchSection = 'overview';
let campusAccountDiscoveryPromise: Promise<void> | null = null;
let lastCampusAccountDiscoveryAt = 0;
const FIRST_LAUNCH_ACCOUNT_DISCOVERY_KEY = 'bjut_first_launch_account_discovery_pending';
const DISMISSED_DISCOVERED_ACCOUNT_KEY = 'bjut_dismissed_discovered_account';
const UNIFIED_AUTH_PASSWORD_POLICY: BillingPasswordPolicy = {
  minLength: 12,
  maxLength: 16,
  requireUppercase: true,
  requireLowercase: true,
  requireDigit: true,
  requireSpecial: true,
};

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
let userInfoRequestId = 0;
let userInfoLoading = false;

// New state for split check loops
let lastKnownIp = '';
let wifiChangeTimer: number | null = null;
let wifiChangeDetectEnabled = localStorage.getItem('bjut_wifi_change_detect') !== 'false';
let connectivityTimer: number | null = null;
let secondsToNextCheck = 0;
let countdownInterval: number | null = null;
let isLoopSuspended = false;

async function restoreRecoverableRecharges() {
  if (!window.__TAURI__) return;
  const transactions = await invoke<RecoverableRecharge[]>('get_recoverable_recharges').catch(() => []);
  const alipay = transactions.find(item => item.method === 'alipay' && isTrustedAlipayPaymentUrl(item.paymentUrl));
  if (alipay) {
    rememberActiveAlipayPayment({
      recoveryId: alipay.id,
      paymentUrl: alipay.paymentUrl,
      payerAccount: alipay.payerAccount,
      targetAccount: alipay.targetAccount,
      amount: alipay.amount,
      cardBalanceBefore: alipay.cardBalanceBefore,
    });
  }
  const wechat = transactions.find(item => item.method === 'wechat'
    && item.paymentId
    && isTrustedWechatLaunchUrl(item.paymentUrl));
  if (wechat) {
    rememberActiveWechatPayment({
      paymentId: wechat.paymentId,
      launchUrl: wechat.paymentUrl,
      payerAccount: wechat.payerAccount,
      targetAccount: wechat.targetAccount,
      amount: wechat.amount,
      cardBalanceBefore: wechat.cardBalanceBefore,
    });
  }
  const uncertain = transactions.find(item => item.stage === 'unknown' || item.stage === 'transferSubmitted');
  if (uncertain) {
    billingRechargeState.textContent = `检测到一笔结果待核对的充值：${uncertain.payerAccount} → ${uncertain.targetAccount}，${uncertain.amount} 元。请先查询余额和记录，不要创建重复订单。`;
    log('计费', `已恢复待核对充值 ${uncertain.id}：${uncertain.note || uncertain.stage}`, 'error');
  } else if (alipay || wechat) {
    billingRechargeState.textContent = '已从安全存储恢复未完成的支付订单，可继续原订单或核对到账。';
    log('计费', '已恢复未完成的充值订单', 'info');
  }
}

// Initialize
async function init() {
  // Instantiate Custom Selects
  overrideAccountSelect = new CustomSelect('override-account');
  overrideMethodSelect = new CustomSelect('override-method');
  settingUpdateChannel = new CustomSelect('setting-update-channel');
  settingLogLevel = new CustomSelect('setting-log-level');
  settingVpnCompatibility = new CustomSelect('setting-vpn-compatibility');
  settingAndroidNotificationMode = new CustomSelect('setting-android-notification-mode');
  logSessionFilter = new CustomSelect('log-session-filter');
  logLevelFilter = new CustomSelect('log-level-filter');
  networkProfileProtocolSelect = new CustomSelect('network-profile-protocol');
  networkProfileAccountSelect = new CustomSelect('network-profile-account');
  billingAccountSelect = new CustomSelect('billing-account-select');
  billingRechargeCardAccountSelect = new CustomSelect('billing-recharge-card-account');
  billingRechargeTargetAccountSelect = new CustomSelect('billing-recharge-target-account');
  billingRecordKindSelect = new CustomSelect('billing-record-kind');
  billingRecordYearSelect = new CustomSelect('billing-record-year-filter');
  billingRecordPageSizeSelect = new CustomSelect('billing-record-page-size');
  billingQuestionSelects = [1, 2, 3].map(index => new CustomSelect(`billing-question-${index}`));
  const currentYear = new Date().getFullYear();
  billingRecordYearSelect.setOptions(Array.from({ length: 10 }, (_, index) => {
    const year = String(currentYear - index);
    return { value: year, text: `${year} 年` };
  }));
  billingRecordYearSelect.setValue(String(currentYear));
  billingRecordKindSelect.addEventListener('change', () => {
    syncBillingRecordControls();
    renderBillingRecords();
  });
  billingRecordPageSizeSelect.addEventListener('change', () => {
    if (!billingCenterData) return;
    const state = currentBillingRecordQueryState();
    state.pageSize = Number.parseInt(billingRecordPageSizeSelect.value || '10', 10);
    if (state.queried) void queryBillingRecords(1);
    else renderBillingRecords();
  });
  billingAccountSelect.addEventListener('change', () => {
    billingCenterData = null;
    billingRecordQueryStates = {};
    resetBillingCenterForSelectedAccount();
    syncStandaloneBillingSecurity();
    void refreshBillingCenterData();
  });
  billingRechargeCardAccountSelect.addEventListener('change', () => {
    billingRechargePreview.hidden = true;
    billingRechargeState.textContent = `已选择校园卡账户 ${selectedRechargePayerAccount() || '--'}，请继续核对目标账户和金额。`;
  });
  billingRechargeTargetAccountSelect.addEventListener('change', () => {
    syncRechargeTargetAccountInput();
    billingRechargePreview.hidden = true;
  });
  syncStandaloneBillingSecurity();

  renderIcons();
  settingAutoLogin.checked = autoLoginEnabled;
  settingWifiChangeDetect.checked = wifiChangeDetectEnabled;
  settingCheckInterval.value = checkInterval.toString();

  // Handle autostart and quit element visibility and status
  if (!window.__TAURI__ || IS_ANDROID) {
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
  settingVpnCompatibility.value = localStorage.getItem('bjut_vpn_compatibility') || 'high';
  settingUpdateChannel.value = localStorage.getItem('bjut_update_channel') || 'release';
  if (window.__TAURI__) {
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
  window.triggerAutoLogin = () => {
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
      localStorage.setItem(FIRST_LAUNCH_ACCOUNT_DISCOVERY_KEY, 'true');
      tosModal.classList.add('hidden');
      log('系统', '已同意用户协议与隐私政策');
      
      // Request foreground permissions
      if (window.__TAURI__) {
        try {
          if (IS_ANDROID && window.AndroidBridge) {
            window.AndroidBridge.requestForegroundPermissions();
          } else if (IS_ANDROID) {
            await invoke('request_foreground_permissions');
          }
          if (IS_ANDROID) log('系统', '已申请前台网络定位相关权限');
        } catch (e) {
          console.error('Failed to request foreground permissions:', e);
        }
        // Load the secure backend configuration before the first sync so accepting the
        // terms cannot overwrite an existing account set with the empty WebView cache.
        await loadConfigFromRust();
        await restoreRecoverableRecharges();
        await listenToRustEvents();
        if (window.AndroidBridge && autoLoginEnabled) {
          window.AndroidBridge.startKeepAliveService();
        }
        log('系统', '应用启动');
      } else {
        startWifiChangeCheckLoop();
        startConnectivityCheckLoop();
        log('系统', '应用启动');
      }
    });
    
    document.getElementById('btn-tos-disagree')!.addEventListener('click', async () => {
      if (window.__TAURI__) {
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
    if (window.__TAURI__) {
      if (IS_ANDROID && !window.AndroidBridge) {
        invoke('request_foreground_permissions').catch(e => {
          console.error('Failed to request foreground permissions:', e);
        });
      }
      try {
        await loadConfigFromRust();
        await restoreRecoverableRecharges();
        await listenToRustEvents();
        log('系统', '应用启动');
        if (window.AndroidBridge) {
          if (autoLoginEnabled) {
            window.AndroidBridge.startKeepAliveService();
            log('系统', '后台保活服务已启动');
          } else {
            window.AndroidBridge.stopKeepAliveService();
          }
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
  updateAccountHealthBadges();
  renderAccountHealthPanel();
}

function updateAccountHealthBadges() {
  accountsList.querySelectorAll<HTMLElement>('.account-item').forEach(item => {
    const user = item.querySelector('.account-user h4')?.textContent || '';
    const health = accountHealthCache.get(user);
    const badge = item.querySelector<HTMLElement>('.account-health-badge');
    const detail = item.querySelector<HTMLElement>('.account-health-inline small');
    if (!badge || !detail) return;
    if (!health) {
      badge.className = 'account-health-badge healthy';
      badge.textContent = '正常';
      detail.textContent = '暂无失败记录';
      detail.title = '';
      return;
    }
    badge.className = `account-health-badge ${health.status}`;
    badge.textContent = accountHealthLabel(health.status);
    detail.textContent = health.consecutiveFailures > 0
      ? `${formatCooldown(health.cooldownSeconds)} · 失败 ${health.consecutiveFailures} 次`
      : '暂无失败记录';
    detail.title = health.lastFailureReason || '';
  });
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
  if (!window.__TAURI__) return;
  try {
    setAccountHealth(await invoke<AccountHealth[]>('get_account_health'));
  } catch (error) {
    console.error('Failed to load account health:', error);
  }
}

async function refreshCredentialStorageHealth() {
  if (!window.__TAURI__) return;
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
  if (!window.__TAURI__) {
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
      (typePolicy.type1 ?? legacyAutoLogin) ? '宿舍' : '',
      (typePolicy.type2 ?? legacyAutoLogin) ? '教学' : '',
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
    if (!window.__TAURI__) return;
    if (networkEventDebounce !== null) window.clearTimeout(networkEventDebounce);
    networkEventDebounce = window.setTimeout(() => {
      networkEventDebounce = null;
      void invoke('notify_network_change', { source });
    }, 500);
  };
  window.addEventListener('online', () => notify('浏览器 online'));
  window.addEventListener('offline', () => notify('浏览器 offline'));
  const connection = navigator.connection;
  connection?.addEventListener?.('change', () => notify('系统连接属性'));
  window.__nativeNetworkChanged = (source = 'Android NetworkCallback') => notify(source);
  window.__nativeNotificationAction = async (action: 'check' | 'pause' | 'resume') => {
    if (!window.__TAURI__) return;
    if (action === 'check') {
      await invoke('notify_network_change', { source: 'Android 常驻通知' });
    } else {
      await invoke('set_auto_login_pause', { minutes: action === 'pause' ? 60 : 0 });
    }
  };
  const pausedUntil = Number(window.AndroidBridge?.getAutoLoginPausedUntil?.() || 0);
  if (pausedUntil > Date.now()) {
    const remainingMinutes = Math.max(1, Math.ceil((pausedUntil - Date.now()) / 60000));
    void invoke('set_auto_login_pause', { minutes: remainingMinutes });
  }
}

async function readPermissionHealth(): Promise<PermissionHealthItem[]> {
  const androidBridge = window.AndroidBridge;
  if (androidBridge?.getPermissionHealth) {
    const raw = androidBridge.getPermissionHealth();
    const parsed = JSON.parse(raw || '[]');
    return Array.isArray(parsed) ? parsed : [];
  }
  const items: PermissionHealthItem[] = [];
  if (IS_WINDOWS) {
    items.push({
      id: 'notifications',
      label: '系统通知',
      granted: true,
      required: false,
      detail: 'Windows 桌面通知无需运行时授权；可在系统“通知”设置中统一管理',
    });
  } else {
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

function setPermissionHealthCollapsed(collapsed: boolean) {
  permissionHealthGroup.classList.toggle('is-collapsed', collapsed);
  btnTogglePermissions.setAttribute('aria-expanded', String(!collapsed));
  permissionHealthList.setAttribute('aria-hidden', String(collapsed));
  permissionHealthToggleLabel.textContent = collapsed ? '展开详情' : '收起详情';
}

async function refreshPermissionHealth() {
  btnRefreshPermissions.disabled = true;
  permissionHealthSummary.textContent = '正在检查系统权限…';
  try {
    const items = await readPermissionHealth();
    permissionHealthList.innerHTML = '';
    const missingRequired = items.filter(item => item.required && !item.granted).length;
    const missingOptional = items.filter(item => !item.required && !item.granted).length;
    const allGranted = items.length > 0 && missingRequired === 0 && missingOptional === 0;
    permissionHealthSummary.className = `permission-health-summary ${missingRequired ? 'error' : missingOptional || !items.length ? 'warning' : 'success'}`;
    permissionHealthSummary.textContent = !items.length
      ? '未获取到权限状态，请重新检查'
      : missingRequired
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
    setPermissionHealthCollapsed(allGranted);
  } catch (error) {
    permissionHealthSummary.className = 'permission-health-summary error';
    permissionHealthSummary.textContent = `权限检查失败：${String(error)}`;
    setPermissionHealthCollapsed(false);
  } finally {
    btnRefreshPermissions.disabled = false;
  }
}

// Navigation
let billingSectionAnimationTimer: number | null = null;

function syncBillingCenterMessageVisibility() {
  billingCenterMessage.hidden = activeBillingWorkbenchSection === 'recharge'
    || !billingCenterMessage.textContent?.trim();
}

function activateBillingWorkbenchSection(section: string, resetScroll = false) {
  if (!billingWorkbenchSections.includes(section as BillingWorkbenchSection)) return;
  const changed = activeBillingWorkbenchSection !== section;
  activeBillingWorkbenchSection = section as BillingWorkbenchSection;
  billingCenterSubtitle.textContent = billingWorkbenchSectionSubtitles[activeBillingWorkbenchSection];
  billingSectionPanels.forEach(panel => {
    const active = panel.dataset.billingSection === activeBillingWorkbenchSection;
    panel.hidden = !active;
    panel.classList.remove('billing-section-enter');
    if (active && changed) {
      void panel.offsetWidth;
      panel.classList.add('billing-section-enter');
    }
  });
  billingSectionNavButtons.forEach(button => {
    const active = button.dataset.billingSectionTarget === activeBillingWorkbenchSection;
    button.classList.toggle('active', active);
    button.setAttribute('aria-selected', String(active));
  });
  syncBillingCenterMessageVisibility();
  if (billingSectionAnimationTimer !== null) window.clearTimeout(billingSectionAnimationTimer);
  if (changed) {
    billingSectionAnimationTimer = window.setTimeout(() => {
      billingSectionPanels.forEach(panel => panel.classList.remove('billing-section-enter'));
      billingSectionAnimationTimer = null;
    }, 260);
  }
  if (resetScroll) {
    document.querySelector<HTMLElement>('main')?.scrollTo({ top: 0, behavior: 'auto' });
  }
}

function activatePage(target: string, navTarget = target) {
  const consoleExpanded = target === 'dashboard' || target === 'billing-center';
  const billingExpanded = target === 'billing-center';
  document.body.classList.toggle('billing-center-active', IS_ANDROID && billingExpanded);
  navItems.forEach(item => {
    const itemTarget = item.getAttribute('data-target');
    const active = itemTarget === navTarget
      || (itemTarget === 'dashboard' && target === 'billing-center');
    item.classList.toggle('active', active);
  });
  navPageLinks.forEach(link => {
    link.classList.toggle('active', link.dataset.navPageTarget === target);
  });
  const nav = document.getElementById('nav');
  nav?.classList.toggle('dashboard-expanded', consoleExpanded);
  nav?.classList.toggle('billing-active', billingExpanded);
  nav?.querySelector<HTMLElement>('.nav-dashboard-parent')
    ?.setAttribute('aria-expanded', String(consoleExpanded));
  nav?.querySelector<HTMLElement>('[data-nav-page-target="billing-center"]')
    ?.setAttribute('aria-expanded', String(billingExpanded));
  nav?.querySelector<HTMLElement>('.nav-dashboard-children')
    ?.setAttribute('aria-hidden', String(!consoleExpanded));
  nav?.querySelector<HTMLElement>('.nav-billing-tree')
    ?.setAttribute('aria-hidden', String(!billingExpanded));
  pages.forEach(page => page.classList.toggle('active', page.id === target));
  if (target === 'diagnostics') {
    void Promise.all([refreshAccountHealth(), refreshCredentialStorageHealth()]);
  }
  if (target === 'logs' && logsDirty) renderFilteredLogs();
  if (target === 'settings') void refreshPermissionHealth();
  if (target === 'billing-center') {
    activateBillingWorkbenchSection(activeBillingWorkbenchSection);
    renderBillingCenter(billingCenterData ? billingOverviewToUserInfo(billingCenterData.overview) : null);
  }
}

function handleAndroidBack() {
  const visibleModal = document.querySelector<HTMLElement>('.modal-overlay:not(.hidden)');
  if (visibleModal) {
    const dismiss = visibleModal.querySelector<HTMLElement>([
      '#btn-confirm-cancel',
      '#btn-alert-ok',
      '#btn-alipay-payment-close-icon',
      '#btn-password-prompt-cancel',
      '#btn-cancel-add',
      '#btn-cancel-edit',
      '#btn-cancel-delete',
      '#btn-sec-cancel',
      '#btn-update-later',
    ].join(','));
    if (dismiss) {
      dismiss.click();
      return true;
    }
  }
  const activePage = document.querySelector<HTMLElement>('.page.active')?.id;
  if (activePage && activePage !== 'dashboard') {
    activatePage('dashboard');
    document.querySelector<HTMLElement>('main')?.scrollTo({ top: 0, behavior: 'auto' });
    return true;
  }
  return false;
}

if (IS_ANDROID) {
  window.__handleAndroidBack = handleAndroidBack;
}

function setupNavigation() {
  navItems.forEach(item => {
    item.addEventListener('click', () => {
      const target = item.getAttribute('data-target');
      if (target) activatePage(target);
    });
  });
  navPageLinks.forEach(link => {
    link.addEventListener('click', () => {
      const target = link.dataset.navPageTarget;
      if (!target) return;
      if (target === 'billing-center') {
        activateBillingWorkbenchSection('overview', true);
        activatePage(target, 'dashboard');
        if (!billingCenterData) void refreshBillingCenterData();
      } else {
        activatePage(target);
      }
    });
  });
}

// Event Listeners
function setupEventListeners() {
  btnLogin.addEventListener('click', manualLogin);
  btnRefreshBillingCenter.addEventListener('click', () => void refreshBillingCenterData());
  btnOpenBilling.addEventListener('click', () => {
    activatePage('billing-center', 'dashboard');
    document.querySelector<HTMLElement>('main')?.scrollTo({ top: 0, behavior: 'auto' });
    if (!billingCenterData) void refreshBillingCenterData();
  });
  btnCloseBilling.addEventListener('click', () => activatePage('dashboard'));
  billingSectionNavButtons.forEach(button => {
    button.addEventListener('click', () => {
      if (button.closest('.nav-billing-tree')) {
        activatePage('billing-center', 'dashboard');
        if (!billingCenterData) void refreshBillingCenterData();
      }
      activateBillingWorkbenchSection(button.dataset.billingSectionTarget || '', true);
    });
    button.addEventListener('keydown', event => {
      if (!['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return;
      event.preventDefault();
      const group = button.closest('.nav-billing-tree, .billing-mobile-nav');
      const groupButtons = Array.from(
        group?.querySelectorAll<HTMLButtonElement>('[data-billing-section-target]') || [],
      );
      const index = groupButtons.indexOf(button);
      if (index < 0 || groupButtons.length === 0) return;
      const backwards = event.key === 'ArrowLeft' || event.key === 'ArrowUp';
      const nextIndex = (index + (backwards ? -1 : 1) + groupButtons.length) % groupButtons.length;
      const nextButton = groupButtons[nextIndex];
      nextButton.focus();
      activateBillingWorkbenchSection(nextButton.dataset.billingSectionTarget || '', true);
    });
  });
  billingSectionShortcuts.forEach(button => {
    button.addEventListener('click', () => {
      const section = button.dataset.billingSectionShortcut || '';
      activateBillingWorkbenchSection(section, true);
      billingSectionNavButtons.find(item => (
        item.dataset.billingSectionTarget === section && item.offsetParent !== null
      ))?.focus();
    });
  });
  btnToggleBillingMauth.addEventListener('click', () => void toggleBillingMauth());
  btnExportBillingRecords.addEventListener('click', () => void exportBillingRecords(false));
  btnExportAllBillingRecords.addEventListener('click', () => void exportBillingRecords(true));
  btnQueryBillingRecords.addEventListener('click', () => void queryBillingRecords(1));
  btnBillingRecordPrev.addEventListener('click', () => {
    const state = currentBillingRecordQueryState();
    if (state.page > 1) void queryBillingRecords(state.page - 1);
  });
  btnBillingRecordNext.addEventListener('click', () => {
    const state = currentBillingRecordQueryState();
    void queryBillingRecords(state.page + 1);
  });
  billingPackageOptions.addEventListener('click', event => {
    const option = (event.target as HTMLElement).closest<HTMLButtonElement>('.billing-package-option[data-package-id]');
    if (!option) return;
    selectedBillingPackageId = option.dataset.packageId || '';
    billingPackageOptions.querySelectorAll('.billing-package-option').forEach(element => {
      element.classList.toggle('selected', element === option);
      element.setAttribute('aria-pressed', String(element === option));
    });
    updateBillingPackageActionButton();
  });
  btnBillingStopNow.addEventListener('click', () => void performConfirmedBillingAction(
    { action: 'stopNow' },
    '立即停机',
    '停机后该账号将无法继续使用校园网，并停止计费。确定立即停机吗？',
    btnBillingStopNow,
  ));
  btnBillingReopenNow.addEventListener('click', () => void performConfirmedBillingAction(
    { action: 'reopenNow' },
    '立即复通',
    '复通成功后账号将恢复校园网使用并继续计费；余额不足时可能失败。确定继续吗？',
    btnBillingReopenNow,
  ));
  btnBillingPackage.addEventListener('click', () => {
    const option = billingCenterData?.service.packageOptions.find(item => item.id === selectedBillingPackageId);
    if (!option) {
      void customAlert('请先选择一个可预约套餐。');
      return;
    }
    const selectsCurrentPackage = billingPackageOptionIsCurrent(option, billingCenterData!.service);
    const restoresCurrentPackage = hasDistinctBillingPackageReservation && selectsCurrentPackage;
    void performConfirmedBillingAction(
      restoresCurrentPackage
        ? { action: 'cancelPackage' }
        : { action: 'schedulePackage', packageId: option.id },
      restoresCurrentPackage ? '下一周期沿用当前套餐' : '预约套餐',
      restoresCurrentPackage
        ? `确定取消“${billingCenterData?.service.scheduledPackage || '已预约套餐'}”，让下一周期继续使用“${option.name}”吗？`
        : `确定将下一周期套餐调整为“${option.name}”吗？新套餐通常在下一计费周期生效。`,
      btnBillingPackage,
    );
  });
  btnBillingCancelPackage.addEventListener('click', () => void performConfirmedBillingAction(
    { action: 'cancelPackage' }, '取消套餐预约', '确定取消当前套餐预约吗？', btnBillingCancelPackage,
  ));
  btnBillingConsumeLimit.addEventListener('click', () => {
    const value = billingConsumeLimit.value.trim();
    if (!/^\d+(?:\.\d{1,3})?$/.test(value) || Number(value) > 999999) {
      void customAlert('消费限额必须是 0–999999 的非负数字，最多三位小数。');
      return;
    }
    void performConfirmedBillingAction(
      { action: 'setConsumeLimit', consumeLimit: value },
      '修改消费保护',
      value === '999999' ? '确定取消消费限额吗？' : `确定将本周期消费限额调整为 ${value} 元吗？`,
      btnBillingConsumeLimit,
    );
  });
  btnBillingBindMac.addEventListener('click', () => {
    const mac = normalizeBillingMac(billingBindMac.value);
    if (!/^[0-9A-F]{12}$/.test(mac)) {
      void customAlert('请输入由 12 位十六进制字符组成的 MAC 地址。');
      return;
    }
    void performConfirmedBillingAction(
      { action: 'bindMac', mac }, '绑定设备', `确定将 MAC ${mac.match(/.{2}/g)?.join('-')} 绑定到当前账号吗？`, btnBillingBindMac,
    );
  });
  billingDeviceList.addEventListener('click', event => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('.action-billing-unbind-mac');
    if (!button?.dataset.mac) return;
    void performConfirmedBillingAction(
      { action: 'unbindMac', mac: button.dataset.mac },
      '解除设备绑定',
      `解除 ${button.dataset.mac} 后，无感认证可能不再识别该设备。确定继续吗？`,
      button,
    );
  });
  billingPasswordForm.addEventListener('submit', event => {
    event.preventDefault();
    const oldPassword = billingOldPassword.value;
    const newPassword = billingNewPassword.value;
    if (!oldPassword || !newPassword || newPassword !== billingConfirmPassword.value) {
      void customAlert('请填写当前密码，并确保两次输入的新密码一致。');
      return;
    }
    void performConfirmedBillingAction(
      { action: 'changePassword', oldPassword, newPassword },
      '修改统一认证密码',
      '修改成功后旧密码会立即失效，校园网登录和统一认证均会使用新密码；App 会同步更新安全存储。确定继续吗？',
      btnBillingPassword,
    );
  });
  billingRechargeForm.addEventListener('submit', event => {
    event.preventDefault();
    if (activeRechargeMethod === 'alipay') void prepareAndOpenAlipayRecharge();
    else if (activeRechargeMethod === 'wechat') void prepareAndOpenWechatRecharge();
    else void prepareAndConfirmNetworkRecharge();
  });
  billingRechargeMethodButtons.forEach(button => {
    button.addEventListener('click', () => {
      activateRechargeMethod((button.dataset.rechargeMethodTarget || '') as RechargeMethod);
    });
  });
  btnBillingAlipayShowPayment.addEventListener('click', () => {
    void showAlipayPaymentModal(btnBillingAlipayShowPayment);
  });
  btnAlipayPaymentOpen.addEventListener('click', () => void openActiveAlipayPayment());
  btnAlipayPaymentCopy.addEventListener('click', () => void copyActiveAlipayPayment());
  btnAlipayPaymentComplete.addEventListener('click', () => void completeAlipayNetworkRecharge(false));
  btnAlipayPaymentClose.addEventListener('click', closeAlipayPaymentModal);
  btnAlipayPaymentCloseIcon.addEventListener('click', closeAlipayPaymentModal);
  alipayPaymentModal.addEventListener('click', event => {
    if (event.target === alipayPaymentModal) closeAlipayPaymentModal();
  });
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && !alipayPaymentModal.classList.contains('hidden')) {
      event.preventDefault();
      closeAlipayPaymentModal();
    }
  });
  window.addEventListener('focus', () => scheduleAlipayAutomaticCompletionCheck());
  window.addEventListener('focus', scheduleWechatAutomaticCompletionCheck);
  btnBillingWechatContinue.addEventListener('click', () => void completeWechatNetworkRecharge(false));
  billingQuestionsForm.addEventListener('submit', event => {
    event.preventDefault();
    if (!billingQuestionPassword.value) {
      void customAlert('请输入当前密码。');
      return;
    }
    const questions = billingQuestionAnswers();
    if (!questions) return;
    void performConfirmedBillingAction(
      { action: 'updateQuestions', oldPassword: billingQuestionPassword.value, questions },
      '更新密码保护',
      '密码保护答案遗失后可能无法找回，请确认已经记录。确定更新吗？',
      btnBillingQuestions,
    );
  });
  billingOnlineList.addEventListener('click', event => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('.action-billing-disconnect');
    if (button) void disconnectBillingSession(button);
  });

  btnTogglePermissions.addEventListener('click', () => {
    setPermissionHealthCollapsed(!permissionHealthGroup.classList.contains('is-collapsed'));
  });
  btnRefreshPermissions.addEventListener('click', () => void refreshPermissionHealth());
  permissionHealthList.addEventListener('click', async event => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('.action-permission-settings');
    const permission = button?.dataset.permission;
    if (!permission) return;
    const androidBridge = window.AndroidBridge;
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
    if (!window.__TAURI__) return;
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
    if (!button?.dataset.user || !window.__TAURI__) return;
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

  settingAndroidNotificationMode.addEventListener('change', () => {
    renderAndroidNotificationModeDescription();
    saveAndroidNotificationSetting(
      'bjut_android_notification_mode',
      settingAndroidNotificationMode.value === 'separate' ? 'separate' : 'combined',
    );
  });
  settingAndroidNotifyNetworkStatus.addEventListener('change', () => {
    saveAndroidNotificationSetting('bjut_android_notify_network_status', String(settingAndroidNotifyNetworkStatus.checked));
  });
  settingAndroidNotifyLoginResults.addEventListener('change', () => {
    saveAndroidNotificationSetting('bjut_android_notify_login_results', String(settingAndroidNotifyLoginResults.checked));
  });
  settingAndroidNotifyBackgroundErrors.addEventListener('change', () => {
    saveAndroidNotificationSetting('bjut_android_notify_background_errors', String(settingAndroidNotifyBackgroundErrors.checked));
  });
  settingUsageAlerts.addEventListener('change', () => saveUsageAlertSetting(settingUsageAlerts.checked));
  settingAndroidNotifyUsageAlerts.addEventListener('change', () => saveUsageAlertSetting(settingAndroidNotifyUsageAlerts.checked));
  settingVpnCompatibility.addEventListener('change', async () => {
    const value = settingVpnCompatibility.value || 'high';
    const previous = localStorage.getItem('bjut_vpn_compatibility') || 'high';
    if (value === 'maximum') {
      const confirmed = await customConfirm(
        '最高兼容模式会通过 HTTP + IP 发送校园网账号和密码，链路不具备 TLS 加密。\n\n该模式仅临时启用 15 分钟，随后自动回退到 HTTPS 高兼容模式。是否继续？',
        '确认启用明文传输',
      );
      if (!confirmed) {
        settingVpnCompatibility.value = previous;
        return;
      }
      vpnMaximumUntil = Math.floor(Date.now() / 1000) + 15 * 60;
      scheduleVpnMaximumRollback();
    } else {
      vpnMaximumUntil = 0;
      if (vpnMaximumRollbackTimer !== null) window.clearTimeout(vpnMaximumRollbackTimer);
      vpnMaximumRollbackTimer = null;
    }
    saveSetting('bjut_vpn_compatibility', value);
    const labels: Record<string, string> = {
      minimum: '最低兼容（HTTPS + 系统 DNS）',
      low: '较低兼容（HTTPS + 校园网 DNS）',
      high: '高兼容（HTTPS + 固定地址）',
      maximum: '最高兼容（HTTP + IP，账密明文传输）',
    };
    log('设置', `VPN 共存兼容等级已切换为${labels[value] || labels.high}`);
  });
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
      if (previousAccounts.find(account => account.user === user)) {
        customAlert('该账号已存在');
        return;
      }
      const nextAccounts = [
        ...previousAccounts.map(account => ({ ...account })),
        { user, pass, hasPassword: true, isDefault: previousAccounts.length === 0, isDisabled: false },
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
    if (logRenderFrame !== null) {
      cancelAnimationFrame(logRenderFrame);
      logRenderFrame = null;
    }
    logEntriesCache = [];
    renderedLogLimit = IS_ANDROID ? ANDROID_LOG_RENDER_BATCH : Number.MAX_SAFE_INTEGER;
    logFilterSignature = '';
    renderFilteredLogs({ resetWindow: true });
    if (window.__TAURI__) {
      invoke('clear_all_logs').catch(e => console.error(e));
    }
    window.AndroidBridge?.clearServiceLogs?.();
  });

  logSearch.addEventListener('input', () => renderFilteredLogs({ resetWindow: true }));
  logSessionFilter.addEventListener('change', () => renderFilteredLogs({ resetWindow: true }));
  logLevelFilter.addEventListener('change', () => renderFilteredLogs({ resetWindow: true }));
  getLogsScroller()?.addEventListener('scroll', loadOlderLogsNearTop, { passive: true });

  btnDiagnosticBundle.addEventListener('click', async () => {
    if (!window.__TAURI__) return;
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

  btnExportLogs.addEventListener('click', async () => {
    try {
      const androidBridge = window.AndroidBridge;
      if (window.__TAURI__ && androidBridge?.exportLogs) {
        const launched = Boolean(androidBridge.exportLogs());
        if (!launched) await customAlert('当前没有可导出的日志，或系统分享窗口启动失败。');
        return;
      }
      if (window.__TAURI__) {
        const destination = await invoke<string>('export_logs');
        await customAlert(`完整日志已导出到：\n${destination}`);
        return;
      }
      const text = logsContent.textContent || '';
      if (!text.trim()) {
        await customAlert('当前没有可导出的日志。');
        return;
      }
      const blob = new Blob([text], { type: 'text/plain;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `BJUT-AL-logs-${new Date().toISOString().replace(/[:.]/g, '-')}.log`;
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (error) {
      await customAlert(`导出日志失败：${String(error)}`);
    }
  });

  btnScrollLogs.addEventListener('click', () => {
    if (logsDirty) renderFilteredLogs();
    const scroller = getLogsScroller();
    if (scroller) requestAnimationFrame(() => scroller.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' }));
    else logsContent.scrollIntoView({ block: 'end', behavior: 'smooth' });
  });

  settingAutoLogin.addEventListener('change', async (e) => {
    autoLoginEnabled = (e.target as HTMLInputElement).checked;
    saveSetting('bjut_auto_login', autoLoginEnabled.toString());
    log('设置', `自动登录已${autoLoginEnabled ? '开启' : '关闭'}`);
    
    if (window.__TAURI__) {
      if (autoLoginEnabled) {
        if (window.AndroidBridge) {
          await customAlert('开启后，App 会申请后台位置与通知权限，并启动前台服务，以便在界面关闭后继续检测校园网。系统还会询问是否忽略电池优化；你可以稍后在权限健康中心调整这些权限。', 'Android 后台自动登录');
          
          try {
            window.AndroidBridge.requestBackgroundPermissions();
            window.AndroidBridge.requestBatteryOptimizations();
            window.AndroidBridge.startKeepAliveService();
            log('系统', '已开启后台保活服务，并申请后台权限');
          } catch (e) {
            console.error('Failed to request background services:', e);
          }
        }
      } else {
        if (window.AndroidBridge) {
          try {
            window.AndroidBridge.stopKeepAliveService();
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
      if (window.__TAURI__) {
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
    if (window.__TAURI__ && isMacos && !dockEnabled) {
      macosDockPolicyReady = invoke<void>('set_dock_visible', { visible: false })
        .catch(error => console.error('Failed to initialize macOS dock policy:', error));
    }
    settingMacosDock.addEventListener('change', async (e) => {
      const enabled = (e.target as HTMLInputElement).checked;
      localStorage.setItem('bjut_macos_dock', enabled.toString());
      log('设置', `已${enabled ? '启用' : '关闭'}在程序坞显示图标`);
      if (window.__TAURI__ && isMacos) {
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

      if (window.__TAURI__) {
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
        if (window.__TAURI__) {
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

  document.getElementById('btn-open-project-home')?.addEventListener('click', () => {
    void openUrl('https://github.com/key-zhzr/BJUT-Auto-Login').catch(error => {
      void customAlert(`无法打开开源仓库：${String(error)}`, '关于 BJUT-AL');
    });
  });
  document.getElementById('btn-open-project-license')?.addEventListener('click', () => {
    void openUrl('https://github.com/key-zhzr/BJUT-Auto-Login/blob/main/LICENSE').catch(error => {
      void customAlert(`无法打开开源许可：${String(error)}`, '关于 BJUT-AL');
    });
  });

  const btnCheckUpdate = document.getElementById('btn-check-update') as HTMLButtonElement | null;
  if (btnCheckUpdate) {
    btnCheckUpdate.addEventListener('click', async () => {
      const channel = settingUpdateChannel.value;
      log('系统', `正在检查更新 (通道: ${channel === 'release' ? '正式版' : '预览版'})...`);
      btnCheckUpdate.disabled = true;
      const originalText = btnCheckUpdate.textContent || '检查更新';
      btnCheckUpdate.textContent = '检查中…';

      try {
        if (!window.__TAURI__) {
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
      showListManageModal('信任的 WiFi (白名单)', [...whitelistCache], (newList) => {
        whitelistCache = [...newList];
        void syncConfigToRust();
      });
    });
  }

  const btnManageBlacklist = document.getElementById('btn-manage-blacklist');
  if (btnManageBlacklist) {
    btnManageBlacklist.addEventListener('click', () => {
      showListManageModal('拒绝的 WiFi (黑名单)', [...blacklistCache], (newList) => {
        blacklistCache = [...newList];
        void syncConfigToRust();
      });
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
          whitelist: JSON.stringify(whitelistCache),
          blacklist: JSON.stringify(blacklistCache),
          moreOptions: localStorage.getItem('bjut_more_options'),
          networkProfiles: networkProfilesCache,
          usageAlerts: localStorage.getItem('bjut_usage_alerts'),
          balanceAlertThreshold: localStorage.getItem('bjut_balance_alert_threshold'),
          flowAlertThreshold: localStorage.getItem('bjut_flow_alert_threshold'),
          vpnCompatibility: localStorage.getItem('bjut_vpn_compatibility'),
          androidNotificationMode: localStorage.getItem('bjut_android_notification_mode'),
          androidNotifyNetworkStatus: localStorage.getItem('bjut_android_notify_network_status'),
          androidNotifyLoginResults: localStorage.getItem('bjut_android_notify_login_results'),
          androidNotifyBackgroundErrors: localStorage.getItem('bjut_android_notify_background_errors'),
        };
        const encrypted = await encryptExport(config, passphrase);
        await writeTextToClipboard(encrypted);
        customAlert('配置已使用你设置的密码加密并复制到剪贴板。账号密码由 Rust 安全存储管理，不会进入导出内容。');
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
        if (Array.isArray(config.accounts)) {
          accountsCache = config.accounts.flatMap((value): AccountView[] => {
            if (!value || typeof value !== 'object') return [];
            const account = value as Partial<AccountView>;
            if (typeof account.user !== 'string') return [];
            return [{
              user: account.user,
              pass: typeof account.pass === 'string' ? account.pass : '',
              hasPassword: account.hasPassword === true || Boolean(account.pass),
              isDefault: account.isDefault === true,
              isDisabled: account.isDisabled === true,
            }];
          });
          config.accounts.forEach(value => {
            if (value && typeof value === 'object' && 'pass' in value) {
              (value as { pass?: unknown }).pass = '';
            }
          });
        }
        const restoreString = (source: string, destination: string) => {
          const value = config[source];
          if (typeof value === 'string') localStorage.setItem(destination, value);
        };
        restoreString('autoLogin', 'bjut_auto_login');
        restoreString('checkInterval', 'bjut_check_interval');
        restoreString('checkIntervalBg', 'bjut_check_interval_bg');
        restoreString('moreOptions', 'bjut_more_options');
        restoreString('usageAlerts', 'bjut_usage_alerts');
        restoreString('balanceAlertThreshold', 'bjut_balance_alert_threshold');
        restoreString('flowAlertThreshold', 'bjut_flow_alert_threshold');
        restoreString('vpnCompatibility', 'bjut_vpn_compatibility');
        restoreString('androidNotificationMode', 'bjut_android_notification_mode');
        restoreString('androidNotifyNetworkStatus', 'bjut_android_notify_network_status');
        restoreString('androidNotifyLoginResults', 'bjut_android_notify_login_results');
        restoreString('androidNotifyBackgroundErrors', 'bjut_android_notify_background_errors');
        if (typeof config.whitelist === 'string') {
          const values: unknown = JSON.parse(config.whitelist);
          if (Array.isArray(values)) whitelistCache = values.filter((item): item is string => typeof item === 'string');
        }
        if (typeof config.blacklist === 'string') {
          const values: unknown = JSON.parse(config.blacklist);
          if (Array.isArray(values)) blacklistCache = values.filter((item): item is string => typeof item === 'string');
        }
        if (Array.isArray(config.networkProfiles)) {
          networkProfilesCache = config.networkProfiles.filter((profile): profile is NetworkProfile => {
            if (!profile || typeof profile !== 'object') return false;
            const item = profile as Partial<NetworkProfile>;
            return typeof item.id === 'string' && typeof item.name === 'string'
              && typeof item.ssid === 'string' && typeof item.bssid === 'string'
              && typeof item.login_type === 'string' && Array.isArray(item.account_order);
          });
        }
        
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
    editAccPassword.value = '';
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
      if (previousAccounts.findIndex((account, accountIndex) => account.user === user && accountIndex !== index) !== -1) {
        customAlert('该账号名已存在');
        return;
      }
      const nextAccounts = previousAccounts.map((account, accountIndex) => accountIndex === index
        ? { ...account, user, pass, hasPassword: true }
        : { ...account });
      const submitButton = editAccountForm.querySelector<HTMLButtonElement>('button[type="submit"]');
      if (submitButton) submitButton.disabled = true;
      try {
        await saveAccounts(nextAccounts);
        editAccPassword.value = '';
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
  accountsList.addEventListener('click', async (e) => {
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
      if (textSpan && index >= 0) {
        if (textSpan.textContent === '*************') {
          const account = getAccounts()[index];
          if (!account?.hasPassword) return;
          const password = await invoke<string>('get_account_password', { user: account.user });
          textSpan.textContent = password;
          btn.classList.remove('hide-password');
          const previousTimer = passwordRevealTimers.get(textSpan);
          if (previousTimer !== undefined) window.clearTimeout(previousTimer);
          const timer = window.setTimeout(() => {
            textSpan.textContent = '*************';
            btn.classList.add('hide-password');
            passwordRevealTimers.delete(textSpan);
          }, 15_000);
          passwordRevealTimers.set(textSpan, timer);
        } else {
          const timer = passwordRevealTimers.get(textSpan);
          if (timer !== undefined) window.clearTimeout(timer);
          passwordRevealTimers.delete(textSpan);
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
      accounts.forEach((account, accountIndex) => account.isDefault = accountIndex === 0);
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
          el.style.willChange = 'transform';
          const animation = el.animate([
            { transform: `translate(${dx}px, ${dy}px)` },
            { transform: 'translate(0, 0)' }
          ], {
            duration: 400,
            easing: 'cubic-bezier(0.34, 1.56, 0.64, 1)'
          });
          void animation.finished
            .catch(() => undefined)
            .finally(() => el.style.removeProperty('will-change'));
        }
      });
      
      log('账号管理', `已将账号 ${account.user} 置顶`);
    } else if (btn.classList.contains('action-edit')) {
      const accounts = getAccounts();
      editAccIndex.value = index.toString();
      editAccUsername.value = accounts[index].user;
      editAccPassword.value = '';
      try {
        editAccPassword.value = await invoke<string>('get_account_password', { user: accounts[index].user });
        editModal.classList.remove('hidden');
      } catch (error) {
        await customAlert(`无法读取该账号密码：${String(error)}`);
      }
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
        if (accounts.length > 0 && !accounts.find(account => account.isDefault)) {
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
  let lastDragPoint: { clientX: number, clientY: number } | null = null;
  let dragGrabOffset: { x: number, y: number } | null = null;
  let draggedCardSize: { width: number, height: number } | null = null;
  let fallbackCaptureFrame: number | null = null;
  const pointFromDragEvent = (event: unknown): { clientX: number, clientY: number } | null => {
    const candidate = event as {
      clientX?: number;
      clientY?: number;
      touches?: ArrayLike<{ clientX: number; clientY: number }>;
      changedTouches?: ArrayLike<{ clientX: number; clientY: number }>;
    } | null;
    const point = candidate?.touches?.[0] || candidate?.changedTouches?.[0] || candidate;
    return Number.isFinite(point?.clientX) && Number.isFinite(point?.clientY)
      ? { clientX: point!.clientX!, clientY: point!.clientY! }
      : null;
  };
  const captureDragPoint = (event: Event) => {
    const point = pointFromDragEvent(event);
    if (point) lastDragPoint = point;
  };
  const startPointerCapture = () => {
    document.addEventListener('pointermove', captureDragPoint, true);
    document.addEventListener('mousemove', captureDragPoint, true);
    document.addEventListener('touchmove', captureDragPoint, { capture: true, passive: true });
  };
  const stopPointerCapture = () => {
    document.removeEventListener('pointermove', captureDragPoint, true);
    document.removeEventListener('mousemove', captureDragPoint, true);
    document.removeEventListener('touchmove', captureDragPoint, true);
  };
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
    onStart: (evt) => {
      lastFallbackRect = null;
      const itemRect = evt.item.getBoundingClientRect();
      draggedCardSize = { width: itemRect.width, height: itemRect.height };
      lastDragPoint = pointFromDragEvent((evt as { originalEvent?: Event }).originalEvent);
      dragGrabOffset = lastDragPoint
        ? { x: lastDragPoint.clientX - itemRect.left, y: lastDragPoint.clientY - itemRect.top }
        : null;
      startPointerCapture();
      if (fallbackCaptureFrame !== null) cancelAnimationFrame(fallbackCaptureFrame);
      fallbackCaptureFrame = requestAnimationFrame(captureFallbackRect);
    },
    onMove: (_evt, originalEvent) => {
      const point = pointFromDragEvent(originalEvent);
      if (point) lastDragPoint = point;
      const fallback = document.querySelector<HTMLElement>('.dragging-fallback');
      if (fallback) lastFallbackRect = fallback.getBoundingClientRect();
      return true;
    },
    onEnd: (evt) => {
      const { oldIndex, newIndex, item } = evt;

      if (fallbackCaptureFrame !== null) cancelAnimationFrame(fallbackCaptureFrame);
      fallbackCaptureFrame = null;
      const releasePoint = pointFromDragEvent((evt as { originalEvent?: Event }).originalEvent);
      if (releasePoint) lastDragPoint = releasePoint;
      stopPointerCapture();
      const pointerRect = lastDragPoint && dragGrabOffset && draggedCardSize
        ? {
            left: lastDragPoint.clientX - dragGrabOffset.x,
            top: lastDragPoint.clientY - dragGrabOffset.y,
            width: draggedCardSize.width,
            height: draggedCardSize.height,
          }
        : null;
      const releaseRect = pointerRect || lastFallbackRect;
      lastFallbackRect = null;
      lastDragPoint = null;
      dragGrabOffset = null;
      draggedCardSize = null;
      
      if (releaseRect) {
        // Create a clone to animate the fly-back natively bypassing overflow: hidden
        const clone = item.cloneNode(true) as HTMLElement;
        clone.classList.remove('dragging');
        clone.classList.add('dragging-fallback-clone');
        clone.style.removeProperty('transform');
        clone.style.removeProperty('transition');

        // onEnd can run while Sortable is still transitioning the real item
        // from an intermediate transform. Hide it and clear that transform
        // without a transition before reading the stable layout destination.
        item.classList.add('flyback-target');
        item.style.opacity = '0';
        item.style.removeProperty('transform');
        void getComputedStyle(item).transform;
        const finalRect = item.getBoundingClientRect();

        clone.style.position = 'fixed';
        clone.style.left = '0';
        clone.style.top = '0';
        clone.style.width = `${releaseRect.width}px`;
        clone.style.height = `${releaseRect.height}px`;
        clone.style.margin = '0';
        clone.style.willChange = 'transform';
        clone.setAttribute('aria-hidden', 'true');
        
        document.body.appendChild(clone);
        
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
        
        let cleanedUp = false;
        const cleanup = () => {
          if (cleanedUp) return;
          cleanedUp = true;
          clone.remove();
          item.style.opacity = '';
          // Commit the restored opacity while transitions are disabled. Without
          // this style flush, Android WebView can start an unintended fade-in
          // when flyback-target is removed on the next frame.
          void getComputedStyle(item).opacity;
          requestAnimationFrame(() => item.classList.remove('flyback-target'));
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
          accounts.forEach((account, accountIndex) => account.isDefault = accountIndex === 0);
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
  if (window.__TAURI__) {
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
    updateBillingAccountOptions();
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
          <span class="password-text${acc.hasPassword ? '' : ' password-missing'}" style="font-family: monospace; font-size: 0.9rem; color: var(--text-muted); display: inline-block; width: 7.5em; text-align: left;"></span>
          <button class="btn-icon action-toggle-password hide-password" style="padding: 0.2rem;" data-index="${index}" title="临时显示密码"${acc.hasPassword ? '' : ' disabled'}><i data-lucide="eye"></i></button>
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
    passwordText.textContent = acc.hasPassword ? '*************' : '未保存密码';
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
  renderIcons(accountsList);
  updateOverrideOptions();
  updateBillingAccountOptions();
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

function enabledBillingAccounts() {
  return getAccounts().filter(account => !account.isDisabled && account.user && account.hasPassword);
}

function defaultBillingAccountUser(): string {
  const accounts = enabledBillingAccounts();
  return accounts.find(account => account.isDefault)?.user || accounts[0]?.user || '';
}

function selectedBillingAccountUser(): string {
  return billingAccountSelect?.value || defaultBillingAccountUser();
}

function selectedRechargePayerAccount(): string {
  return billingRechargeCardAccountSelect?.value || selectedBillingAccountUser();
}

function syncRechargeTargetAccountInput() {
  if (!billingRechargeTargetAccountSelect) return;
  const selected = billingRechargeTargetAccountSelect.value;
  const custom = selected === '__custom__' || !selected;
  billingRechargeCustomTarget.hidden = !custom;
  if (!custom) billingRechargeAccount.value = selected;
  else if (enabledBillingAccounts().some(account => account.user === billingRechargeAccount.value)) {
    billingRechargeAccount.value = '';
  }
  billingRechargeAccount.required = custom;
  billingRechargeAccount.disabled = !custom;
}

function updateBillingAccountOptions() {
  if (!billingAccountSelect || !billingRechargeCardAccountSelect || !billingRechargeTargetAccountSelect) return;
  const accounts = enabledBillingAccounts();
  const previousBilling = billingAccountSelect.value;
  const options = accounts.map(account => ({
    value: account.user,
    text: `${account.user}${account.isDefault ? '（默认）' : ''}`,
  }));
  const fallback = accounts.find(account => account.isDefault)?.user || accounts[0]?.user || '';

  const currentBilling = accounts.some(account => account.user === billingAccountSelect.value)
    ? billingAccountSelect.value
    : fallback;
  billingAccountSelect.setOptions(options.length > 0 ? options : [{ value: '', text: '暂无可用账号' }]);
  billingAccountSelect.setValue(currentBilling);

  const currentPayer = accounts.some(account => account.user === billingRechargeCardAccountSelect.value)
    ? billingRechargeCardAccountSelect.value
    : currentBilling;
  billingRechargeCardAccountSelect.setOptions(options.length > 0 ? options : [{ value: '', text: '暂无可用账号' }]);
  billingRechargeCardAccountSelect.setValue(currentPayer);

  const previousTarget = billingRechargeTargetAccountSelect.value;
  const targetOptions = [
    ...options.map(option => ({ ...option, text: `已保存 · ${option.value}` })),
    { value: '__custom__', text: '自定义其他学工号' },
  ];
  billingRechargeTargetAccountSelect.setOptions(targetOptions);
  billingRechargeTargetAccountSelect.setValue(
    targetOptions.some(option => option.value === previousTarget)
      ? previousTarget
      : (currentBilling || '__custom__'),
  );
  syncRechargeTargetAccountInput();
  syncStandaloneBillingSecurity();
  if (billingCenterData && (billingCenterData.account !== currentBilling || previousBilling !== currentBilling)) {
    billingCenterData = null;
    billingRecordQueryStates = {};
    resetBillingCenterForSelectedAccount();
  }
}

// Split Network Check Loops
async function isAppInBackground(): Promise<boolean> {
  if (document.hidden) return true;
  if (!window.__TAURI__) {
    return !IS_ANDROID && !document.hasFocus();
  }
  try {
    const win = getCurrentWindow();
    const isVisible = await win.isVisible();
    const isMinimized = await win.isMinimized();
    const isFocused = await win.isFocused();
    return !isVisible || isMinimized || (!IS_ANDROID && !isFocused);
  } catch (e) {
    return !IS_ANDROID && !document.hasFocus();
  }
}

async function ensureBillingRequestForeground(): Promise<boolean> {
  if (!await isAppInBackground()) return true;
  billingCenterMessage.textContent = 'App 已进入后台，计费系统请求已暂停；返回前台后可手动刷新。';
  syncBillingCenterMessageVisibility();
  return false;
}

function startWifiChangeCheckLoop() {
  if (window.__TAURI__) return;
  if (wifiChangeTimer) {
    clearTimeout(wifiChangeTimer);
    wifiChangeTimer = null;
  }
  if (!wifiChangeDetectEnabled) return;

  const tick = async () => {
    if (window.__TAURI__) {
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
    wifiChangeTimer = window.setTimeout(tick, 3000);
  };

  tick();
}

// Native keep-alive hook for Android: called from Kotlin Handler every 10s
// to counteract Chromium's internal background timer throttling.
window.__nativeKeepAlive = () => {
  if (window.__TAURI__) return;
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
  if (window.__TAURI__) return;
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

  countdownInterval = window.setInterval(async () => {
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
  }, 1000);

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
  if (!window.__TAURI__) return;
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
    infoAccountLabel.textContent = '当前登录账号';
  }
  if (IS_ANDROID && window.AndroidBridge?.updateKeepAliveStatus) {
    const notification = state === NetworkState.Online
      ? '互联网已连接，后台自动登录正常'
      : state === NetworkState.BjutCampus
        ? '校园网需要认证，自动登录正在处理'
        : '网络离线或不在校园网环境';
    window.AndroidBridge.updateKeepAliveStatus(notification);
  }
  renderIcons(networkIcon);
}

function billingValueWithUnit(value: string | null | undefined, unit: string): string {
  const normalized = value?.trim();
  if (!normalized || normalized === '--') return '--';
  return normalized.includes(unit) ? normalized : `${normalized} ${unit}`;
}

function createBillingField(label: string, value: string): HTMLElement {
  const field = document.createElement('div');
  field.className = 'billing-record-field';
  const labelElement = document.createElement('span');
  labelElement.textContent = label;
  const valueElement = document.createElement('strong');
  valueElement.textContent = value || '--';
  field.append(labelElement, valueElement);
  return field;
}

function createBillingEmpty(text: string): HTMLElement {
  const empty = document.createElement('div');
  empty.className = 'billing-empty';
  empty.textContent = text;
  return empty;
}

function renderBillingOnlineSessions(sessions: BillingOnlineSession[]) {
  billingOnlineCount.textContent = String(sessions.length);
  if (sessions.length === 0) {
    billingOnlineList.replaceChildren(createBillingEmpty('当前没有在线会话'));
    return;
  }
  const fragment = document.createDocumentFragment();
  sessions.forEach(session => {
    const card = document.createElement('article');
    card.className = 'billing-record-card';
    const header = document.createElement('div');
    header.className = 'billing-record-header';
    const route = document.createElement('div');
    route.className = 'billing-record-route';
    const title = document.createElement('span');
    title.textContent = session.ip || session.ipv6 || '在线设备';
    route.appendChild(title);
    const disconnect = document.createElement('button');
    disconnect.type = 'button';
    disconnect.className = 'btn btn-danger btn-sm action-billing-disconnect';
    disconnect.textContent = '注销';
    disconnect.dataset.sessionId = session.sessionId;
    disconnect.dataset.ip = session.ip;
    disconnect.dataset.mac = session.mac;
    header.append(route, disconnect);
    const grid = document.createElement('div');
    grid.className = 'billing-record-grid';
    grid.append(
      createBillingField('上线时间', session.loginAt),
      createBillingField('IPv4', session.ip),
      createBillingField('IPv6', session.ipv6),
      createBillingField('MAC', session.mac),
      createBillingField('使用时长', billingValueWithUnit(session.durationMinutes, '分钟')),
      createBillingField('使用流量', billingValueWithUnit(session.usedFlowMb, 'MB')),
    );
    card.append(header, grid);
    fragment.appendChild(card);
  });
  billingOnlineList.replaceChildren(fragment);
}

function renderBillingHistory(records: BillingLoginRecord[]) {
  billingHistoryCount.textContent = String(records.length);
  billingHistoryPanel.open = false;
  if (records.length === 0) {
    billingHistoryList.replaceChildren(createBillingEmpty('暂无近期上网记录'));
    return;
  }
  const fragment = document.createDocumentFragment();
  records.forEach(record => {
    const card = document.createElement('article');
    card.className = 'billing-record-card';
    const header = document.createElement('div');
    header.className = 'billing-record-header';
    const route = document.createElement('div');
    route.className = 'billing-record-route';
    const login = document.createElement('span');
    login.textContent = record.loginAt;
    const arrow = document.createElement('i');
    arrow.setAttribute('data-lucide', 'arrow-right');
    const logout = document.createElement('span');
    logout.textContent = record.logoutAt;
    route.append(login, arrow, logout);
    header.appendChild(route);
    const grid = document.createElement('div');
    grid.className = 'billing-record-grid';
    grid.append(
      createBillingField('IPv4', record.ip),
      createBillingField('IPv6', record.ipv6),
      createBillingField('MAC', record.mac),
      createBillingField('使用时长', billingValueWithUnit(record.durationMinutes, '分钟')),
      createBillingField('使用流量', billingValueWithUnit(record.usedFlowMb, 'MB')),
      createBillingField('计费方式', record.billingMode),
      createBillingField('计费金额', billingValueWithUnit(record.amount, '元')),
    );
    card.append(header, grid);
    fragment.appendChild(card);
  });
  billingHistoryList.replaceChildren(fragment);
}

const billingRecordDefinitions: Record<BillingRecordKind, {
  table: keyof Pick<BillingCenterData, 'usageRecords' | 'monthlyBills' | 'payments' | 'operations' | 'stopLogs' | 'reopenLogs' | 'packageLogs'>;
  title: string;
  fields: { keys: string[]; label: string; unit?: string; format?: (value: string) => string }[];
}> = {
  usage: {
    table: 'usageRecords',
    title: '上网记录',
    fields: [
      { keys: ['loginTime'], label: '上线时间' },
      { keys: ['logoutTime'], label: '下线时间' },
      { keys: ['time'], label: '使用时长', unit: '分钟' },
      { keys: ['flow'], label: '使用流量', unit: 'MB' },
      { keys: ['costMoney'], label: '计费金额', unit: '元' },
      { keys: ['userIp'], label: 'IPv4' },
      { keys: ['userIp1'], label: 'IPv6' },
      { keys: ['macAddress'], label: 'MAC' },
      { keys: ['accessMethod'], label: '上网方式' },
    ],
  },
  monthly: {
    table: 'monthlyBills',
    title: '历史账单',
    fields: [
      { keys: ['startAt'], label: '账单开始' },
      { keys: ['endAt'], label: '账单结束' },
      { keys: ['package'], label: '套餐' },
      { keys: ['baseFee'], label: '基本月租', unit: '元' },
      { keys: ['usageFee'], label: '流量/时长计费', unit: '元' },
      { keys: ['durationMinutes'], label: '使用时长', unit: '分钟' },
      { keys: ['flowMb'], label: '使用流量', unit: 'MB' },
      { keys: ['billedAt'], label: '出账时间' },
    ],
  },
  payments: {
    table: 'payments',
    title: '充值明细',
    fields: [
      { keys: ['paidAt'], label: '交费时间' },
      { keys: ['amount'], label: '金额', unit: '元' },
      { keys: ['type'], label: '交费类型' },
      { keys: ['terminal'], label: '受理终端' },
      { keys: ['note'], label: '备注' },
    ],
  },
  operations: {
    table: 'operations',
    title: '业务办理记录',
    fields: [
      { keys: ['operatedAt'], label: '办理时间' },
      { keys: ['description'], label: '业务描述' },
      { keys: ['terminal'], label: '受理终端' },
      { keys: ['note'], label: '备注' },
    ],
  },
  stopLogs: {
    table: 'stopLogs',
    title: '报停记录',
    fields: [
      { keys: ['fldoperatedate'], label: '办理时间' },
      { keys: ['fldoperateid'], label: '业务描述', format: value => value === '8' ? '立即报停' : '预约报停' },
      { keys: ['fldadminid'], label: '受理终端' },
      { keys: ['fldmemo'], label: '备注' },
    ],
  },
  reopenLogs: {
    table: 'reopenLogs',
    title: '复通记录',
    fields: [
      { keys: ['fldoperatedate'], label: '办理时间' },
      { keys: ['fldoperateid'], label: '业务描述', format: value => value === '9' ? '立即复通' : '预约复通' },
      { keys: ['fldnewvalue'], label: '预约日期' },
      { keys: ['fldadminid'], label: '受理终端' },
      { keys: ['fldmemo'], label: '备注' },
    ],
  },
  packageLogs: {
    table: 'packageLogs',
    title: '套餐预约记录',
    fields: [
      { keys: ['fldchangedate'], label: '操作时间' },
      { keys: ['fldexcutedate'], label: '生效时间' },
      { keys: ['flddefaultname1'], label: '原套餐' },
      { keys: ['flddefaultname2'], label: '预约套餐' },
      { keys: ['fldstate'], label: '状态', format: value => value === '1' ? '有效' : '已取消' },
      { keys: ['fldstatedate'], label: '状态变更' },
      { keys: ['fldextend'], label: '备注' },
    ],
  },
};

function billingRowValue(row: Record<string, string>, keys: string[]): string {
  for (const key of keys) {
    if (row[key] !== undefined) return row[key];
    const actual = Object.keys(row).find(candidate => candidate.toLowerCase() === key.toLowerCase());
    if (actual) return row[actual];
  }
  return '--';
}

function activeBillingRecordKind(): BillingRecordKind {
  const value = billingRecordKindSelect?.value || 'usage';
  return (value in billingRecordDefinitions ? value : 'usage') as BillingRecordKind;
}

function currentBillingRecordQueryState(kind = activeBillingRecordKind()): BillingRecordQueryState {
  const fallbackYear = billingCenterData?.queryYear || String(new Date().getFullYear());
  const existing = billingRecordQueryStates[kind];
  if (existing) return existing;
  const state = {
    page: 1,
    pageSize: 10,
    startDate: billingCenterData?.queryStartDate || '',
    endDate: billingCenterData?.queryEndDate || '',
    year: fallbackYear,
    queried: false,
  };
  billingRecordQueryStates[kind] = state;
  return state;
}

function initializeBillingRecordQueryStates(data: BillingCenterData) {
  const next: Partial<Record<BillingRecordKind, BillingRecordQueryState>> = {};
  (Object.keys(billingRecordDefinitions) as BillingRecordKind[]).forEach(kind => {
    next[kind] = {
      page: 1,
      pageSize: 10,
      startDate: data.queryStartDate,
      endDate: data.queryEndDate,
      year: data.queryYear,
      queried: false,
    };
  });
  billingRecordQueryStates = next;
  billingRecordStartDate.max = data.queryEndDate;
  billingRecordEndDate.max = data.queryEndDate;
}

function billingKindUsesDate(kind: BillingRecordKind): boolean {
  return kind === 'usage' || kind === 'payments' || kind === 'operations';
}

function syncBillingRecordControls() {
  const kind = activeBillingRecordKind();
  const state = currentBillingRecordQueryState(kind);
  const usesDate = billingKindUsesDate(kind);
  const usesYear = kind === 'monthly';
  billingRecordDateFilter.hidden = !usesDate;
  billingRecordYearFilter.hidden = !usesYear;
  billingRecordStartDate.value = state.startDate;
  billingRecordEndDate.value = state.endDate;
  billingRecordYearSelect.setValue(state.year);
  billingRecordPageSizeSelect.setValue(String(state.pageSize));
  if (usesDate) {
    billingRecordRange.textContent = `${state.startDate} 至 ${state.endDate}；日期范围最多 60 天。`;
  } else if (usesYear) {
    billingRecordRange.textContent = `${state.year} 年历史账单。`;
  } else {
    billingRecordRange.textContent = '该类办理记录不提供日期筛选。';
  }
}

function readBillingRecordQuery(page: number, all = false): BillingRecordQuery | null {
  const kind = activeBillingRecordKind();
  const state = currentBillingRecordQueryState(kind);
  const pageSize = Number.parseInt(billingRecordPageSizeSelect.value || String(state.pageSize), 10);
  if (![10, 20, 25, 50, 100].includes(pageSize)) {
    void customAlert('请选择有效的每页记录数。');
    return null;
  }
  const query: BillingRecordQuery = { kind, page, pageSize, all };
  if (billingKindUsesDate(kind)) {
    const startDate = billingRecordStartDate.value;
    const endDate = billingRecordEndDate.value;
    const start = new Date(`${startDate}T00:00:00`);
    const end = new Date(`${endDate}T00:00:00`);
    const today = new Date();
    today.setHours(23, 59, 59, 999);
    if (!startDate || !endDate || Number.isNaN(start.getTime()) || Number.isNaN(end.getTime())
      || start > end || end > today || end.getTime() - start.getTime() > 60 * 86_400_000) {
      void customAlert('查询日期必须截至今天，且范围不能超过 60 天。');
      return null;
    }
    query.startDate = startDate;
    query.endDate = endDate;
  } else if (kind === 'monthly') {
    const year = billingRecordYearSelect.value;
    if (!/^\d{4}$/.test(year)) {
      void customAlert('请选择历史账单年份。');
      return null;
    }
    query.year = year;
  }
  return query;
}

function setBillingRecordQueryBusy(busy: boolean) {
  billingRecordQueryBusy = busy;
  btnQueryBillingRecords.disabled = busy || !billingCenterData;
  btnExportBillingRecords.disabled = busy || !currentBillingRecordSelection()?.table.rows.length;
  btnExportAllBillingRecords.disabled = busy || !currentBillingRecordSelection()?.table.total;
  if (busy) {
    btnBillingRecordPrev.disabled = true;
    btnBillingRecordNext.disabled = true;
  } else {
    renderBillingRecordPager();
  }
}

async function queryBillingRecords(page: number) {
  if (!billingCenterData || billingRecordQueryBusy) return;
  if (!await ensureBillingRequestForeground()) return;
  const query = readBillingRecordQuery(page);
  if (!query) return;
  const queriedKind = query.kind;
  const scrollContainer = billingRecordsList.closest('main') as HTMLElement | null;
  const previousScrollTop = scrollContainer?.scrollTop ?? 0;
  const previousListHeight = billingRecordsList.getBoundingClientRect().height;
  if (previousListHeight > 0) billingRecordsList.style.minHeight = `${previousListHeight}px`;
  const original = btnQueryBillingRecords.innerHTML;
  setBillingRecordQueryBusy(true);
  btnQueryBillingRecords.textContent = '查询中…';
  try {
    const result = await invoke<BillingRecordResult>('query_billing_records', {
      query,
      accountUser: selectedBillingAccountUser(),
    });
    if (!billingCenterData || result.kind !== queriedKind) {
      throw new Error('计费记录类型与请求不一致');
    }
    const definition = billingRecordDefinitions[queriedKind];
    billingCenterData[definition.table] = result.table;
    billingRecordQueryStates[queriedKind] = {
      page: result.page,
      pageSize: result.pageSize,
      startDate: result.startDate || query.startDate || '',
      endDate: result.endDate || query.endDate || '',
      year: result.year || query.year || billingCenterData.queryYear,
      queried: true,
    };
    if (activeBillingRecordKind() === queriedKind) {
      syncBillingRecordControls();
      renderBillingRecords();
    }
  } catch (error) {
    await customAlert(`账单记录查询失败：${String(error)}`);
  } finally {
    btnQueryBillingRecords.innerHTML = original;
    setBillingRecordQueryBusy(false);
    renderIcons(btnQueryBillingRecords);
    requestAnimationFrame(() => {
      if (scrollContainer) scrollContainer.scrollTop = previousScrollTop;
      billingRecordsList.style.removeProperty('min-height');
      requestAnimationFrame(() => {
        if (scrollContainer) scrollContainer.scrollTop = previousScrollTop;
      });
    });
  }
}

function currentBillingRecordSelection(): { kind: BillingRecordKind; definition: typeof billingRecordDefinitions[BillingRecordKind]; table: BillingTable } | null {
  if (!billingCenterData) return null;
  const kind = activeBillingRecordKind();
  const definition = billingRecordDefinitions[kind];
  return { kind, definition, table: billingCenterData[definition.table] };
}

function renderBillingRecordPager() {
  const selection = currentBillingRecordSelection();
  if (!selection) {
    billingRecordPageLabel.textContent = '第 1 / 1 页';
    btnBillingRecordPrev.disabled = true;
    btnBillingRecordNext.disabled = true;
    return;
  }
  const state = currentBillingRecordQueryState(selection.kind);
  if (!state.queried) {
    billingRecordPageLabel.textContent = '尚未查询';
    btnBillingRecordPrev.disabled = true;
    btnBillingRecordNext.disabled = true;
    return;
  }
  const pages = Math.max(1, Math.ceil(selection.table.total / state.pageSize));
  billingRecordPageLabel.textContent = `第 ${state.page} / ${pages} 页`;
  btnBillingRecordPrev.disabled = billingRecordQueryBusy || state.page <= 1;
  btnBillingRecordNext.disabled = billingRecordQueryBusy || state.page >= pages;
}

function renderBillingRecords() {
  const selection = currentBillingRecordSelection();
  if (!selection) {
    billingRecordTotal.textContent = '0';
    billingRecordSummary.replaceChildren();
    billingRecordsList.replaceChildren(createBillingEmpty('尚未读取完整账单数据'));
    btnExportBillingRecords.disabled = true;
    btnExportAllBillingRecords.disabled = true;
    renderBillingRecordPager();
    return;
  }
  const { kind, definition, table } = selection;
  const queryState = currentBillingRecordQueryState(kind);
  if (!queryState.queried) {
    billingRecordTotal.textContent = '0';
    billingRecordSummary.replaceChildren();
    billingRecordsList.replaceChildren(createBillingEmpty(`点击“查询”读取${definition.title}`));
    btnExportBillingRecords.disabled = true;
    btnExportAllBillingRecords.disabled = true;
    renderBillingRecordPager();
    return;
  }
  billingRecordTotal.textContent = String(table.total);
  btnExportBillingRecords.disabled = billingRecordQueryBusy || table.rows.length === 0;
  btnExportAllBillingRecords.disabled = billingRecordQueryBusy || table.total === 0;
  const summary = document.createDocumentFragment();
  Object.entries(table.summary).forEach(([name, value]) => {
    const item = document.createElement('span');
    item.className = 'billing-summary-chip';
    item.textContent = `${name}: ${value}`;
    summary.appendChild(item);
  });
  billingRecordSummary.replaceChildren(summary);
  if (table.rows.length === 0) {
    billingRecordsList.replaceChildren(createBillingEmpty(`暂无${definition.title}`));
    renderBillingRecordPager();
    return;
  }
  const fragment = document.createDocumentFragment();
  table.rows.forEach((row, index) => {
    const card = document.createElement('article');
    card.className = 'billing-record-card billing-module-record-card';
    const title = document.createElement('div');
    title.className = 'billing-record-header';
    const strong = document.createElement('strong');
    strong.textContent = `${definition.title} #${(queryState.page - 1) * queryState.pageSize + index + 1}`;
    title.appendChild(strong);
    const grid = document.createElement('div');
    grid.className = 'billing-record-grid';
    definition.fields.forEach(field => {
      let value = billingRowValue(row, field.keys);
      if (field.format) value = field.format(value);
      if (field.unit && value !== '--' && !value.endsWith(field.unit)) value = `${value} ${field.unit}`;
      grid.appendChild(createBillingField(field.label, value));
    });
    card.append(title, grid);
    fragment.appendChild(card);
  });
  billingRecordsList.replaceChildren(fragment);
  renderBillingRecordPager();
}

function normalizedPackageName(value: string | null | undefined): string {
  return (value || '').replace(/\s+/g, '').toLocaleLowerCase('zh-CN');
}

function billingPackageOptionIsCurrent(
  option: BillingPackageOption,
  service: BillingServiceState,
): boolean {
  return option.id === service.currentPackageId
    || (!service.currentPackageId
      && normalizedPackageName(option.name) === normalizedPackageName(service.currentPackage));
}

function updateBillingPackageActionButton() {
  const service = billingCenterData?.service;
  const selected = service?.packageOptions.find(option => option.id === selectedBillingPackageId);
  const restoresCurrentPackage = Boolean(
    service
    && selected
    && hasDistinctBillingPackageReservation
    && billingPackageOptionIsCurrent(selected, service),
  );
  btnBillingPackage.textContent = restoresCurrentPackage
    ? '下一周期沿用当前套餐'
    : '确认预约套餐';
  btnBillingPackage.disabled = !selectedBillingPackageId
    || selectedBillingPackageId === effectiveNextBillingPackageId;
}

function renderBillingService(data: BillingCenterData) {
  const service = data.service;
  billingServiceStatusBadge.textContent = service.accountStatus || '未知';
  billingServiceStatusBadge.className = `billing-state-badge ${service.accountStatus === '正常' ? 'success' : service.accountStatus ? 'warning' : 'neutral'}`;
  billingServiceReason.textContent = service.statusReason || service.packageDetail || '报停、复通、套餐预约与消费保护。';
  billingServicePackage.textContent = service.currentPackage || '--';
  billingServiceSettlement.textContent = service.nextSettlementDate || '--';
  billingServiceSpend.textContent = service.currentCycleSpend || '--';
  billingServiceLimit.textContent = service.consumeLimit || '--';
  btnBillingStopNow.disabled = !service.canStopNow;
  btnBillingReopenNow.disabled = !service.canReopenNow;
  btnBillingConsumeLimit.disabled = false;

  const currentName = service.currentPackage || '';
  const currentId = service.currentPackageId || '';
  const scheduledName = service.scheduledPackage || '';
  const scheduledId = service.scheduledPackageId || '';
  const scheduledMatchesCurrent = service.packageScheduled && (
    (Boolean(scheduledId) && Boolean(currentId) && scheduledId === currentId)
    || (Boolean(normalizedPackageName(scheduledName))
      && Boolean(normalizedPackageName(currentName))
      && normalizedPackageName(scheduledName) === normalizedPackageName(currentName))
  );
  const hasDistinctReservation = service.packageScheduled && !scheduledMatchesCurrent;
  hasDistinctBillingPackageReservation = hasDistinctReservation;
  effectiveNextBillingPackageName = hasDistinctReservation
    ? scheduledName
    : currentName;
  const nextOption = service.packageOptions.find(option => (
    (Boolean(scheduledId) && option.id === scheduledId)
    || (Boolean(normalizedPackageName(effectiveNextBillingPackageName))
      && normalizedPackageName(option.name) === normalizedPackageName(effectiveNextBillingPackageName))
  ));
  effectiveNextBillingPackageId = hasDistinctReservation
    ? (scheduledId || nextOption?.id || '')
    : currentId;
  selectedBillingPackageId = effectiveNextBillingPackageId;
  btnBillingCancelPackage.hidden = !hasDistinctReservation;
  btnBillingCancelPackage.disabled = !hasDistinctReservation;

  if (service.packageOptions.length === 0) {
    billingPackageOptions.replaceChildren(createBillingEmpty('当前没有可预约套餐'));
    btnBillingPackage.disabled = true;
  } else {
    const fragment = document.createDocumentFragment();
    service.packageOptions.forEach(option => {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'billing-package-option';
      button.dataset.packageId = option.id;
      const isCurrent = billingPackageOptionIsCurrent(option, service);
      const isNext = effectiveNextBillingPackageId
        ? option.id === effectiveNextBillingPackageId
        : normalizedPackageName(option.name) === normalizedPackageName(effectiveNextBillingPackageName);
      button.classList.toggle('selected', isNext);
      button.setAttribute('aria-pressed', String(isNext));
      const name = document.createElement('strong');
      name.textContent = option.name;
      const description = document.createElement('span');
      description.textContent = option.description || '计费系统未提供套餐说明';
      button.append(name, description);
      if (isCurrent || isNext) {
        const badges = document.createElement('div');
        badges.className = 'billing-package-badges';
        if (isCurrent) {
          const badge = document.createElement('span');
          badge.className = 'billing-package-badge current';
          badge.textContent = '当前周期';
          badges.appendChild(badge);
        }
        if (isNext) {
          const badge = document.createElement('span');
          badge.className = 'billing-package-badge next';
          badge.textContent = '下一周期';
          badges.appendChild(badge);
        }
        button.appendChild(badges);
      }
      fragment.appendChild(button);
    });
    billingPackageOptions.replaceChildren(fragment);
  }
  updateBillingPackageActionButton();
}

function renderBillingDevices(data: BillingCenterData) {
  billingDeviceCount.textContent = String(data.devices.total);
  btnBillingBindMac.disabled = false;
  if (data.devices.rows.length === 0) {
    billingDeviceList.replaceChildren(createBillingEmpty('暂无已绑定设备'));
  } else {
    const fragment = document.createDocumentFragment();
    data.devices.rows.forEach(row => {
      const mac = billingRowValue(row, ['mac', '1']);
      const card = document.createElement('article');
      card.className = 'billing-record-card';
      const header = document.createElement('div');
      header.className = 'billing-record-header';
      const title = document.createElement('strong');
      title.textContent = mac;
      const unbind = document.createElement('button');
      unbind.type = 'button';
      unbind.className = 'btn btn-danger btn-sm action-billing-unbind-mac';
      unbind.dataset.mac = mac;
      unbind.textContent = '解除绑定';
      header.append(title, unbind);
      const grid = document.createElement('div');
      grid.className = 'billing-record-grid';
      grid.append(
        createBillingField('在线状态', billingRowValue(row, ['online', '0']) === '0' ? '离线' : '在线'),
        createBillingField('终端信息', billingRowValue(row, ['device', '2'])),
        createBillingField('最近登录', billingRowValue(row, ['lastLoginAt', '3'])),
        createBillingField('最近 IP', billingRowValue(row, ['lastIp', '4'])),
      );
      card.append(header, grid);
      fragment.appendChild(card);
    });
    billingDeviceList.replaceChildren(fragment);
  }

  if (!billingRechargeState.textContent?.trim() || billingRechargeState.textContent.includes('正在读取')) {
    billingRechargeState.textContent = activeRechargeMethod === 'alipay'
      ? '选择校园卡、目标学工号和金额后，App 将先核对所选校园卡，再创建支付宝支付入口。'
      : activeRechargeMethod === 'wechat'
        ? '选择校园卡、目标学工号和金额后，App 将核对账户，并由后端保持会话直接唤起微信。'
        : '选择校园卡、目标学工号和金额后，App 将通过统一认证核对双方账户与可充值状态。';
  }
}

function applyBillingPasswordPolicy(policy: BillingPasswordPolicy) {
  const requirements = [
    `${policy.minLength}–${policy.maxLength} 位`,
    policy.requireUppercase ? '大写字母' : '',
    policy.requireLowercase ? '小写字母' : '',
    policy.requireDigit ? '数字' : '',
    policy.requireSpecial ? '特殊字符 !@#$%^&*()' : '',
  ].filter(Boolean);
  billingPasswordPolicy.textContent = `密码要求：${requirements.join('、')}`;
  [billingOldPassword, billingNewPassword, billingConfirmPassword].forEach(input => {
    input.maxLength = policy.maxLength || 16;
  });
}

function syncStandaloneBillingSecurity() {
  applyBillingPasswordPolicy(UNIFIED_AUTH_PASSWORD_POLICY);
  btnBillingPassword.disabled = !selectedBillingAccountUser();
}

function renderBillingSecurity(data: BillingCenterData) {
  applyBillingPasswordPolicy(data.passwordPolicy || UNIFIED_AUTH_PASSWORD_POLICY);
  btnBillingPassword.disabled = !selectedBillingAccountUser();
  const questionOptions = data.securityQuestions.map(question => ({ value: question.id, text: question.text }));
  billingQuestionSelects.forEach((select, index) => {
    select.setOptions(questionOptions.map(option => ({ ...option })));
    select.setValue('');
    select.triggerSpan.textContent = `选择问题${['一', '二', '三'][index]}`;
  });
  btnBillingQuestions.disabled = questionOptions.length === 0;
}

function resetBillingCenterForSelectedAccount() {
  const account = selectedBillingAccountUser();
  billingCenterAccount.textContent = account || '--';
  billingCenterBalance.textContent = '--';
  billingCenterFlow.textContent = '--';
  billingCenterStatus.textContent = '--';
  billingCenterStatus.className = '';
  billingMauthBadge.className = 'billing-state-badge neutral';
  billingMauthBadge.textContent = '未知';
  btnToggleBillingMauth.disabled = true;
  btnToggleBillingMauth.textContent = '状态未知';
  renderBillingOnlineSessions([]);
  renderBillingHistory([]);
  billingRecordTotal.textContent = '0';
  billingRecordSummary.replaceChildren();
  billingRecordsList.replaceChildren(createBillingEmpty('请查询所选账号的账单记录'));
  billingServiceStatusBadge.className = 'billing-state-badge neutral';
  billingServiceStatusBadge.textContent = '未知';
  billingServiceReason.textContent = '正在读取所选账号的账户服务状态。';
  [billingServicePackage, billingServiceSettlement, billingServiceSpend, billingServiceLimit]
    .forEach(element => { element.textContent = '--'; });
  btnBillingStopNow.disabled = true;
  btnBillingReopenNow.disabled = true;
  btnBillingConsumeLimit.disabled = true;
  btnBillingPackage.disabled = true;
  selectedBillingPackageId = '';
  effectiveNextBillingPackageId = '';
  effectiveNextBillingPackageName = '';
  hasDistinctBillingPackageReservation = false;
  btnBillingPackage.textContent = '确认预约套餐';
  btnBillingCancelPackage.hidden = true;
  btnBillingCancelPackage.disabled = true;
  billingPackageOptions.replaceChildren(createBillingEmpty('正在读取可预约套餐…'));
  billingDeviceCount.textContent = '0';
  btnBillingBindMac.disabled = true;
  billingDeviceList.replaceChildren(createBillingEmpty('正在读取设备列表…'));
  billingCenterMessage.textContent = account ? `正在读取账号 ${account} 的计费数据` : '请先添加并启用一个已保存密码的账号';
  syncBillingCenterMessageVisibility();
  syncStandaloneBillingSecurity();
  renderIcons(document.getElementById('billing-center')!);
}

function renderBillingCenterData(data: BillingCenterData) {
  billingCenterData = data;
  const info = billingOverviewToUserInfo(data.overview);
  renderBillingCenter(info);
  initializeBillingRecordQueryStates(data);
  syncBillingRecordControls();
  renderBillingRecords();
  renderBillingService(data);
  renderBillingDevices(data);
  renderBillingSecurity(data);
  const messages = [
    ...info.billingWarnings,
    ...data.warnings,
  ].filter(Boolean) as string[];
  billingCenterMessage.textContent = messages.join('\n');
  syncBillingCenterMessageVisibility();
  renderIcons(document.getElementById('billing-center')!);
}

function updateBillingRefreshProgress(percent: number, loading: boolean) {
  const normalized = Math.max(0, Math.min(100, Math.round(Number.isFinite(percent) ? percent : 0)));
  btnRefreshBillingCenter.style.setProperty('--billing-refresh-progress', `${normalized}%`);
  btnRefreshBillingCenter.classList.toggle('is-loading', loading);
  billingRefreshLabel.textContent = loading ? `${normalized}%` : '刷新';
  if (loading) {
    btnRefreshBillingCenter.setAttribute('role', 'progressbar');
    btnRefreshBillingCenter.setAttribute('aria-valuemin', '0');
    btnRefreshBillingCenter.setAttribute('aria-valuemax', '100');
    btnRefreshBillingCenter.setAttribute('aria-valuenow', String(normalized));
  } else {
    btnRefreshBillingCenter.removeAttribute('role');
    btnRefreshBillingCenter.removeAttribute('aria-valuemin');
    btnRefreshBillingCenter.removeAttribute('aria-valuemax');
    btnRefreshBillingCenter.removeAttribute('aria-valuenow');
  }
}

async function refreshBillingCenterData() {
  if (billingRecordQueryBusy || billingCenterLoading) return;
  if (!await ensureBillingRequestForeground()) return;
  const accountUser = selectedBillingAccountUser();
  if (!accountUser) {
    resetBillingCenterForSelectedAccount();
    return;
  }
  billingCenterLoading = true;
  btnRefreshBillingCenter.disabled = true;
  billingAccountSelect.setDisabled(true);
  setBillingRecordQueryBusy(true);
  updateBillingRefreshProgress(2, true);
  billingCenterMessage.textContent = '正在连接计费系统';
  syncBillingCenterMessageVisibility();
  try {
    const data = await invoke<BillingCenterData>('get_billing_center', { accountUser });
    if (selectedBillingAccountUser() !== accountUser) return;
    renderBillingCenterData(data);
  } catch (error) {
    billingCenterMessage.textContent = `完整计费数据读取失败：${String(error)}`;
    syncBillingCenterMessageVisibility();
    syncStandaloneBillingSecurity();
  } finally {
    btnRefreshBillingCenter.disabled = false;
    billingAccountSelect.setDisabled(false);
    updateBillingRefreshProgress(0, false);
    setBillingRecordQueryBusy(false);
    billingCenterLoading = false;
  }
}

function buildBillingCsv(
  table: BillingTable,
  definition: typeof billingRecordDefinitions[BillingRecordKind],
): string {
  const safeCell = (raw: string) => {
    const value = /^[=+\-@]/.test(raw.trimStart()) ? `'${raw}` : raw;
    return `"${value.replaceAll('"', '""')}"`;
  };
  const columns = definition.fields;
  const lines = [
    columns.map(column => safeCell(column.label)).join(','),
    ...table.rows.map(row => columns.map(column => {
      let value = billingRowValue(row, column.keys);
      if (column.format) value = column.format(value);
      return safeCell(value);
    }).join(',')),
  ];
  return `\uFEFF${lines.join('\r\n')}`;
}

async function saveBillingCsv(kind: BillingRecordKind, title: string, csv: string) {
  if (window.__TAURI__) {
    const destination = await invoke<string>('export_billing_csv', { kind, csv });
    const androidBridge = window.AndroidBridge;
    if (IS_ANDROID && androidBridge?.shareExportFile) {
      const launched = Boolean(androidBridge.shareExportFile(destination, `BJUT-AL ${title}`));
      if (!launched) throw new Error('系统分享窗口启动失败');
      return;
    }
    await customAlert(`CSV 已导出到：\n${destination}`, '导出完成');
    return;
  }
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `BJUT-AL-${title}-${new Date().toISOString().slice(0, 10)}.csv`;
  anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

async function exportBillingRecords(all: boolean) {
  const selection = currentBillingRecordSelection();
  if (!selection || (all ? selection.table.total === 0 : selection.table.rows.length === 0)) return;
  if (all && !await ensureBillingRequestForeground()) return;
  const button = all ? btnExportAllBillingRecords : btnExportBillingRecords;
  const original = button.innerHTML;
  setBillingRecordQueryBusy(true);
  button.textContent = all ? '读取全部…' : '导出中…';
  try {
    let table = selection.table;
    if (all) {
      const query = readBillingRecordQuery(1, true);
      if (!query) return;
      const result = await invoke<BillingRecordResult>('query_billing_records', {
        query,
        accountUser: selectedBillingAccountUser(),
      });
      if (result.kind !== selection.kind) throw new Error('计费记录类型与请求不一致');
      table = result.table;
    }
    const csv = buildBillingCsv(table, selection.definition);
    await saveBillingCsv(selection.kind, selection.definition.title, csv);
  } catch (error) {
    await customAlert(`导出账单记录失败：${String(error)}`);
  } finally {
    button.innerHTML = original;
    setBillingRecordQueryBusy(false);
    renderIcons(button);
  }
}

function billingOverviewToUserInfo(overview: BillingOverview): UserInfo {
  return {
    account: overview.account,
    balance: overview.balance,
    flow: overview.remainingFlow,
    source: 'billing',
    status: overview.status,
    statusReason: overview.statusReason,
    package: overview.package,
    packageDetail: overview.packageDetail,
    usedFlow: overview.usedFlow,
    billingCycle: overview.billingCycle,
    updatedAt: overview.updatedAt,
    loginHistory: overview.loginHistory || [],
    onlineSessions: overview.onlineSessions || [],
    offlineTip: overview.offlineTip,
    mauthEnabled: overview.mauthEnabled,
    billingWarnings: overview.warnings || [],
  };
}

function clearBillingSecretInputs() {
  billingOldPassword.value = '';
  billingNewPassword.value = '';
  billingConfirmPassword.value = '';
  billingQuestionPassword.value = '';
  [1, 2, 3].forEach(index => {
    const answer = document.getElementById(`billing-answer-${index}`) as HTMLInputElement;
    answer.value = '';
  });
}

function activateRechargeMethod(method: RechargeMethod) {
  if (!['campus-card', 'alipay', 'wechat'].includes(method)) return;
  if (method === 'wechat' && !IS_ANDROID) return;
  activeRechargeMethod = method;
  billingRechargeMethodButtons.forEach(button => {
    const active = button.dataset.rechargeMethodTarget === method;
    button.classList.toggle('active', active);
    button.setAttribute('aria-selected', String(active));
  });
  const alipay = method === 'alipay';
  const wechat = method === 'wechat';
  billingRechargeMethodTitle.textContent = alipay
    ? '支付宝充值网费'
    : wechat
      ? '微信充值网费'
      : '从校园卡转入网费';
  billingRechargeMethodDescription.textContent = alipay
    ? '支付宝先为所选校园卡充值，到账后 App 会继续把同一金额转入目标网费账户。'
    : wechat
      ? '后端保持学校支付会话并直接唤起微信，支付确认后 App 会继续把同一金额转入目标网费账户。'
      : '将先核对校园卡余额和目标账户，二次确认后再执行一次扣费。';
  btnBillingRecharge.textContent = alipay
    ? '核对并前往支付宝'
    : wechat
      ? '核对并前往微信支付'
      : '核对充值信息';
  billingRechargeState.textContent = alipay
    ? '目标和金额保持不变；支付宝页会保留原会话，支付完成后按学校流程自动回到 ydapp，App 会自动核对到账。'
    : wechat
      ? '目标和金额保持不变；后端将以同一会话进入 Tenpay，取得受信任的微信协议地址后直接唤起微信。'
      : '校园卡、目标和金额保持不变；确认后从所选账号的校园卡直接转入网费。';
  if (activeAlipayPayment) {
    btnBillingAlipayShowPayment.hidden = IS_ANDROID || !alipay;
  } else {
    btnBillingAlipayShowPayment.hidden = true;
  }
  btnBillingWechatContinue.hidden = !wechat || !activeWechatPayment;
}

function setRechargeBusy(busy: boolean, label?: string) {
  btnBillingRecharge.disabled = busy;
  billingRechargeCardAccountSelect.setDisabled(busy);
  billingRechargeTargetAccountSelect.setDisabled(busy);
  billingRechargeAccount.disabled = busy || billingRechargeTargetAccountSelect.value !== '__custom__';
  billingRechargeAmount.disabled = busy;
  billingRechargeMethodButtons.forEach(button => { button.disabled = busy; });
  btnBillingRecharge.textContent = label || (activeRechargeMethod === 'alipay'
    ? '核对并前往支付宝'
    : activeRechargeMethod === 'wechat'
      ? '核对并前往微信支付'
      : '核对充值信息');
}

function updateRechargeProgress(percent: number, text: string, visible = true) {
  const normalized = Math.max(0, Math.min(100, Math.round(percent)));
  billingRechargeProgress.hidden = !visible;
  billingRechargeProgress.setAttribute('aria-valuenow', String(normalized));
  billingRechargeProgressText.textContent = text;
  billingRechargeProgressPercent.textContent = `${normalized}%`;
  billingRechargeProgressBar.style.width = `${normalized}%`;
}

function finishRechargeProgress(text: string) {
  updateRechargeProgress(100, text);
  window.setTimeout(() => {
    if (billingRechargeProgressText.textContent === text) {
      billingRechargeProgress.hidden = true;
    }
  }, 1800);
}

function formatBillingCurrency(value: string) {
  const normalized = value.trim();
  return normalized.endsWith('元') ? normalized : `${normalized} 元`;
}

function parseBillingCurrency(value: string) {
  const matched = value.replaceAll(',', '').match(/-?\d+(?:\.\d+)?/);
  return matched ? Number(matched[0]) : Number.NaN;
}

function rechargeServiceIsOpen(now = Date.now()) {
  const beijingHour = Math.floor((now / 3_600_000 + 8) % 24);
  return beijingHour >= 6 && beijingHour < 23;
}

async function ensureRechargeServiceOpen(title: string) {
  if (rechargeServiceIsOpen()) return true;
  await customAlert(
    '充值系统仅在北京时间每日 06:00–23:00 开放。当前可查看账户与余额，但不能创建或确认充值订单。',
    title,
  );
  return false;
}

function renderRechargePreview(preview: RechargePreview) {
  billingRechargePayer.textContent = preview.payerAccount;
  billingRechargeCardBalance.textContent = formatBillingCurrency(preview.cardBalance);
  billingRechargeTargetStatus.textContent = preview.targetStatus;
  billingRechargeTargetBalance.textContent = formatBillingCurrency(preview.targetBalance);
  billingRechargePreview.hidden = false;
  billingRechargeState.textContent = `信息已核对；充值入口开放时间 ${preview.allowedTime}，确认信息将在 ${preview.expiresInSeconds} 秒后失效。`;
}

function renderRechargeBalances(snapshot: RechargeBalanceSnapshot) {
  billingRechargePayer.textContent = snapshot.payerAccount;
  billingRechargeCardBalance.textContent = formatBillingCurrency(snapshot.cardBalance);
  billingRechargeTargetStatus.textContent = snapshot.targetStatus;
  billingRechargeTargetBalance.textContent = formatBillingCurrency(snapshot.targetBalance);
  billingRechargePreview.hidden = false;
}

async function refreshRechargeBalancesOnce(
  targetAccount: string,
  accountUser = selectedRechargePayerAccount(),
): Promise<string | null> {
  updateRechargeProgress(88, '正在刷新校园卡与目标网费余额…');
  try {
    const snapshot = await invoke<RechargeBalanceSnapshot>('get_network_recharge_balances', {
      targetAccount,
      accountUser,
    });
    renderRechargeBalances(snapshot);
    return null;
  } catch (error) {
    return String(error);
  }
}

async function prepareAndConfirmNetworkRecharge() {
  if (btnBillingRecharge.disabled) return;
  if (!await ensureRechargeServiceOpen('充值网费')) return;
  const targetAccount = billingRechargeAccount.value.trim();
  const accountUser = selectedRechargePayerAccount();
  if (!accountUser) {
    await customAlert('请选择一个已保存密码的校园卡账户。', '充值网费');
    return;
  }
  const amount = billingRechargeAmount.value.trim();
  if (!/^[A-Za-z0-9_-]{5,20}$/.test(targetAccount)) {
    await customAlert('请输入 5–20 位有效学工号。', '充值网费');
    return;
  }
  if (!/^\d+(?:\.\d{1,2})?$/.test(amount) || Number(amount) <= 0 || Number(amount) > 500) {
    await customAlert('充值金额必须大于 0、不超过 500 元，且最多保留两位小数。', '充值网费');
    return;
  }
  billingRechargePreview.hidden = true;
  billingRechargeState.textContent = '正在通过统一认证核对校园卡和目标网费账户…';
  setRechargeBusy(true, '正在核对…');
  updateRechargeProgress(12, '正在登录统一认证并核对账户…');
  try {
    const preview = await invoke<RechargePreview>('prepare_network_recharge', {
      targetAccount,
      amount,
      accountUser,
    });
    updateRechargeProgress(42, '账户与余额核对完成，等待确认…');
    renderRechargePreview(preview);
    const confirmed = await customConfirm(
      `付款校园卡：${preview.payerAccount}\n校园卡余额：${formatBillingCurrency(preview.cardBalance)}\n目标学工号：${preview.targetAccount}\n目标网费余额：${formatBillingCurrency(preview.targetBalance)}\n充值金额：${formatBillingCurrency(preview.amount)}\n\n确认后将创建一次订单并从校园卡扣费。`,
      '确认校园卡充值',
    );
    if (!confirmed) {
      await invoke('cancel_network_recharge', {
        confirmationId: preview.confirmationId,
      }).catch(() => undefined);
      billingRechargeState.textContent = '已取消充值，没有创建订单或扣费。';
      billingRechargeProgress.hidden = true;
      return;
    }
    setRechargeBusy(true, '正在充值…');
    billingRechargeState.textContent = '正在创建一次性充值订单并确认校园卡扣费，请勿重复操作…';
    updateRechargeProgress(68, '正在创建一次性订单并提交扣费…');
    const result = await invoke<RechargeResult>('confirm_network_recharge', {
      confirmationId: preview.confirmationId,
    });
    updateRechargeProgress(84, '充值已提交，正在读取最新余额…');
    const balanceError = await refreshRechargeBalancesOnce(targetAccount, accountUser);
    billingRechargeAmount.value = '';
    billingRechargeState.textContent = balanceError
      ? `${result.message}；余额刷新失败：${balanceError}`
      : `${result.message}；校园卡与目标网费余额已刷新。`;
    finishRechargeProgress(balanceError ? '充值完成，余额暂未刷新' : '充值与余额刷新完成');
    await customAlert(billingRechargeState.textContent, '充值完成');
  } catch (error) {
    billingRechargeState.textContent = `充值未完成：${String(error)}`;
    billingRechargeProgress.hidden = true;
    await customAlert(`充值未完成：${String(error)}`, '充值网费');
  } finally {
    setRechargeBusy(false);
  }
}

function isTrustedAlipayPaymentUrl(value: string) {
  try {
    const url = new URL(value);
    return url.protocol === 'https:'
      && url.hostname === 'openapi.alipay.com'
      && url.pathname === '/gateway.do'
      && url.searchParams.get('method') === 'alipay.trade.wap.pay'
      && url.searchParams.get('sign_type') === 'RSA2';
  } catch {
    return false;
  }
}

function closeAlipayPaymentModal() {
  alipayPaymentModal.classList.add('hidden');
  alipayPaymentModal.setAttribute('aria-hidden', 'true');
  const returnFocus = alipayPaymentModalReturnFocus;
  alipayPaymentModalReturnFocus = null;
  if (returnFocus?.isConnected) {
    window.setTimeout(() => returnFocus.focus(), 0);
  }
}

function clearActiveAlipayPayment() {
  if (alipayAutomaticCheckTimer !== null) {
    window.clearTimeout(alipayAutomaticCheckTimer);
    alipayAutomaticCheckTimer = null;
  }
  activeAlipayPayment = null;
  alipayExternalHandoffAt = 0;
  alipayAutomaticCheckCount = 0;
  alipayCompletionBusy = false;
  btnBillingAlipayShowPayment.hidden = true;
  btnAlipayPaymentComplete.disabled = false;
  closeAlipayPaymentModal();
  alipayPaymentQrShell.classList.remove('has-error');
  alipayPaymentQrFallback.hidden = true;
  const context = alipayPaymentQr.getContext('2d');
  context?.clearRect(0, 0, alipayPaymentQr.width, alipayPaymentQr.height);
}

function rememberActiveAlipayPayment(payment: ActiveAlipayPayment) {
  activeAlipayPayment = payment;
  alipayExternalHandoffAt = Date.now();
  alipayAutomaticCheckCount = 0;
  btnBillingAlipayShowPayment.hidden = IS_ANDROID;
  scheduleAlipayAutomaticCompletionCheck(3500);
}

function setAlipayCompletionBusy(busy: boolean) {
  alipayCompletionBusy = busy;
  btnAlipayPaymentComplete.disabled = busy;
  setRechargeBusy(busy, busy ? '正在完成充值…' : undefined);
}

function scheduleAlipayAutomaticCompletionCheck(delay = 650) {
  if (!activeAlipayPayment
    || !alipayExternalHandoffAt
    || alipayCompletionBusy
    || alipayAutomaticCheckCount >= 180
    || !rechargeServiceIsOpen()) return;
  if (alipayAutomaticCheckTimer !== null) window.clearTimeout(alipayAutomaticCheckTimer);
  alipayAutomaticCheckTimer = window.setTimeout(async () => {
    alipayAutomaticCheckTimer = null;
    if (!activeAlipayPayment || alipayCompletionBusy) return;
    if (await isAppInBackground()) return;
    alipayAutomaticCheckCount += 1;
    void completeAlipayNetworkRecharge(true);
  }, delay);
}

async function showAlipayPaymentModal(returnFocus?: HTMLElement) {
  const payment = activeAlipayPayment;
  if (!payment || !isTrustedAlipayPaymentUrl(payment.paymentUrl)) {
    await customAlert('当前没有可用的支付宝支付入口，请重新核对充值信息。', '支付宝充值');
    return;
  }

  alipayPaymentModalReturnFocus = returnFocus
    || (document.activeElement instanceof HTMLElement ? document.activeElement : null);
  alipayPaymentPayer.textContent = payment.payerAccount;
  alipayPaymentAmount.textContent = formatBillingCurrency(payment.amount);
  alipayPaymentTarget.textContent = payment.targetAccount;
  alipayPaymentQrShell.classList.remove('has-error');
  alipayPaymentQrFallback.hidden = true;
  alipayPaymentModalStatus.textContent = '正在本机生成支付二维码…';
  alipayPaymentModal.classList.remove('hidden');
  alipayPaymentModal.setAttribute('aria-hidden', 'false');
  btnAlipayPaymentCloseIcon.focus();

  try {
    const QRCode = await import('qrcode');
    await QRCode.toCanvas(alipayPaymentQr, payment.paymentUrl, {
      errorCorrectionLevel: 'L',
      margin: 2,
      width: 360,
      color: {
        dark: '#0f172a',
        light: '#ffffff',
      },
    });
    if (activeAlipayPayment !== payment) return;
    alipayPaymentModalStatus.textContent = '二维码已在本机生成；支付链接未上传到任何二维码服务。';
  } catch {
    if (activeAlipayPayment !== payment) return;
    alipayPaymentQrShell.classList.add('has-error');
    alipayPaymentQrFallback.hidden = false;
    alipayPaymentModalStatus.textContent = '二维码生成失败，仍可打开或复制本次支付链接。';
  }
}

async function copyActiveAlipayPayment() {
  const payment = activeAlipayPayment;
  if (!payment || !isTrustedAlipayPaymentUrl(payment.paymentUrl)) {
    alipayPaymentModalStatus.textContent = '当前支付链接已经失效，请重新核对充值信息。';
    return;
  }
  try {
    await writeTextToClipboard(payment.paymentUrl);
    alipayPaymentModalStatus.textContent = '支付链接已复制。请仅发送到自己的手机，并尽快完成支付。';
  } catch (error) {
    alipayPaymentModalStatus.textContent = `复制失败：${String(error)}`;
  }
}

async function openActiveAlipayPayment() {
  const payment = activeAlipayPayment;
  if (!payment || !isTrustedAlipayPaymentUrl(payment.paymentUrl)) {
    alipayPaymentModalStatus.textContent = '当前支付链接已经失效，请重新核对充值信息。';
    return false;
  }
  alipayExternalHandoffAt = Date.now();
  alipayAutomaticCheckCount = 0;
  scheduleAlipayAutomaticCompletionCheck(3500);
  try {
    const androidBridge = window.AndroidBridge;
    if (IS_ANDROID && androidBridge?.openAlipay) {
      const launched = Boolean(androidBridge.openAlipay(payment.paymentUrl));
      if (launched) {
        alipayPaymentModalStatus.textContent = '已直接打开支付宝；支付页会保留原会话，并在完成后按学校流程返回 ydapp。';
        billingRechargeState.textContent = '已直接打开支付宝；返回 App 后会自动检测到账，无需再点“支付完成”。';
        return true;
      }
    }
    await openUrl(payment.paymentUrl);
    alipayPaymentModalStatus.textContent = IS_ANDROID
      ? '未检测到可处理该订单的支付宝，已改用系统浏览器；支付完成后页面会按学校流程返回 ydapp。'
      : '支付页面已在系统浏览器中保留原会话；完成后会按学校流程自动返回 ydapp。';
    billingRechargeState.textContent = alipayPaymentModalStatus.textContent;
    return true;
  } catch (error) {
    alipayExternalHandoffAt = 0;
    alipayPaymentModalStatus.textContent = `系统浏览器打开失败：${String(error)}。可复制支付链接后手动打开。`;
    return false;
  }
}

async function completeAlipayNetworkRecharge(automatic = false) {
  if (alipayCompletionBusy) return;
  if (!rechargeServiceIsOpen()) {
    if (!automatic) await ensureRechargeServiceOpen('支付宝充值');
    return;
  }
  if (await isAppInBackground()) return;
  const payment = activeAlipayPayment;
  if (!payment) {
    if (!automatic) await customAlert('当前没有等待处理的支付宝订单，请重新发起充值。', '支付宝充值');
    return;
  }

  let preparedConfirmationId: string | null = null;
  let transferSubmitted = false;
  setAlipayCompletionBusy(true);
  const checkingText = automatic
    ? '正在自动检测支付宝是否已充入校园卡…'
    : '正在检测校园卡到账并核对目标网费账户…';
  billingRechargeState.textContent = checkingText;
  alipayPaymentModalStatus.textContent = checkingText;
  updateRechargeProgress(48, checkingText);

  try {
    const snapshot = await invoke<RechargeBalanceSnapshot>('get_network_recharge_balances', {
      targetAccount: payment.targetAccount,
      accountUser: payment.payerAccount,
    });
    if (activeAlipayPayment !== payment) {
      return;
    }
    if (snapshot.payerAccount !== payment.payerAccount || snapshot.targetAccount !== payment.targetAccount) {
      throw new Error('到账核对返回的校园卡或目标网费账户与当前订单不一致');
    }
    renderRechargeBalances(snapshot);

    const balanceBefore = parseBillingCurrency(payment.cardBalanceBefore);
    const balanceNow = parseBillingCurrency(snapshot.cardBalance);
    const paidAmount = parseBillingCurrency(payment.amount);
    const balanceComparable = [balanceBefore, balanceNow, paidAmount].every(Number.isFinite);
    const arrivalDetected = balanceComparable
      && balanceNow - balanceBefore >= paidAmount - 0.005;

    if (!arrivalDetected) {
      const message = balanceComparable
        ? `支付宝页面仍保留本次会话；暂未检测到到账（校园卡余额 ${formatBillingCurrency(snapshot.cardBalance)}），App 会在前台继续自动检查。`
        : '暂时无法比较校园卡余额；支付宝页面仍保留本次会话，App 会在前台继续自动检查。';
      billingRechargeState.textContent = message;
      alipayPaymentModalStatus.textContent = message;
      billingRechargeProgress.hidden = true;
      if (!automatic) await customAlert(message, '支付宝充值');
      return;
    }

    if (automatic) {
      alipayAutomaticCheckCount = 180;
      const message = `检测到校园卡余额增加 ${formatBillingCurrency(String(balanceNow - balanceBefore))}。余额变化本身不能证明对应订单已经到账，请点击“核对并转入网费”确认后再继续。`;
      billingRechargeState.textContent = message;
      alipayPaymentModalStatus.textContent = message;
      billingRechargeProgress.hidden = true;
      return;
    }
    const confirmedArrival = await customConfirm(
      `检测到校园卡余额由 ${formatBillingCurrency(payment.cardBalanceBefore)} 增加到 ${formatBillingCurrency(snapshot.cardBalance)}。\n\n学校接口没有返回可核对的支付宝订单号。请确认这次余额增加确实来自当前 ${formatBillingCurrency(payment.amount)} 的支付宝订单；确认后才会继续转入目标网费账户 ${payment.targetAccount}。`,
      '确认支付宝到账',
    );
    if (!confirmedArrival) {
      billingRechargeState.textContent = '已暂停自动转入；请先核对校园卡充值记录，确认后再继续。';
      alipayPaymentModalStatus.textContent = billingRechargeState.textContent;
      billingRechargeProgress.hidden = true;
      return;
    }

    billingRechargeState.textContent = '已检测到支付宝到账，正在按首次确认的信息准备转入目标网费账户…';
    updateRechargeProgress(64, '支付宝已到账，正在准备网费转入…');
    const preview = await invoke<RechargePreview>('prepare_network_recharge', {
      targetAccount: payment.targetAccount,
      amount: payment.amount,
      accountUser: payment.payerAccount,
    });
    preparedConfirmationId = preview.confirmationId;
    if (activeAlipayPayment !== payment) {
      await invoke('cancel_network_recharge', { confirmationId: preview.confirmationId }).catch(() => undefined);
      preparedConfirmationId = null;
      return;
    }
    if (preview.payerAccount !== payment.payerAccount || preview.targetAccount !== payment.targetAccount) {
      await invoke('cancel_network_recharge', { confirmationId: preview.confirmationId }).catch(() => undefined);
      preparedConfirmationId = null;
      throw new Error('网费转入核对结果与首次确认的信息不一致');
    }

    if (!alipayPaymentModal.classList.contains('hidden')) closeAlipayPaymentModal();
    billingRechargeState.textContent = '正在按首次确认的信息从校园卡转入目标网费账户，请勿重复操作…';
    updateRechargeProgress(72, '正在自动转入目标网费账户…');
    transferSubmitted = true;
    preparedConfirmationId = null;
    const result = await invoke<RechargeResult>('confirm_network_recharge', {
      confirmationId: preview.confirmationId,
    });
    updateRechargeProgress(84, '网费转入完成，正在刷新双方余额…');
    const balanceError = await refreshRechargeBalancesOnce(payment.targetAccount, payment.payerAccount);
    await invoke('finish_recharge_recovery', {
      id: payment.recoveryId,
      completed: true,
      note: '支付宝到账并已转入目标网费账户',
    }).catch(() => undefined);
    clearActiveAlipayPayment();
    billingRechargeAmount.value = '';
    billingRechargeState.textContent = balanceError
      ? `${result.message}。支付宝 → 校园卡 → 网费账户流程已完成；余额刷新失败：${balanceError}`
      : `${result.message}。支付宝 → 校园卡 → 网费账户流程已完成，最新余额已读取。`;
    finishRechargeProgress(balanceError ? '充值完成，余额暂未刷新' : '充值与余额刷新完成');
    await customAlert(billingRechargeState.textContent, '充值完成');
  } catch (error) {
    if (preparedConfirmationId) {
      await invoke('cancel_network_recharge', { confirmationId: preparedConfirmationId }).catch(() => undefined);
    }
    const detail = String(error);
    if (transferSubmitted) {
      await invoke('finish_recharge_recovery', {
        id: payment.recoveryId,
        completed: false,
        note: detail,
      }).catch(() => undefined);
      clearActiveAlipayPayment();
    }
    const message = transferSubmitted
      ? `网费转入已提交，但结果未能确认：${detail}。为避免重复扣费，App 不会自动重试；请先查询校园卡和网费记录。`
      : automatic
        ? `自动到账核对暂未完成：${detail}。App 会在前台继续重试。`
        : `到账核对或网费转入未完成：${detail}`;
    billingRechargeState.textContent = message;
    alipayPaymentModalStatus.textContent = message;
    billingRechargeProgress.hidden = true;
    if (!automatic) await customAlert(message, '支付宝充值');
  } finally {
    setAlipayCompletionBusy(false);
    if (activeAlipayPayment && alipayAutomaticCheckCount < 180) {
      scheduleAlipayAutomaticCompletionCheck(6000);
    }
  }
}

async function prepareAndOpenAlipayRecharge() {
  if (btnBillingRecharge.disabled) return;
  if (!await ensureRechargeServiceOpen('支付宝充值')) return;
  const targetAccount = billingRechargeAccount.value.trim();
  const amount = billingRechargeAmount.value.trim();
  const accountUser = selectedRechargePayerAccount();
  if (!accountUser) {
    await customAlert('请选择一个已保存密码的校园卡账户。', '支付宝充值');
    return;
  }
  if (!/^[A-Za-z0-9_-]{5,20}$/.test(targetAccount)) {
    await customAlert('请输入 5–20 位有效学工号。', '支付宝充值');
    return;
  }
  if (!/^\d+(?:\.\d{1,2})?$/.test(amount) || Number(amount) <= 0 || Number(amount) > 500) {
    await customAlert('充值金额必须大于 0、不超过 500 元，且最多保留两位小数。', '支付宝充值');
    return;
  }

  if (activeAlipayPayment) {
    const sameOrder = activeAlipayPayment.payerAccount === accountUser
      && activeAlipayPayment.targetAccount === targetAccount
      && Number(activeAlipayPayment.amount) === Number(amount);
    const replace = await customConfirm(
      sameOrder
        ? '当前仍保留着相同金额和目标的支付宝支付入口。仅在原入口已经失效或确定不再支付时才应创建新订单。\n\n选择“取消”将继续使用原支付入口，选择“确定”将创建新订单。'
        : '当前仍保留着另一个支付宝支付入口。仅在确认原订单不再支付时才应创建新订单。\n\n选择“取消”将继续使用原支付入口，选择“确定”将替换它。',
      '替换现有支付订单',
    );
    if (!replace) {
      if (IS_ANDROID) {
        const opened = await openActiveAlipayPayment();
        if (!opened) {
          btnBillingAlipayShowPayment.hidden = false;
          await showAlipayPaymentModal(btnBillingRecharge);
        }
      } else {
        await showAlipayPaymentModal(btnBillingRecharge);
      }
      return;
    }
  }

  clearActiveAlipayPayment();
  billingRechargePreview.hidden = true;
  billingRechargeState.textContent = '正在通过统一认证核对当前校园卡…';
  setRechargeBusy(true, '正在核对…');
  updateRechargeProgress(10, '正在登录统一认证并读取校园卡…');
  try {
    const preview = await invoke<AlipayRechargePreview>('prepare_alipay_card_recharge', {
      targetAccount,
      amount,
      accountUser,
    });
    billingRechargePayer.textContent = preview.payerAccount;
    billingRechargeCardBalance.textContent = formatBillingCurrency(preview.cardBalance);
    billingRechargeTargetStatus.textContent = '待支付宝付款';
    billingRechargeTargetBalance.textContent = '完成后刷新';
    billingRechargePreview.hidden = false;
    billingRechargeState.textContent = `校园卡信息已核对；确认信息将在 ${preview.expiresInSeconds} 秒后失效。`;
    updateRechargeProgress(32, '校园卡已核对，等待确认创建支付订单…');
    const confirmed = await customConfirm(
      `充值校园卡账号：${preview.payerAccount}\n支付宝金额：${formatBillingCurrency(preview.amount)}\n最终网费目标：${targetAccount}\n\nApp 将先创建“支付宝 → 当前校园卡”订单。支付宝页会保留本次学校会话，支付完成后自动跳回 ydapp；检测到账后，App 会按本次确认的信息自动转入目标网费账户，不再要求点击“支付完成”。`,
      '确认前往支付宝',
    );
    if (!confirmed) {
      await invoke('cancel_alipay_card_recharge', {
        confirmationId: preview.confirmationId,
      }).catch(() => undefined);
      billingRechargeState.textContent = '已取消，没有创建支付宝订单。';
      billingRechargeProgress.hidden = true;
      return;
    }
    setRechargeBusy(true, '正在创建订单…');
    billingRechargeState.textContent = '正在创建一次性支付宝订单，请勿重复操作…';
    updateRechargeProgress(46, '正在创建一次性支付宝订单…');
    const result = await invoke<AlipayRechargeResult>('confirm_alipay_card_recharge', {
      confirmationId: preview.confirmationId,
    });
    if (!isTrustedAlipayPaymentUrl(result.paymentUrl)) {
      billingRechargeState.textContent = '支付宝订单已经创建，但支付平台返回了不受信任的跳转地址。请勿立即重复创建订单。';
      throw new Error('支付平台返回了不受信任的跳转地址');
    }
    rememberActiveAlipayPayment({
      recoveryId: preview.confirmationId,
      paymentUrl: result.paymentUrl,
      payerAccount: result.payerAccount,
      amount: result.amount,
      targetAccount,
      cardBalanceBefore: preview.cardBalance,
    });
    billingRechargeState.textContent = `${result.message}。支付宝页会在支付完成后自动返回 ydapp；App 保持前台时会自动检测到账并衔接网费转入。`;
    updateRechargeProgress(55, '支付订单已创建，等待支付宝付款…');
    if (IS_ANDROID) {
      const opened = await openActiveAlipayPayment();
      if (!opened) {
        btnBillingAlipayShowPayment.hidden = false;
        await showAlipayPaymentModal(btnBillingRecharge);
      }
    } else {
      await showAlipayPaymentModal(btnBillingRecharge);
    }
  } catch (error) {
    if (!billingRechargeState.textContent.includes('订单已经创建')) {
      billingRechargeState.textContent = `支付宝充值未开始：${String(error)}`;
    }
    billingRechargeProgress.hidden = true;
    await customAlert(billingRechargeState.textContent, '支付宝充值');
  } finally {
    setRechargeBusy(false);
  }
}

function isTrustedWechatLaunchUrl(value: string) {
  try {
    const url = new URL(value);
    const query = url.search.slice(1).toLowerCase();
    return value.length <= 4096
      && url.protocol === 'weixin:'
      && url.hostname === 'wap'
      && url.pathname === '/pay'
      && !url.username
      && !url.password
      && !url.port
      && !url.hash
      && query.length >= 16
      && ['prepay', 'package', 'sign'].every(marker => query.includes(marker));
  } catch {
    return false;
  }
}

function clearActiveWechatPayment() {
  if (wechatAutomaticCheckTimer !== null) {
    window.clearTimeout(wechatAutomaticCheckTimer);
    wechatAutomaticCheckTimer = null;
  }
  activeWechatPayment = null;
  wechatExternalHandoffAt = 0;
  wechatLastAutomaticCheckAt = 0;
  wechatAutomaticCheckCount = 0;
  wechatCompletionBusy = false;
  btnBillingWechatContinue.hidden = true;
  btnBillingWechatContinue.disabled = false;
}

function rememberActiveWechatPayment(payment: ActiveWechatPayment) {
  activeWechatPayment = payment;
  wechatExternalHandoffAt = 0;
  wechatLastAutomaticCheckAt = 0;
  wechatAutomaticCheckCount = 0;
  btnBillingWechatContinue.hidden = activeRechargeMethod !== 'wechat';
}

function setWechatCompletionBusy(busy: boolean) {
  wechatCompletionBusy = busy;
  btnBillingWechatContinue.disabled = busy;
  setRechargeBusy(busy, busy ? '正在确认微信支付…' : undefined);
}

function scheduleWechatAutomaticCompletionCheck() {
  if (!activeWechatPayment
    || !wechatExternalHandoffAt
    || wechatCompletionBusy
    || document.hidden
    || wechatAutomaticCheckCount >= 20) return;
  const now = Date.now();
  if (now - wechatExternalHandoffAt < 800 || now - wechatLastAutomaticCheckAt < 2200) return;
  if (wechatAutomaticCheckTimer !== null) window.clearTimeout(wechatAutomaticCheckTimer);
  wechatAutomaticCheckTimer = window.setTimeout(() => {
    wechatAutomaticCheckTimer = null;
    if (!activeWechatPayment || wechatCompletionBusy || document.hidden) return;
    wechatLastAutomaticCheckAt = Date.now();
    wechatAutomaticCheckCount += 1;
    void completeWechatNetworkRecharge(true);
  }, 650);
}

async function openActiveWechatPayment() {
  const payment = activeWechatPayment;
  if (!payment || !isTrustedWechatLaunchUrl(payment.launchUrl)) {
    await customAlert('当前没有可用的微信支付入口，请重新核对充值信息。', '微信充值');
    return false;
  }
  wechatExternalHandoffAt = Date.now();
  wechatLastAutomaticCheckAt = 0;
  wechatAutomaticCheckCount = 0;
  try {
    // Rust has already visited Tenpay with the order-creation session and a
    // regular UA. Only the validated weixin:// launch address reaches JS.
    const androidBridge = window.AndroidBridge;
    if (IS_ANDROID && androidBridge?.openWechat) {
      if (!androidBridge.openWechat(payment.launchUrl)) {
        throw new Error('未检测到可处理该订单的微信客户端');
      }
    } else {
      await openUrl(payment.launchUrl);
    }
    billingRechargeState.textContent = '已直接唤起微信支付；支付完成后返回 App，将自动确认订单并转入网费。';
    updateRechargeProgress(55, '等待微信支付完成…');
    return true;
  } catch (error) {
    wechatExternalHandoffAt = 0;
    billingRechargeState.textContent = `微信支付页打开失败：${String(error)}`;
    return false;
  }
}

async function completeWechatNetworkRecharge(automatic = false) {
  if (wechatCompletionBusy) return;
  if (!rechargeServiceIsOpen()) {
    if (!automatic) await ensureRechargeServiceOpen('微信充值');
    return;
  }
  const payment = activeWechatPayment;
  if (!payment) {
    if (!automatic) await customAlert('当前没有等待处理的微信订单，请重新发起充值。', '微信充值');
    return;
  }

  let preparedConfirmationId: string | null = null;
  let transferSubmitted = false;
  setWechatCompletionBusy(true);
  billingRechargeState.textContent = '正在向学校支付平台确认微信订单状态…';
  updateRechargeProgress(58, '正在确认微信支付状态…');
  try {
    const status = await invoke<WechatPaymentStatus>('check_wechat_card_recharge', {
      paymentId: payment.paymentId,
    });
    if (activeWechatPayment !== payment) return;
    if (status.status !== 'paid') {
      billingRechargeState.textContent = automatic
        ? '学校支付平台暂未确认微信付款，App 将在前台继续检查。'
        : '暂未确认微信付款；若已完成支付，请稍候几秒再检查。';
      updateRechargeProgress(55, '等待微信支付完成…');
      if (automatic && wechatAutomaticCheckCount < 20) {
        wechatAutomaticCheckTimer = window.setTimeout(() => {
          wechatAutomaticCheckTimer = null;
          if (!activeWechatPayment || wechatCompletionBusy || document.hidden) return;
          wechatLastAutomaticCheckAt = Date.now();
          wechatAutomaticCheckCount += 1;
          void completeWechatNetworkRecharge(true);
        }, 2400);
      }
      return;
    }

    billingRechargeState.textContent = `${status.message}，正在核对目标网费账户并准备自动转入…`;
    updateRechargeProgress(68, '微信已支付，正在准备网费转入…');
    const preview = await invoke<RechargePreview>('prepare_network_recharge', {
      targetAccount: payment.targetAccount,
      amount: payment.amount,
      accountUser: payment.payerAccount,
    });
    preparedConfirmationId = preview.confirmationId;
    if (activeWechatPayment !== payment) {
      await invoke('cancel_network_recharge', { confirmationId: preview.confirmationId }).catch(() => undefined);
      preparedConfirmationId = null;
      return;
    }
    if (preview.payerAccount !== payment.payerAccount || preview.targetAccount !== payment.targetAccount) {
      await invoke('cancel_network_recharge', { confirmationId: preview.confirmationId }).catch(() => undefined);
      preparedConfirmationId = null;
      throw new Error('到账核对返回的校园卡或目标网费账户与当前微信订单不一致');
    }

    renderRechargePreview(preview);
    billingRechargeState.textContent = '微信支付已确认，正在按首次确认的信息从校园卡转入目标网费账户，请勿重复操作…';
    updateRechargeProgress(76, '正在从校园卡转入目标网费账户…');
    transferSubmitted = true;
    preparedConfirmationId = null;
    const result = await invoke<RechargeResult>('confirm_network_recharge', {
      confirmationId: preview.confirmationId,
    });
    updateRechargeProgress(88, '网费转入完成，正在刷新双方余额…');
    const balanceError = await refreshRechargeBalancesOnce(payment.targetAccount, payment.payerAccount);
    await invoke('finish_recharge_recovery', {
      id: payment.paymentId,
      completed: true,
      note: '微信到账并已转入目标网费账户',
    });
    await invoke('cancel_wechat_card_recharge', {
      confirmationId: null,
      paymentId: payment.paymentId,
    }).catch(() => undefined);
    clearActiveWechatPayment();
    billingRechargeAmount.value = '';
    billingRechargeState.textContent = balanceError
      ? `${result.message}。微信 → 校园卡 → 网费账户流程已完成；余额刷新失败：${balanceError}`
      : `${result.message}。微信 → 校园卡 → 网费账户流程已完成，最新余额已读取。`;
    finishRechargeProgress(balanceError ? '充值完成，余额暂未刷新' : '充值与余额刷新完成');
    await customAlert(billingRechargeState.textContent, '微信充值完成');
  } catch (error) {
    if (preparedConfirmationId) {
      await invoke('cancel_network_recharge', { confirmationId: preparedConfirmationId }).catch(() => undefined);
    }
    const detail = String(error);
    const uncertain = transferSubmitted
      && /结果未知|订单已经创建|不要立即重复|未能确认/.test(detail);
    if (uncertain) clearActiveWechatPayment();
    const message = automatic
      ? `微信支付后的自动处理暂未完成：${detail}。可稍后点击“检测微信支付并转入网费”重试。`
      : `微信支付确认或网费转入未完成：${detail}`;
    billingRechargeState.textContent = message;
    billingRechargeProgress.hidden = true;
    if (!automatic) await customAlert(message, '微信充值');
  } finally {
    setWechatCompletionBusy(false);
  }
}

async function prepareAndOpenWechatRecharge() {
  if (!IS_ANDROID || btnBillingRecharge.disabled) return;
  if (!await ensureRechargeServiceOpen('微信充值')) return;
  const targetAccount = billingRechargeAccount.value.trim();
  const amount = billingRechargeAmount.value.trim();
  const accountUser = selectedRechargePayerAccount();
  if (!accountUser) {
    await customAlert('请选择一个已保存密码的校园卡账户。', '微信充值');
    return;
  }
  if (!/^[A-Za-z0-9_-]{5,20}$/.test(targetAccount)) {
    await customAlert('请输入 5–20 位有效学工号。', '微信充值');
    return;
  }
  if (!/^\d+(?:\.\d{1,2})?$/.test(amount) || Number(amount) <= 0 || Number(amount) > 500) {
    await customAlert('充值金额必须大于 0、不超过 500 元，且最多保留两位小数。', '微信充值');
    return;
  }

  if (activeWechatPayment) {
    const sameOrder = activeWechatPayment.payerAccount === accountUser
      && activeWechatPayment.targetAccount === targetAccount
      && Number(activeWechatPayment.amount) === Number(amount);
    const replace = await customConfirm(
      sameOrder
        ? '当前仍保留着相同金额和目标的微信支付入口。仅在原入口已经失效或确定不再支付时才应创建新订单。\n\n选择“取消”将继续使用原支付入口，选择“确定”将创建新订单。'
        : '当前仍保留着另一个微信支付入口。仅在确认原订单不再支付时才应创建新订单。\n\n选择“取消”将继续使用原支付入口，选择“确定”将替换它。',
      '替换现有微信订单',
    );
    if (!replace) {
      await openActiveWechatPayment();
      return;
    }
    await invoke('cancel_wechat_card_recharge', {
      confirmationId: null,
      paymentId: activeWechatPayment.paymentId,
    }).catch(() => undefined);
    clearActiveWechatPayment();
  }

  billingRechargePreview.hidden = true;
  billingRechargeState.textContent = '正在核对当前校园卡与目标网费账户…';
  setRechargeBusy(true, '正在核对…');
  updateRechargeProgress(10, '正在复用移动门户登录状态并核对账户…');
  try {
    const preview = await invoke<WechatRechargePreview>('prepare_wechat_card_recharge', {
      targetAccount,
      amount,
      accountUser,
    });
    billingRechargePayer.textContent = preview.payerAccount;
    billingRechargeCardBalance.textContent = formatBillingCurrency(preview.cardBalance);
    billingRechargeTargetStatus.textContent = preview.targetStatus;
    billingRechargeTargetBalance.textContent = formatBillingCurrency(preview.targetBalance);
    billingRechargePreview.hidden = false;
    updateRechargeProgress(34, '校园卡与目标网费账户已核对，等待确认…');
    const confirmed = await customConfirm(
      `充值校园卡账号：${preview.payerAccount}\n当前校园卡余额：${formatBillingCurrency(preview.cardBalance)}\n微信支付金额：${formatBillingCurrency(preview.amount)}\n最终网费目标：${preview.targetAccount}\n目标当前余额：${formatBillingCurrency(preview.targetBalance)}\n\n确认后后端将保持当前支付会话进入 Tenpay，并直接唤起微信；支付成功后会自动把同一金额从校园卡转入上述网费账户。`,
      '确认前往微信支付',
    );
    if (!confirmed) {
      await invoke('cancel_wechat_card_recharge', {
        confirmationId: preview.confirmationId,
        paymentId: null,
      }).catch(() => undefined);
      billingRechargeState.textContent = '已取消，没有创建微信支付订单。';
      billingRechargeProgress.hidden = true;
      return;
    }
    setRechargeBusy(true, '正在创建订单…');
    billingRechargeState.textContent = '正在创建一次性微信支付订单，请勿重复操作…';
    updateRechargeProgress(46, '正在创建一次性微信支付订单…');
    const result = await invoke<WechatRechargeResult>('confirm_wechat_card_recharge', {
      confirmationId: preview.confirmationId,
    });
    if (!isTrustedWechatLaunchUrl(result.launchUrl)
      || result.payerAccount !== preview.payerAccount
      || result.targetAccount !== preview.targetAccount) {
      throw new Error('学校支付平台返回的微信订单信息未通过安全校验');
    }
    rememberActiveWechatPayment({
      paymentId: result.paymentId,
      launchUrl: result.launchUrl,
      payerAccount: result.payerAccount,
      amount: result.amount,
      targetAccount: result.targetAccount,
      cardBalanceBefore: preview.cardBalance,
    });
    billingRechargeState.textContent = `${result.message}。支付后返回 App，将自动确认并转入网费。`;
    updateRechargeProgress(55, '支付订单已创建，等待微信付款…');
    if (!await openActiveWechatPayment()) {
      await customAlert('微信支付订单已创建，但未能直接唤起微信。请稍后重试现有订单，避免重复创建。', '微信充值');
    }
  } catch (error) {
    billingRechargeState.textContent = `微信充值未开始或支付入口未打开：${String(error)}`;
    billingRechargeProgress.hidden = true;
    await customAlert(billingRechargeState.textContent, '微信充值');
  } finally {
    setRechargeBusy(false);
  }
}

async function performConfirmedBillingAction(
  request: BillingActionRequest,
  title: string,
  confirmation: string,
  button: HTMLButtonElement,
) {
  if (!await customConfirm(confirmation, title)) {
    request.oldPassword = undefined;
    request.newPassword = undefined;
    clearBillingSecretInputs();
    return;
  }
  if (request.action !== 'changePassword' && !await ensureBillingRequestForeground()) return;
  const original = button.textContent;
  const wasDisabled = button.disabled;
  button.disabled = true;
  button.textContent = '处理中…';
  try {
    const result = await invoke<BillingActionResult>('perform_billing_action', {
      request,
      accountUser: selectedBillingAccountUser(),
    });
    clearBillingSecretInputs();
    if (result.passwordChanged) {
      await loadConfigFromRust();
      renderAccounts();
    } else {
      await refreshBillingCenterData();
    }
    await customAlert(result.message, '操作完成');
  } catch (error) {
    clearBillingSecretInputs();
    await customAlert(`操作失败：${String(error)}`, title);
  } finally {
    request.oldPassword = undefined;
    request.newPassword = undefined;
    request.questions = undefined;
    clearBillingSecretInputs();
    button.textContent = original;
    if (billingCenterData) {
      renderBillingService(billingCenterData);
      renderBillingDevices(billingCenterData);
      renderBillingSecurity(billingCenterData);
    } else {
      button.disabled = wasDisabled;
    }
  }
}

function normalizeBillingMac(value: string): string {
  return value.replace(/[^0-9a-f]/gi, '').toUpperCase();
}

function billingQuestionAnswers(): BillingQuestionAnswer[] | null {
  const values = billingQuestionSelects.map(select => select.value);
  const answers = [1, 2, 3].map(index =>
    (document.getElementById(`billing-answer-${index}`) as HTMLInputElement).value.trim(),
  );
  if (values.some(value => !value) || new Set(values).size !== 3) {
    void customAlert('请选择三个互不重复的密码保护问题。');
    return null;
  }
  if (answers.some(answer => answer.length < 1 || answer.length > 16)) {
    void customAlert('每个密码保护答案必须为 1–16 个字符。');
    return null;
  }
  return values.map((questionId, index) => ({ questionId, answer: answers[index] }));
}

function renderBillingCenter(info: UserInfo | null) {
  billingCenterAccount.textContent = info?.account || selectedBillingAccountUser() || '--';
  billingCenterBalance.textContent = info?.balance || '--';
  billingCenterFlow.textContent = info?.flow || '--';
  billingCenterStatus.textContent = info?.status || '--';
  billingCenterStatus.className = info?.status === '正常' ? 'success' : info?.status ? 'error' : '';

  const messages = [
    info?.billingError,
    ...(info?.billingWarnings || []),
    ...(billingCenterData?.warnings || []),
  ].filter(Boolean) as string[];
  billingCenterMessage.textContent = messages.join('\n');
  syncBillingCenterMessageVisibility();

  const mauthKnown = info?.source === 'billing' && typeof info.mauthEnabled === 'boolean';
  billingMauthBadge.className = `billing-state-badge ${mauthKnown ? (info!.mauthEnabled ? 'success' : 'warning') : 'neutral'}`;
  billingMauthBadge.textContent = mauthKnown ? (info!.mauthEnabled ? '已开启' : '已关闭') : '未知';
  btnToggleBillingMauth.disabled = !mauthKnown;
  btnToggleBillingMauth.textContent = mauthKnown ? (info!.mauthEnabled ? '关闭无感认证' : '开启无感认证') : '状态未知';

  renderBillingOnlineSessions(info?.source === 'billing' ? (info.onlineSessions || []) : []);
  renderBillingHistory(info?.source === 'billing' ? (info.loginHistory || []) : []);
  renderIcons(document.getElementById('billing-center')!);
}

async function disconnectBillingSession(button: HTMLButtonElement) {
  const sessionId = button.dataset.sessionId;
  const ip = button.dataset.ip;
  const mac = button.dataset.mac;
  if (!sessionId || !ip || mac === undefined) {
    await customAlert('在线会话信息不完整，请刷新后重试。');
    return;
  }
  const tip = billingCenterData?.overview.offlineTip
    || '注销会话会中断对应设备的校园网连接；无感认证设备可能还需要解绑 MAC。';
  const confirmed = await customConfirm(`${tip}\n\n设备 IP：${ip}\n设备 MAC：${mac || '--'}`, '确认注销在线会话');
  if (!confirmed) return;
  if (!await ensureBillingRequestForeground()) return;
  button.disabled = true;
  const originalText = button.textContent;
  button.textContent = '注销中…';
  try {
    const message = await invoke<string>('disconnect_billing_session', {
      sessionId,
      ip,
      mac,
      accountUser: selectedBillingAccountUser(),
    });
    await refreshBillingCenterData();
    await customAlert(message, '操作完成');
  } catch (error) {
    await customAlert(`注销失败：${String(error)}`);
  } finally {
    button.disabled = false;
    button.textContent = originalText;
  }
}

async function toggleBillingMauth() {
  const current = billingCenterData?.overview.mauthEnabled;
  if (typeof current !== 'boolean') {
    await customAlert('无感认证状态尚未读取，请刷新后重试。');
    return;
  }
  const enabled = !current;
  const text = enabled
    ? '确认开启无感认证吗？计费系统将按已绑定设备识别校园网会话。'
    : '关闭无感认证可能导致当前设备下线，确认继续吗？';
  if (!await customConfirm(text, enabled ? '开启无感认证' : '关闭无感认证')) return;
  if (!await ensureBillingRequestForeground()) return;
  btnToggleBillingMauth.disabled = true;
  try {
    const message = await invoke<string>('set_billing_mauth', {
      enabled,
      accountUser: selectedBillingAccountUser(),
    });
    await refreshBillingCenterData();
    await customAlert(message, '操作完成');
  } catch (error) {
    await customAlert(`修改失败：${String(error)}`);
  } finally {
    btnToggleBillingMauth.disabled = typeof billingCenterData?.overview.mauthEnabled !== 'boolean';
  }
}

async function updateUserInfo(force = false) {
  if (userInfoLoading) return;
  if (await isAppInBackground()) return;
  userInfoLoading = true;
  const requestId = ++userInfoRequestId;
  try {
    const info: UserInfo | null = await invoke('get_user_info', {
      localIp: lastKnownIp || null,
      force,
    });
    if (requestId !== userInfoRequestId) return;
    if (info) {
      infoAccount.textContent = info.account || '--';
      infoBalance.textContent = info.balance || '--';
      infoFlow.textContent = info.flow || '--';
      infoAccountLabel.textContent = '当前登录账号';
    } else {
      infoAccount.textContent = '--';
      infoBalance.textContent = '--';
      infoFlow.textContent = '--';
      infoAccountLabel.textContent = '当前登录账号';
    }
  } catch (error) {
    if (requestId !== userInfoRequestId) return;
    console.error('Failed to refresh portal user information:', error);
    infoAccount.textContent = '--';
    infoBalance.textContent = '--';
    infoFlow.textContent = '--';
    infoAccountLabel.textContent = '当前登录账号';
  } finally {
    userInfoLoading = false;
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
  renderIcons(btnLogin);
  
  const isSafe = await checkNetworkSecurity();
  if (!isSafe) {
    log('安全', '已取消登录：安全检查未通过', 'error');
    isLoggingIn = false;
    btnLogin.disabled = false;
    btnLogin.innerHTML = '<i data-lucide="log-in"></i> 立即登录';
    renderIcons(btnLogin);
    return;
  }
  
  btnLogin.innerHTML = '<i data-lucide="loader"></i> 登录中...';
  renderIcons(btnLogin);
  
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
    setTimeout(() => void updateUserInfo(true), 2000);
  } catch (error) {
    log('登录', `登录请求失败: ${String(error)}`, 'error');
    btnLogin.disabled = false;
    btnLogin.innerHTML = '<i data-lucide="log-in"></i> 立即登录';
  } finally {
    renderIcons(btnLogin);
    isLoggingIn = false;
  }
}

async function checkNetworkSecurity(): Promise<boolean> {
  if (!window.__TAURI__) return true;

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
    const whitelist = whitelistCache;
    const blacklist = blacklistCache;
    
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
        blacklistCache = [...blacklistCache, netKey];
        void syncConfigToRust();
        log('安全', `已将 ${netInfo.ssid} 加入黑名单`, 'info');
        cleanup(); resolve(false);
      };
      btnCancelBlack.addEventListener('click', onCancelBlack);
      
      const btnTrustOnce = document.getElementById('btn-sec-trust-once')!;
      const onTrustOnce = () => { cleanup(); resolve(true); };
      btnTrustOnce.addEventListener('click', onTrustOnce);
      
      const btnTrustWhite = document.getElementById('btn-sec-trust-white')!;
      const onTrustWhite = () => {
        whitelistCache = [...whitelistCache, netKey];
        void syncConfigToRust();
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
