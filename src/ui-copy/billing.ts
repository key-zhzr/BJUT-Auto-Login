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
      description: '开启后，计费系统可按已绑定设备识别校园网会话；仅适用于 bjut_wifi。',
    },
    online: {
      title: '当前在线会话',
      description: '注销会话可能会中断对应设备的校园网连接。',
    },
    history: {
      title: '近期上网记录',
      description: '时间、地址、流量与计费方式均来自计费系统；点击展开。',
    },
    records: {
      title: '账单与办理记录',
      description: '选择记录类型、日期或年份后查询；日期范围最多 60 天。',
    },
    services: {
      title: '账号服务',
      description: '报停、复通、套餐预约与消费保护。',
    },
    devices: {
      title: '设备管理',
      description: '查看、绑定或解绑用于无感认证的 MAC 地址。',
    },
    recharge: {
      title: '充值',
      description: '按支付方式、校园卡、网费账户和金额依次确认，App 会串联并核对完整充值流程。',
    },
    security: {
      title: '安全设置',
      description: '通过统一认证修改密码；成功后同步更新本 App 中该账号的安全凭据。',
    },
  } satisfies Record<string, BillingPanelCopy>,
  serviceActions: {
    stopReopen: {
      title: '停复机',
      description: '写操作仅在确认后发送；立即操作可能中断或恢复校园网计费。',
    },
    consumeLimit: {
      title: '消费保护',
      description: '输入非负金额，最多两位小数；999999 表示不限制。',
    },
    package: {
      title: '预约套餐',
      description: '下方会直接标记当前周期与下一周期套餐；未预约时，下一周期继续使用当前套餐。',
    },
  },
  recharge: {
    hoursTitle: '充值服务开放时间：每日 06:00–23:00',
    hoursDescription: '开放时间以北京时间为准；其他时段可查看余额，但不能创建或确认充值订单。',
    initialState: '填写目标学工号和金额后，先核对账户再确认扣费。',
    initialMethodDescription: '将先核对校园卡余额和目标账户，二次确认后再执行一次扣费。',
    safetyNote: '到账状态可自动查询，但任何扣费或网费转入订单只提交一次，结果不明确时不会自动重复扣费。支付宝与微信均由学校支付平台发起，App 不读取支付密码。',
  },
  security: {
    passwordTitle: '修改统一认证密码',
    passwordPolicy: '密码要求：12–16 位、大写字母、小写字母、数字、特殊字符 !@#$%^&*()',
    questionsTitle: '密码保护',
    questionsDescription: '三个问题不能重复；密码和答案仅用于本次 HTTPS 请求，不会写入日志。',
  },
} as const;
