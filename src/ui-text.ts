import { BILLING_COPY } from './ui-copy/billing';
import { SETTINGS_COPY, type SettingItemCopy } from './ui-copy/settings';

export { BILLING_COPY, SETTINGS_COPY };

/**
 * Centralized frontend copy for dynamic or shared UI areas.
 *
 * Settings and billing copy live in `src/ui-copy/` and are applied before the
 * custom controls are initialized. `index.html` keeps matching fallback text
 * so the first frame is still complete if JavaScript has not loaded yet.
 */
export const UI_TEXT = {
  networkStatus: {
    checkingTitle: '正在检测网络…',
    checkingDetail: '正在检查互联网连通性与校园认证网关',
    onlineTitle: '互联网已连接',
    onlineDetail: '网络畅通无阻',
    campusTitle: '检测到校园网',
    campusIncompleteTitle: '校园网登录未完成',
    offlineTitle: '网络断开或非校园网',
    offlineDetail: '无法访问互联网和校园网登录页',
  },
  accountDiscovery: {
    title: '发现当前校园网账号',
    importDescription: (user: string) =>
      `计费系统识别到当前登录账号 ${user}，该账号尚未加入 App。可以直接保存到安全凭据存储。`,
    passwordDescription: (user: string) =>
      `计费系统识别到当前登录账号 ${user}，但学校页面只返回了临时密码字段。请手动补录密码。`,
    importAction: '保存此账号',
    passwordAction: '补充密码',
    dismissAction: '本次忽略',
  },
  networkTrust: {
    whitelistTitle: '信任的 Wi-Fi（白名单）',
    blacklistTitle: '拒绝的 Wi-Fi（黑名单）',
    whitelistDescription:
      '白名单 Wi-Fi 可用于明确授权自动登录。每条记录同时匹配 SSID 与 BSSID。',
    blacklistDescription:
      '黑名单优先于白名单；匹配后不会向该 Wi-Fi 发送校园网账号密码。',
  },
} as const;

function setText(root: ParentNode, selector: string, value: string): void {
  const element = root.querySelector<HTMLElement>(selector);
  if (element) element.textContent = value;
}

function setSettingItem(root: ParentNode, controlId: string, copy: SettingItemCopy): void {
  const item = root.querySelector<HTMLElement>(`#${controlId}`)?.closest<HTMLElement>('.setting-item');
  const info = item?.querySelector<HTMLElement>('.setting-info');
  const title = info?.querySelector<HTMLElement>('h4');
  const description = info?.querySelector<HTMLElement>('p');
  if (title) title.textContent = copy.title;
  if (description) description.textContent = copy.description;
}

function setSettingsGroupTitle(root: ParentNode, anchorId: string, value: string): void {
  const group = root.querySelector<HTMLElement>(`#${anchorId}`)?.closest<HTMLElement>('.settings-group');
  const title = group?.querySelector<HTMLElement>(':scope > h3');
  if (title) title.textContent = value;
}

function setBillingPanel(
  root: ParentNode,
  anchorSelector: string,
  copy: { title: string; description: string },
): void {
  const panel = root.querySelector<HTMLElement>(anchorSelector)?.closest<HTMLElement>('.billing-center-panel');
  if (!panel) return;
  const header = panel.querySelector<HTMLElement>('.billing-center-panel-header');
  const title = header?.querySelector<HTMLElement>('.billing-panel-title h2');
  const description = header?.querySelector<HTMLElement>('.billing-panel-title + p')
    ?? header?.querySelector<HTMLElement>('.billing-panel-title')?.parentElement?.querySelector<HTMLElement>(':scope > p');
  if (title) title.textContent = copy.title;
  if (description) description.textContent = copy.description;
}

function setBillingAction(
  root: ParentNode,
  anchorId: string,
  copy: { title: string; description: string },
): void {
  const group = root.querySelector<HTMLElement>(`#${anchorId}`)?.closest<HTMLElement>('.billing-action-group');
  setText(group ?? root, '.billing-action-copy h3', copy.title);
  setText(group ?? root, '.billing-action-copy p', copy.description);
}

/** Apply user-editable page copy before controls copy their initial labels. */
export function applyConfiguredUiText(root: ParentNode = document): void {
  setText(root, '#settings > .page-header h1', SETTINGS_COPY.pageTitle);
  setText(root, '#settings > .page-header p', SETTINGS_COPY.pageDescription);

  setSettingsGroupTitle(root, 'setting-theme', SETTINGS_COPY.groups.appearance);
  setSettingsGroupTitle(root, 'setting-auto-login', SETTINGS_COPY.groups.automation);
  setSettingsGroupTitle(root, 'setting-vpn-compatibility', SETTINGS_COPY.groups.networkSecurity);
  setSettingsGroupTitle(root, 'setting-android-notification-mode', SETTINGS_COPY.groups.androidNotifications);
  setSettingsGroupTitle(root, 'setting-usage-alerts', SETTINGS_COPY.groups.usage);
  setSettingsGroupTitle(root, 'setting-more-options', SETTINGS_COPY.groups.advanced);
  setText(root, '#permission-health-group .setting-group-heading h3', SETTINGS_COPY.groups.permissions.title);
  setText(root, '#permission-health-group .setting-group-heading p', SETTINGS_COPY.groups.permissions.description);
  const profileGroup = root.querySelector<HTMLElement>('#btn-add-network-profile')?.closest<HTMLElement>('.settings-group');
  setText(profileGroup ?? root, '.setting-group-heading h3', SETTINGS_COPY.groups.profiles.title);
  setText(profileGroup ?? root, '.setting-group-heading p', SETTINGS_COPY.groups.profiles.description);
  Object.entries(SETTINGS_COPY.items).forEach(([controlId, copy]) => setSettingItem(root, controlId, copy));

  setText(root, '#settings-about .settings-about-eyebrow', SETTINGS_COPY.about.eyebrow);
  setText(root, '#settings-about .settings-about-header h3', SETTINGS_COPY.about.title);
  setText(root, '#settings-about .settings-about-header h3 + p', SETTINGS_COPY.about.description);
  setText(root, '#settings-about .settings-about-meta > div:nth-child(2) strong', SETTINGS_COPY.about.privacy);
  setText(root, '#settings-about .settings-about-meta > div:nth-child(3) strong', SETTINGS_COPY.about.nature);
  setText(root, '#settings-about .settings-about-note', SETTINGS_COPY.about.note);

  setText(root, '#billing-center .billing-center-heading h1', BILLING_COPY.pageTitle);
  setText(root, '#billing-center-subtitle', BILLING_COPY.pageDescription);
  setText(root, '#billing-center .billing-account-picker > span', BILLING_COPY.accountPicker);
  Object.entries(BILLING_COPY.nav).forEach(([section, label]) => {
    root.querySelectorAll<HTMLElement>(`[data-billing-section-target="${section}"] span`).forEach(element => {
      element.textContent = label;
    });
  });
  root.querySelectorAll<HTMLElement>('#billing-center .billing-center-summary .billing-summary-card > span')
    .forEach((element, index) => {
      const label = BILLING_COPY.summary[index];
      if (label) element.textContent = label;
    });

  const panelAnchors: Array<[string, keyof typeof BILLING_COPY.panels]> = [
    ['#billing-mauth-badge', 'mauth'],
    ['#billing-online-count', 'online'],
    ['#billing-history-panel', 'history'],
    ['#billing-records-panel', 'records'],
    ['#billing-services-panel', 'services'],
    ['#billing-devices-panel', 'devices'],
    ['#billing-recharge-panel', 'recharge'],
    ['#billing-security-panel', 'security'],
  ];
  panelAnchors.forEach(([selector, key]) => setBillingPanel(root, selector, BILLING_COPY.panels[key]));
  setBillingAction(root, 'btn-billing-stop-now', BILLING_COPY.serviceActions.stopReopen);
  setBillingAction(root, 'btn-billing-consume-limit', BILLING_COPY.serviceActions.consumeLimit);
  setBillingAction(root, 'btn-billing-package', BILLING_COPY.serviceActions.package);

  setText(root, '#billing-recharge-panel .billing-recharge-hours strong', BILLING_COPY.recharge.hoursTitle);
  setText(root, '#billing-recharge-panel .billing-recharge-hours span', BILLING_COPY.recharge.hoursDescription);
  setText(root, '#billing-recharge-state', BILLING_COPY.recharge.initialState);
  setText(root, '#billing-recharge-method-description', BILLING_COPY.recharge.initialMethodDescription);
  const rechargePanel = root.querySelector<HTMLElement>('#billing-recharge-panel .billing-payment-panel');
  const rechargeNotes = rechargePanel?.querySelectorAll<HTMLElement>(':scope > .billing-inline-note');
  const safetyNote = rechargeNotes?.item(Math.max(0, (rechargeNotes?.length ?? 1) - 1));
  if (safetyNote) safetyNote.textContent = BILLING_COPY.recharge.safetyNote;

  setText(root, '#billing-password-form > h3', BILLING_COPY.security.passwordTitle);
  setText(root, '#billing-password-policy', BILLING_COPY.security.passwordPolicy);
  setText(root, '#billing-questions-form > h3', BILLING_COPY.security.questionsTitle);
  setText(root, '#billing-questions-form > .billing-inline-note', BILLING_COPY.security.questionsDescription);
}
