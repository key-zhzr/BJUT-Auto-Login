import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { emit } from '@tauri-apps/api/event';
// Load the current app markup so this regression page cannot drift from it.
const appDocument = new DOMParser().parseFromString(await (await fetch('/index.html')).text(), 'text/html');
const styles = Array.from(appDocument.head.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]'));
await Promise.all(styles.map(link => new Promise<void>((resolve, reject) => {
  link.addEventListener('load', () => resolve(), { once: true });
  link.addEventListener('error', () => reject(new Error(`样式加载失败：${link.href}`)), { once: true });
  document.head.append(link);
})));
for (const script of appDocument.body.querySelectorAll('script')) script.remove();
document.body.replaceChildren(...Array.from(appDocument.body.childNodes));

// This harness has isolated memory-only preferences and synthetic IPC. It cannot
// read/write the real app's credentials or send any campus or payment request.
const memory = new Map<string, string>([['bjut_tos_accepted', 'true'], ['bjut_theme', 'basic'], ['bjut_color_mode', 'light']]);
Object.defineProperty(window, 'localStorage', { value: {
  getItem: (key: string) => memory.get(key) ?? null,
  setItem: (key: string, value: string) => memory.set(key, String(value)),
  removeItem: (key: string) => memory.delete(key), clear: () => memory.clear(),
} });
Object.defineProperty(navigator, 'userAgent', { value: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) MockApp' });
(window as any).__TAURI__ = {};
mockWindows('main');
const now = () => performance.now();
const calls: {cmd: string, payload: any, at: number}[] = [];
const table = {total: 0, rows: [], summary: {}};
const cfg = { accounts: [
  { user: '25000001', hasPassword: true, isDefault: true, isDisabled: false },
  { user: '25000002', hasPassword: true, isDefault: false, isDisabled: true },
], auto_login:false, check_interval:15, check_interval_bg:60, wifi_change_detect:true, log_level:'debug', theme:'basic', accent_color:'blue', color_mode:'light', vpn_compatibility:'high', whitelist:[], blacklist:[], network_profiles:[], preferred_interface:'', adaptive_network_checks:true, usage_alerts:false };
function center(account: string) {
  return { account, overview:{account,balance:'23 元',remainingFlow:'18 GB',status:'正常',updatedAt:'测试数据',loginHistory:[],onlineSessions:[],warnings:[]},
    fetchedAt:'2026-09-15',queryStartDate:'2026-08-01',queryEndDate:'2026-09-15',queryYear:'2026',
    ...Object.fromEntries(['usageRecords','monthlyBills','payments','operations','stopLogs','reopenLogs','packageLogs','devices','tariffGroups'].map(key=>[key,table])),
    service:{ accountStatus:'正常',packageOptions:[],canStopNow:true,canReopenNow:false,packageScheduled:false },
    passwordPolicy:{minLength:12,maxLength:16,requireUppercase:true,requireLowercase:true,requireDigit:true,requireSpecial:true}, securityQuestions:[],rechargeAvailable:true,warnings:[] };
}
mockIPC(async (cmd, payload: any = {}) => {
  calls.push({cmd,payload,at:now()});
  if (cmd === 'get_app_config') return cfg;
  if (cmd === 'get_credential_storage_status') return 'available';
  if (cmd === 'get_credential_storage_health') return {backend:'模拟存储',status:'available',persistent:true,message:'测试',savedAccounts:2,missingPasswordAccounts:[]};
  if (cmd === 'get_network_adapters') { await new Promise(r=>setTimeout(r,1200)); return {adapters:[],preferredInterface:'',selectionSupported:true}; }
  if (cmd === 'get_network_schedule') return {intervalSeconds:15,interfacePollSeconds:4,reason:'模拟检查'};
  if (cmd === 'get_current_network_state') return {state:'BjutCampus',loginType:'lgn-wired',ip:'172.26.1.2',ssid:'',bssid:'',timestamp:'测试'};
  if (cmd === 'get_countdown_status') return {status:'ticking',seconds:15};
  if (cmd === 'get_update_target') return {currentVersion:'test',platform:'windows',arch:'x86_64'};
  if (['get_logs','get_network_events','get_account_health','get_recoverable_recharges'].includes(cmd)) return [];
  if (cmd === 'plugin:window|is_visible' || cmd === 'plugin:window|is_focused') return true;
  if (cmd === 'plugin:window|is_minimized') return false;
  if (cmd === 'get_billing_center') return center(payload.currentSession ? '25000999' : payload.accountUser);
  if (cmd === 'query_billing_records') return {kind:payload.query.kind,page:1,pageSize:10,table};
  return null;
}, {shouldMockEvents:true});
await import('../src/main');
const panel=document.createElement('aside'); panel.style.cssText='position:fixed;right:8px;bottom:8px;z-index:30000;padding:12px;border:1px solid #64748b;border-radius:8px;background:#fff;color:#111;max-width:380px;font-size:12px;';
const button=document.createElement('button'); button.textContent='运行本轮界面回归'; button.id='qa-run';
const out=document.createElement('pre');out.id='qa-results';out.style.whiteSpace='pre-wrap';out.textContent='仅使用模拟账号与本地数据';panel.append(button,out);document.body.append(panel);
const assert=(value:unknown,message:string)=>{if(!value)throw new Error(message);};
const wait=async(test:()=>boolean)=>{for(let i=0;i<100;i++){if(test())return;await new Promise(r=>setTimeout(r,50));}throw new Error('等待界面超时');};
const click=(selector:string)=>(document.querySelector(selector) as HTMLElement)?.click();
button.addEventListener('click',async()=>{
  button.disabled=true;out.textContent='';const note=(text:string)=>out.textContent+=text+'\n';
  try {
    await wait(()=>!document.getElementById('app-loading-mask')!.classList.contains('is-visible'));
    assert(calls.find(c=>c.cmd==='frontend_ready')!.at < calls.find(c=>c.cmd==='get_network_adapters')!.at,'窗口仍在等待初始网卡查询');note('通过：先显示窗口，再读取网络快照');
    assert(document.querySelector('#override-account [data-value="1"]'),'禁用账号未保留在手动登录列表');
    click('#btn-open-billing');await wait(()=>document.getElementById('billing-center-account')!.textContent==='25000001');
    click('#billing-account-select .custom-select-trigger');
    assert(!document.querySelector('#billing-account-select .keyboard-active'),'鼠标打开后出现键盘框');
    click('#billing-account-select [data-value="25000002"]');await wait(()=>calls.some(c=>c.cmd==='get_billing_center'&&c.payload.accountUser==='25000002') && !(document.getElementById('btn-refresh-billing-center') as HTMLButtonElement).disabled);
    assert(calls.some(c=>c.cmd==='get_billing_center'&&c.payload.accountUser==='25000002'&&!c.payload.currentSession),'禁用账号请求不正确');note('通过：禁用账号可手动选择并读取计费');
    click('#billing-account-select .custom-select-trigger');click('#billing-account-select [data-value="__current_session__"]');
    await wait(()=>document.getElementById('billing-center-account')!.textContent==='25000999');
    assert(document.getElementById('billing-center-status')!.textContent==='正常','概览未采用 dashboard 状态');
    assert(calls.some(c=>c.cmd==='get_billing_center'&&c.payload.currentSession),'当前账号未请求 SSO');
    assert(!document.querySelector('#billing-recharge-card-account [data-value="__current_session__"]'),'SSO 标记不应成为充值账号');note('通过：当前会话显示真实账号及 dashboard 状态');
    click('[data-billing-section-target="records"]');click('#btn-query-billing-records');
    await wait(()=>calls.some(c=>c.cmd==='query_billing_records'));
    assert(calls.some(c=>c.cmd==='query_billing_records'&&c.payload.currentSession&&c.payload.accountUser==='25000999'),'账单查询未绑定当前真实账号');note('通过：账单查询复用当前会话并携带真实账号');
    click('[data-target="dashboard"]');
    const progress={id:99,generation:1,revision:0,message:'正在等待新的 IP 分配',percent:5,elapsedMs:0,complete:false};
    await emit('network-check-progress',progress);await new Promise(r=>setTimeout(r,300));
    const before=parseFloat(document.getElementById('network-check-bar')!.style.width);
    await new Promise(r=>setTimeout(r,300));assert(parseFloat(document.getElementById('network-check-bar')!.style.width)>before,'等待时进度未增长');
    await emit('network-check-progress',{...progress,revision:1,message:'正在确认登录类型',percent:65});
    await emit('network-check-progress',{...progress,id:98,message:'过期进度'});
    assert(document.getElementById('network-check-message')!.textContent==='正在确认登录类型','旧进度覆盖了新进度');
    await emit('network-check-progress',{...progress,revision:2,message:'检测完成',percent:100,complete:true});
    note('通过：阶段、持续进度与过期事件隔离');note('全部通过');
  }catch(error){note('失败：'+String(error));}finally{button.disabled=false;}
});
