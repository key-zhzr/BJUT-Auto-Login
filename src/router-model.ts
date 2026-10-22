export interface RouterUplink {
  id: string; network?: string; network6?: string; device?: string; device6?: string;
  ipv4?: string; ipv6?: string; ssid?:string;bssid?:string;state: string; message?: string; internet4?: string;
  internet6?: string; account?: string; checking?: number; check_message?: string;
  updated_at?: number; route4?: string; route6?: string; session?: string; transport?: string;
  age_seconds?: number; stale?: number; valid_for?: number;
  operation?: {id?:string;action?:string;state?:string;phase?:string;duration?:number|string;deadline?:number|string};
}
export interface RouterPort { uplink: string; device?: string; linkUp?: number; speedMbps?: string; rxBytes?: string; txBytes?: string; }
export interface RouterAccount { id: string; user: string; enabled: number; hasPassword: number; loginType?: string; maxSessions?:number;onlineCount?:number; }
export interface RouterSnapshot { version: string; uplinks: RouterUplink[]; accounts: RouterAccount[]; managementUrl: string; capabilities?: string[]; ports?: RouterPort[]; }
export interface RouterCandidate { name: string; address: string; version: string; secure: boolean; }
export interface RouterBillingItem { state: string; message?: string; percent?: number; account?: string; balance?: string; remainingFlow?: string; totalFlow?: string; usedFlow?: string; accountStatus?: string; package?: string; image?: string; updatedAt?: number; }
export interface RouterBillingJob { uplink: string; account: string; ticket: string; running: boolean; starting: boolean; item: RouterBillingItem; sessions?: Record<string,string>[]; sessionsAvailable?: boolean; }

export const routerLabels: Record<string, string> = {
  logged_out:'已注销（自动登录已暂停）',logout_failed:'注销未完成',
  online:'互联网已连接', busy:'正在检测', authenticated_limited:'已认证，探测受限', probe_uncertain:'连通性待确认',
  offline_suspected:'正在复核', waiting_interface:'等待接口', cooldown:'等待重试', auth_rejected:'账号未通过',
  portal_not_found:'未发现认证网关', portal_not_ready:'等待认证网关', no_account:'请选择账号', stopped:'尚未运行',
  login_accepted:'登录已受理', blocked:'请检查上游', request_failed:'认证结果待确认',
  account_busy:'账号使用中',
};

export function routerStatus(item:RouterUplink, disconnected=false, elapsed=0):string {
  if(disconnected || item.stale===1 || (item.valid_for && Number(item.age_seconds || 0)+elapsed>item.valid_for))return '数据已过期';
  return routerLabels[item.state] || '状态待确认';
}
export function routerAdvice(item:RouterUplink):string {
  if(item.stale)return '检测结果已过期，请核对路由器服务是否运行。';
  if(item.route4==='failed' || item.route6==='failed' || item.route4==='missing_default')return '上游出口未就绪，请检查接口网关和策略路由。';
  if(item.state==='auth_rejected')return '账号被网关拒绝，请核对密码、余额及账号状态。';
  if(item.state==='account_busy')return '适用账号已占用，请选择备用账号或核对并发上限。';
  if(item.state==='waiting_interface')return '上游未取得地址，请检查网线或 Wi-Fi 连接。';
  if(item.session==='online' && item.internet4!=='reachable' && item.internet6!=='reachable')return '认证会话有效，外网探测失败；请检查 DNS、路由及探测目标。';
  if(item.internet4==='reachable' && item.internet6==='unreachable')return 'IPv4 可用，IPv6 未通过；请检查 IPv6 地址、默认路由及校园网双栈认证。';
  return item.state==='online'?'此上游连接正常。':'请刷新检测结果，结合本机与其他上游定位问题。';
}

export function routerCounter(value?: string): bigint {
  return value && /^\d{1,20}$/.test(value) ? BigInt(value) : 0n;
}
export function routerBytes(value: bigint | number): string {
  let n = Math.max(0, Number(value)), unit = 0;
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB', 'EiB'];
  while (n >= 1024 && unit < units.length - 1) { n /= 1024; unit++; }
  return `${n.toFixed(unit > 1 ? 1 : 0)} ${units[unit]}`;
}
export function routerBillingSummary(jobs: RouterBillingJob[]): { accounts: number; balance: number | null; totalFlow: number | null; remainingFlow: number | null } {
  const unique = new Map<string, RouterBillingItem>();
  for (const {item} of jobs) {
    if (item.account && ['loading','ready'].includes(item.state)) {
      const previous = unique.get(item.account);
      if (!previous || (item.updatedAt || 0) >= (previous.updatedAt || 0)) unique.set(item.account, item);
    }
  }
  const balances = [...unique.values()].map(item => item.balance?.trim() ? Number(item.balance) : NaN);
  const sum = (key:'totalFlow'|'remainingFlow') => {
    const values=[...unique.values()].map(item=>item[key]?.trim()?Number(item[key]):NaN);
    return values.length && values.every(value=>Number.isFinite(value) && value>=0)?values.reduce((a,b)=>a+b,0):null;
  };
  return {accounts: unique.size, balance: balances.length && balances.every(Number.isFinite) ? balances.reduce((a,b)=>a+b,0) : null,totalFlow:sum('totalFlow'),remainingFlow:sum('remainingFlow')};
}
