export interface SettingItemCopy {
  title: string;
  description: string;
}

/** 设置页文案：修改这里即可覆盖对应控件旁的标题与说明。 */
export const SETTINGS_COPY = {
  pageTitle: '设置',
  pageDescription: '外观、自动登录与网络偏好',
  groups: {
    appearance: '外观',
    interface: '界面与托盘',
    automation: '自动化设置',
    networkSecurity: '网络与安全',
    androidNotifications: 'Android 通知',
    permissions: {
      title: '权限健康中心',
      description: '查看并修复网络识别、后台保活、通知和更新所需权限',
    },
    profiles: {
      title: '网络配置档案',
      description: '按 Wi-Fi 名称或有线环境单独指定账号顺序、协议和检测间隔',
    },
    usage: '流量与余额提醒',
    trafficStats: '设备流量统计',
    advanced: '数据与维护',
  },
  items: {
    'setting-theme': {
      title: '界面主题',
      description: 'Basic 采用液态玻璃导航与覆盖式菜单；WinUI 采用 Windows 11 原生导航与材质。',
    },
    'setting-color-mode': {
      title: '明暗模式',
      description: '跟随系统深浅色外观自动切换。',
    },
    'setting-accent-color': {
      title: '强调色',
      description: '用于高亮当前导航项、主按钮和焦点状态。',
    },
    'setting-auto-login': {
      title: '后台自动登录',
      description: '检测到校园网需要认证时自动登录',
    },
    'setting-wifi-change-detect': {
      title: 'Wi-Fi 变更检测',
      description: '监听系统网络变化并立即检测，同时以局域网 IP 轮询兜底',
    },
    'setting-autostart': {
      title: '开机自启动',
      description: '开机后在后台自动启动（仅桌面端）',
    },
    'setting-check-interval': {
      title: '前台自动检测间隔',
      description: '窗口打开时的检测周期（秒）',
    },
    'setting-check-interval-bg': {
      title: '后台自动检测间隔',
      description: '后台静默运行时的检测周期（秒）',
    },
    'setting-vpn-compatibility': {
      title: 'VPN 共存兼容等级',
      description: '推荐“高兼容”：保持 HTTPS 加密并将校园域名解析到固定网关地址',
    },
    'btn-manage-whitelist': {
      title: '受信任的 Wi-Fi（白名单）',
      description: '连接列表中的 Wi-Fi 时跳过安全提示',
    },
    'btn-manage-blacklist': {
      title: '不信任的 Wi-Fi（黑名单）',
      description: '连接列表中的 Wi-Fi 时禁止自动和手动登录',
    },
    'setting-android-notification-mode': {
      title: '通知方式',
      description: '常驻通知同步显示后台检测与自动登录状态',
    },
    'setting-android-notify-network-status': {
      title: '网络状态',
      description: '通知联网、断网、移动数据与校园网待认证状态',
    },
    'setting-android-notify-login-results': {
      title: '自动登录结果',
      description: '通知登录成功、失败原因或安全拦截情况',
    },
    'setting-android-notify-usage-alerts': {
      title: '余额与流量提醒',
      description: '余额或流量低于阈值时提醒，月末自动核对下月套餐费用',
    },
    'setting-android-notify-background-errors': {
      title: '后台异常',
      description: '后台检测服务异常退出或中断时通知',
    },
    'setting-usage-alerts': {
      title: '启用用量提醒',
      description: '余额或流量不足时发出提醒，月末核对下月套餐费用',
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
      description: '在控制台显示手动选择账号和认证方式的入口',
    },
    'setting-macos-dock': {
      title: '在程序坞显示图标',
      description: '关闭后仅在顶部菜单栏保留图标，不在程序坞占位',
    },
    'setting-update-channel': {
      title: '更新通道',
      description: '选择接收正式版或测试版更新',
    },
    'setting-log-level': {
      title: '日志详细等级',
      description: '设置“运行日志”页记录的日志级别',
    },
    'btn-export-config': {
      title: '配置导入/导出',
      description: '通过剪贴板或加密二维码备份并迁移设置和账号',
    },
    'btn-quit-app': {
      title: '退出应用',
      description: '完全退出并停止后台自动检测服务',
    },
  } satisfies Record<string, SettingItemCopy>,
  about: {
    eyebrow: '关于',
    title: 'BJUT Auto Login',
    description: '开源的北京工业大学校园网连接、诊断与计费助手。',
    privacy: '账号密码仅加密保存在本机，不上传第三方服务器',
    nature: '开源第三方工具，非学校官方出品',
    note: '仅连接校园网认证页、计费系统、统一认证、学校服务、GitHub 更新与你配置的路由器；密码等敏感信息不会写入日志。',
  },
} as const;
