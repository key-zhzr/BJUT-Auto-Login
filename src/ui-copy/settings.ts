export interface SettingItemCopy {
  title: string;
  description: string;
}

/** 设置页文案：修改这里即可覆盖对应控件旁的标题与说明。 */
export const SETTINGS_COPY = {
  pageTitle: '设置',
  pageDescription: '自定义应用程序行为',
  groups: {
    appearance: '外观',
    automation: '自动化设置',
    networkSecurity: '网络与安全',
    androidNotifications: 'Android 通知',
    permissions: {
      title: '权限健康中心',
      description: '集中检查网络识别、后台运行、通知与应用更新所需权限',
    },
    profiles: {
      title: '网络配置档案',
      description: '按 SSID 或有线环境绑定账号顺序、协议和检测策略',
    },
    usage: '流量与余额提醒',
    advanced: '高级设置',
  },
  items: {
    'setting-theme': {
      title: '界面主题',
      description: 'Basic 保留项目视觉；Apple OS 27 与 WinUI 分别遵循 Liquid Glass 和 Fluent 材质层级。',
    },
    'setting-color-mode': {
      title: '明暗模式',
      description: '跟随系统会实时响应设备的深色或浅色外观变化。',
    },
    'setting-accent-color': {
      title: '强调色',
      description: '应用于活动栏目、主按钮、进度与焦点状态。',
    },
    'setting-auto-login': {
      title: '后台自动登录',
      description: '当检测到校园网断开时自动进行登录',
    },
    'setting-wifi-change-detect': {
      title: 'Wi-Fi 变更检测',
      description: '系统网络事件触发即时检测，并以局域网 IP 检测作为后台兜底',
    },
    'setting-autostart': {
      title: '开机自启动',
      description: '在系统启动时自动后台运行应用（仅桌面端）',
    },
    'setting-check-interval': {
      title: '前台自动检测间隔',
      description: '应用在屏幕显示时的检测频率（秒）',
    },
    'setting-check-interval-bg': {
      title: '后台自动检测间隔',
      description: '应用在后台静默运行时的检测频率（秒）',
    },
    'setting-vpn-compatibility': {
      title: 'VPN 共存兼容等级',
      description: '推荐“高兼容”：保留 HTTPS 校验，并将校园网域名固定到已知网关地址',
    },
    'btn-manage-whitelist': {
      title: '受信任的 Wi-Fi（白名单）',
      description: '在此列表中的网络将跳过安全提示',
    },
    'btn-manage-blacklist': {
      title: '不信任的 Wi-Fi（黑名单）',
      description: '在此列表中的网络将被拒绝登录',
    },
    'setting-android-notification-mode': {
      title: '通知方式',
      description: '常驻通知同时显示后台检测与自动登录状态',
    },
    'setting-android-notify-network-status': {
      title: '网络状态',
      description: '联网、离线、移动数据及校园网待认证状态',
    },
    'setting-android-notify-login-results': {
      title: '自动登录结果',
      description: '登录成功、失败、安全阻止及没有可用账号',
    },
    'setting-android-notify-usage-alerts': {
      title: '余额与流量提醒',
      description: '余额或剩余套餐流量低于提醒线',
    },
    'setting-android-notify-background-errors': {
      title: '后台异常',
      description: '无界面检测核心或后台服务运行失败',
    },
    'setting-usage-alerts': {
      title: '启用用量提醒',
      description: '余额或剩余套餐流量低于提醒线时每天通知一次',
    },
    'setting-balance-threshold': {
      title: '余额提醒线',
      description: '单位：元',
    },
    'setting-flow-threshold': {
      title: '流量提醒线',
      description: '单位：GB',
    },
    'setting-more-options': {
      title: '更多控制台选项',
      description: '在控制台显示指定的账号和登录方式组件',
    },
    'setting-macos-dock': {
      title: '在程序坞显示图标',
      description: '关闭后，App 将只在右上角菜单栏显示，不占用程序坞',
    },
    'setting-update-channel': {
      title: '更新通道',
      description: '选择希望接收的更新版本类型',
    },
    'setting-log-level': {
      title: '日志详细等级',
      description: '控制“运行日志”页面记录的日志级别',
    },
    'btn-export-config': {
      title: '配置导入/导出',
      description: '在 Rust 中加密完整配置与账号密码，并在导入后回读校验安全存储',
    },
    'btn-quit-app': {
      title: '退出应用',
      description: '彻底关闭应用并停止后台网络检测与保活服务',
    },
  } satisfies Record<string, SettingItemCopy>,
  about: {
    eyebrow: '关于',
    title: 'BJUT Auto Login',
    description: '面向北京工业大学校园网的本地自动登录、网络诊断与计费服务助手。',
    privacy: '账号凭据仅保存在本机安全存储',
    nature: '开源第三方工具，非学校官方应用',
    note: '应用只会连接校园网登录、计费、统一认证、校园服务与 GitHub Releases 等明确用途的服务；敏感字段不会写入运行日志。',
  },
} as const;
