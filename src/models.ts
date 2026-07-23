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
}
