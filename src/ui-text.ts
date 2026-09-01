/**
 * Centralized frontend copy for newly split or frequently edited UI areas.
 *
 * Static headings, labels and input placeholders remain in `index.html`;
 * legacy feature-specific dynamic strings are still beside their handlers in
 * `main.ts` and the smaller frontend modules.
 * Protocol/network errors emitted by Rust remain beside their implementation
 * under `src-tauri/src/` so translations cannot accidentally weaken a safety
 * decision. See `docs/ui-text-guide.md` for the complete editing map.
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
    importAction: '保存到账号列表',
    passwordAction: '补录账号密码',
    dismissAction: '本次忽略',
  },
  networkTrust: {
    whitelistTitle: '信任的 Wi-Fi（白名单）',
    blacklistTitle: '拒绝的 Wi-Fi（黑名单）',
    whitelistDescription:
      '白名单 Wi-Fi 可用于明确授权自动登录。每条记录同时匹配 SSID 与 BSSID，避免同名热点冒充。',
    blacklistDescription:
      '黑名单优先于白名单；匹配后不会向该 Wi-Fi 发送校园网账号密码。',
  },
} as const;
