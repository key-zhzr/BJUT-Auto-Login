export interface BillingPanelCopy {
  title: string;
  description: string;
}

/** 计费中心文案：页面标题、模块说明与充值说明集中在这里。 */
export const BILLING_COPY = {
  pageTitle: '校园网计费中心',
  pageDescription: '账户状态、在线会话与近期上网记录',
  accountPicker: '计费账号',
  nav: {
    overview: '概览',
    records: '用量与账单',
    services: '账户服务',
    recharge: '充值',
    devices: '设备与安全',
  },
  summary: ['计费账号', '账户余额', '剩余流量', '账号状态'],
  panels: {
    mauth: {
      title: '无感认证',
      description: '开启后，已绑定 MAC 的设备连接 bjut_wifi 可自动免密上网。',
    },
    online: {
      title: '当前在线会话',
      description: '下线会话会断开对应设备的校园网连接。',
    },
    history: {
      title: '近期上网记录',
      description: '点击展开最近的上网时间、IP、流量与计费方式。',
    },
    records: {
      title: '账单与办理记录',
      description: '选择记录类型和日期范围后查询。',
    },
    services: {
      title: '账号服务',
      description: '办理停机保号、复通、下月套餐预约和消费限额。',
    },
    devices: {
      title: '设备管理',
      description: '绑定或解绑用于无感认证的 MAC 地址。',
    },
    recharge: {
      title: '充值',
      description: '使用校园卡、支付宝或微信为自己或他人充网费。',
    },
    security: {
      title: '安全设置',
      description: '修改统一认证密码，并同步更新本机保存的密码。',
    },
  } satisfies Record<string, BillingPanelCopy>,
  serviceActions: {
    stopReopen: {
      title: '停复机',
      description: '暂停或恢复校园网服务。',
    },
    consumeLimit: {
      title: '消费保护',
      description: '填入非负金额（最多两位小数），填 999999 表示不限制。',
    },
    package: {
      title: '预约套餐',
      description: '当前套餐和已预约的下周期套餐会在下方标出；未预约时下月沿用原套餐。',
    },
  },
  recharge: {
    hoursTitle: '充值服务开放时间：每日 06:00–23:00',
    hoursDescription: '开放时段按北京时间显示，其余时段也可尝试发起订单。',
    initialState: '填好目标学工号和金额后，先核对账户信息再确认扣费。',
    initialMethodDescription: '先查询校园卡余额与目标账户状态，二次确认后才会提交扣款。',
    safetyNote: '扣费和网费转入订单不会自动重复提交，避免重复扣款；支付宝与微信由学校支付平台处理，App 不读取支付密码。',
  },
  security: {
    passwordTitle: '修改统一认证密码',
    passwordPolicy: '密码要求：12–16 位、大写字母、小写字母、数字、特殊字符 !@#$%^&*()',
    questionsTitle: '密码保护',
    questionsDescription: '三个问题不能重复；密码和答案仅用于本次 HTTPS 请求，不会写入日志。',
  },
} as const;
