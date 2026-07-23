export interface AccountView {
  user: string;
  pass: string;
  hasPassword: boolean;
  isDefault: boolean;
  isDisabled: boolean;
}

export type LegacyAccount = Omit<AccountView, 'hasPassword'>;

export interface NetworkProfile {
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

export interface BackendConfig {
  accounts?: Partial<AccountView>[];
  auto_login: boolean;
  check_interval: number;
  check_interval_bg: number;
  wifi_change_detect: boolean;
  log_level: string;
  vpn_compatibility?: string;
  vpn_maximum_until?: number | null;
  whitelist?: string[];
  blacklist?: string[];
  network_profiles?: NetworkProfile[];
  usage_alerts?: boolean;
  balance_alert_threshold?: number;
  flow_alert_threshold?: number;
  android_notification_mode?: string;
  android_notify_network_status?: boolean;
  android_notify_login_results?: boolean;
  android_notify_background_errors?: boolean;
}

export interface RechargePreview {
  confirmationId: string;
  payerAccount: string;
  cardBalance: string;
  targetAccount: string;
  targetBalance: string;
  targetStatus: string;
  amount: string;
  allowedTime: string;
  expiresInSeconds: number;
}

export interface RechargeResult {
  message: string;
  targetAccount: string;
  amount: string;
}

export interface RechargeBalanceSnapshot {
  payerAccount: string;
  cardBalance: string;
  targetAccount: string;
  targetBalance: string;
  targetStatus: string;
}

export interface AlipayRechargePreview {
  confirmationId: string;
  payerAccount: string;
  cardBalance: string;
  amount: string;
  expiresInSeconds: number;
}

export interface AlipayRechargeResult {
  message: string;
  payerAccount: string;
  amount: string;
  paymentUrl: string;
}

export interface ActiveAlipayPayment {
  recoveryId: string;
  paymentUrl: string;
  payerAccount: string;
  amount: string;
  targetAccount: string;
  cardBalanceBefore: string;
}

export interface WechatRechargePreview {
  confirmationId: string;
  payerAccount: string;
  cardBalance: string;
  targetAccount: string;
  targetBalance: string;
  targetStatus: string;
  amount: string;
  allowedTime: string;
  expiresInSeconds: number;
}

export interface WechatRechargeResult {
  message: string;
  paymentId: string;
  payerAccount: string;
  targetAccount: string;
  amount: string;
  launchUrl: string;
}

export interface WechatPaymentStatus {
  status: 'pending' | 'paid';
  message: string;
}

export interface ActiveWechatPayment {
  paymentId: string;
  launchUrl: string;
  payerAccount: string;
  amount: string;
  targetAccount: string;
  cardBalanceBefore: string;
}

export interface RecoverableRecharge {
  id: string;
  method: 'campusCard' | 'alipay' | 'wechat';
  payerAccount: string;
  targetAccount: string;
  amount: string;
  stage: 'prepared' | 'transferSubmitted' | 'orderCreated' | 'handedOff' | 'paymentConfirmed' | 'completed' | 'unknown' | 'cancelled';
  cardBalanceBefore: string;
  paymentUrl: string;
  paymentId: string;
  note: string;
}

export interface DiscoveredCampusAccount {
  user: string;
  token: string;
}

export interface GitHubReleaseAsset {
  name: string;
  browser_download_url: string;
  size: number;
}

export interface GitHubRelease {
  tag_name: string;
  name: string | null;
  body: string | null;
  html_url: string;
  prerelease: boolean;
  draft: boolean;
  assets: GitHubReleaseAsset[];
}

export interface UpdateTarget {
  platform: 'android' | 'ios' | 'windows' | 'macos' | 'linux';
  arch: string;
  format: string;
  currentVersion: string;
}

export interface UpdateProgress {
  status: 'downloading' | 'installing';
  received?: number;
  total?: number;
  percent?: number | null;
}

export interface AccountHealth {
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

export interface CredentialStorageHealth {
  status: string;
  backend: string;
  persistent: boolean;
  savedAccounts: number;
  missingPasswordAccounts: string[];
  message: string;
}

export interface DiagnosticStep {
  id: string;
  label: string;
  status: 'success' | 'warning' | 'error' | 'skipped';
  message: string;
  durationMs: number;
}

export interface DiagnosticReport {
  createdAt: string;
  overall: 'healthy' | 'auth_required' | 'no_network' | 'offline';
  summary: string;
  ssid: string;
  ip: string;
  steps: DiagnosticStep[];
}

export interface AppLogEntry {
  time: string;
  module: string;
  message: string;
  type: 'info' | 'error' | 'success' | 'debug';
}

export interface CountdownPayload {
  status: 'checking' | 'suspended' | 'ticking';
  seconds: number;
}

export interface NetworkStatePayload {
  state: 'Online' | 'BjutCampus' | 'Offline';
  loginType?: string;
  ssid?: string;
  bssid?: string;
  ip?: string;
  timestamp?: string;
}

export interface BillingLoginRecord {
  loginAt: string;
  logoutAt: string;
  ip: string;
  ipv6: string;
  mac: string;
  durationMinutes: string;
  usedFlowMb: string;
  billingMode: string;
  amount: string;
}

export interface BillingOnlineSession {
  loginAt: string;
  ip: string;
  ipv6: string;
  mac: string;
  durationMinutes: string;
  usedFlowMb: string;
  sessionId: string;
}

export interface UserInfo {
  account: string;
  balance: string;
  flow: string;
  source: 'billing' | 'portal' | 'unavailable';
  status?: string | null;
  statusReason?: string | null;
  package?: string | null;
  packageDetail?: string | null;
  usedFlow?: string | null;
  billingCycle?: string | null;
  updatedAt: string;
  billingError?: string | null;
  loginHistory: BillingLoginRecord[];
  onlineSessions: BillingOnlineSession[];
  offlineTip?: string | null;
  mauthEnabled?: boolean | null;
  billingWarnings: string[];
}

export interface BillingOverview {
  account: string;
  balance: string;
  remainingFlow: string;
  usedFlow?: string | null;
  status?: string | null;
  statusReason?: string | null;
  package?: string | null;
  packageDetail?: string | null;
  billingCycle?: string | null;
  updatedAt: string;
  loginHistory: BillingLoginRecord[];
  onlineSessions: BillingOnlineSession[];
  offlineTip?: string | null;
  mauthEnabled?: boolean | null;
  warnings: string[];
}

export interface BillingTable {
  total: number;
  rows: Record<string, string>[];
  summary: Record<string, string>;
}

export interface BillingPackageOption {
  id: string;
  name: string;
  description: string;
}

export interface BillingPasswordPolicy {
  minLength: number;
  maxLength: number;
  requireUppercase: boolean;
  requireLowercase: boolean;
  requireDigit: boolean;
  requireSpecial: boolean;
}

export interface BillingSecurityQuestion {
  id: string;
  text: string;
}

export interface BillingServiceState {
  accountStatus?: string | null;
  statusReason?: string | null;
  currentPackageId?: string | null;
  currentPackage?: string | null;
  packageDetail?: string | null;
  nextSettlementDate?: string | null;
  canStopNow: boolean;
  canReopenNow: boolean;
  packageScheduled: boolean;
  scheduledPackageId?: string | null;
  scheduledPackage?: string | null;
  consumeLimit?: string | null;
  currentCycleSpend?: string | null;
  balance?: string | null;
  packageOptions: BillingPackageOption[];
}

export interface BillingCenterData {
  account: string;
  overview: BillingOverview;
  fetchedAt: string;
  queryStartDate: string;
  queryEndDate: string;
  queryYear: string;
  usageRecords: BillingTable;
  monthlyBills: BillingTable;
  payments: BillingTable;
  operations: BillingTable;
  stopLogs: BillingTable;
  reopenLogs: BillingTable;
  packageLogs: BillingTable;
  devices: BillingTable;
  tariffGroups: BillingTable;
  service: BillingServiceState;
  passwordPolicy: BillingPasswordPolicy;
  securityQuestions: BillingSecurityQuestion[];
  rechargeAvailable: boolean;
  warnings: string[];
}

export interface BillingQuestionAnswer {
  questionId: string;
  answer: string;
}

export interface BillingActionRequest {
  action: string;
  packageId?: string;
  consumeLimit?: string;
  mac?: string;
  oldPassword?: string;
  newPassword?: string;
  questions?: BillingQuestionAnswer[];
}

export interface BillingActionResult {
  message: string;
  passwordChanged: boolean;
}

export type BillingRecordKind =
  | 'usage'
  | 'monthly'
  | 'payments'
  | 'operations'
  | 'stopLogs'
  | 'reopenLogs'
  | 'packageLogs';

export interface BillingRecordQuery {
  kind: BillingRecordKind;
  page: number;
  pageSize: number;
  startDate?: string;
  endDate?: string;
  year?: string;
  all?: boolean;
}

export interface BillingRecordResult {
  kind: BillingRecordKind;
  page: number;
  pageSize: number;
  startDate?: string | null;
  endDate?: string | null;
  year?: string | null;
  all: boolean;
  table: BillingTable;
}

export interface BillingRecordQueryState {
  page: number;
  pageSize: number;
  startDate: string;
  endDate: string;
  year: string;
  queried: boolean;
}
