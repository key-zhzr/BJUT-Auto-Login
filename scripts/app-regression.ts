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
const platform = new URL(location.href).searchParams.get('platform');
const mobileLayout = platform === 'android';
Object.defineProperty(document,'hidden',{configurable:true,value:false});
Object.defineProperty(navigator, 'userAgent', { value: mobileLayout ? 'Mozilla/5.0 (Linux; Android 15) Mobile MockApp' : platform === 'macos' ? 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) MockApp' : 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) MockApp' });
(window as any).__TAURI__ = {};
mockWindows('main');
const now = () => performance.now();
const calls: {cmd: string, payload: any, at: number}[] = [];
let initialAdapterFinishedAt = Infinity;
const table = {total: 0, rows: [], summary: {}};
const cfg = { accounts: [
  { user: '25000001', hasPassword: true, isDefault: true, isDisabled: false },
  { user: '25000002', hasPassword: true, isDefault: false, isDisabled: true },
  ...Array.from({length:6}, (_, i) => ({user:`2500000${i+3}`,hasPassword:true,isDefault:false,isDisabled:false})),
], auto_login:false, check_interval:15, check_interval_bg:60, wifi_change_detect:true, log_level:'debug', theme:'basic', accent_color:'blue', color_mode:'light', vpn_compatibility:'high', whitelist:[], blacklist:[], network_profiles:[], preferred_interface:'', adaptive_network_checks:true, usage_alerts:false };
function center(account: string) {
  return { account, overview:{account,balance:'23 元',remainingFlow:'18 GB',status:'正常',updatedAt:'测试数据',loginHistory:[],onlineSessions:[],warnings:[]},
    fetchedAt:'2026-09-15',queryStartDate:'2026-08-01',queryEndDate:'2026-09-15',queryYear:'2026',
    ...Object.fromEntries(['usageRecords','monthlyBills','payments','operations','stopLogs','reopenLogs','packageLogs','devices','tariffGroups'].map(key=>[key,table])),
    service:{ accountStatus:'正常',packageOptions:[],canStopNow:true,canReopenNow:false,packageScheduled:false },
    passwordPolicy:{minLength:12,maxLength:16,requireUppercase:true,requireLowercase:true,requireDigit:true,requireSpecial:true}, securityQuestions:[],rechargeAvailable:true,warnings:[] };
}
let diagnosticsCount = 0;
let requireBillingCaptcha = false;
let captchaSequence = 0;
let pendingCaptcha: {id:string;title:string;resolve:(accepted:boolean)=>void} | null = null;
async function publishFixtureCaptcha(title:string, resolve:(accepted:boolean)=>void) {
  const id = `image-fixture-${++captchaSequence}`;
  pendingCaptcha = {id,title,resolve};
  const canvas = document.createElement('canvas'); canvas.width=168; canvas.height=56;
  const context=canvas.getContext('2d')!;
  context.fillStyle='#edf3fc'; context.fillRect(0,0,168,56);
  context.font='italic 30px monospace'; context.fillStyle='#244c73'; context.fillText('7 x 2 a',14,38);
  await emit('image-captcha',{id,title,expiresInSeconds:120,image:canvas.toDataURL('image/png')});
}
async function fixtureCaptcha(title:string) {
  const accepted = await new Promise<boolean>(resolve => { void publishFixtureCaptcha(title,resolve); });
  if (!accepted) throw new Error('已取消验证码验证');
}
const routerSnapshot = {
  version: '0.1.6', managementUrl:'https://192.0.2.1/cgi-bin/luci/admin/network/bjut-autologin/billing',
  capabilities:['ports','billing','logs','settings','account_set','account_manage','uplink_manage','billing_records','billing_actions','uplink_actions','log_settings','operations','account_limits','pushplus','billing_background','maintenance'],
  ports:[{uplink:'dorm_a',device:'lan3',linkUp:1,speedMbps:'1000',rxBytes:'10000000000',txBytes:'2000000000'},
    {uplink:'dorm_b',device:'lan4',linkUp:1,speedMbps:'2500',rxBytes:'30000000000',txBytes:'5000000000'}],
  uplinks:[
    {id:'dorm_a',network:'wan14',network6:'wan16',device:'lan3',ipv4:'10.126.19.139',ipv6:'2001:db8::1',state:'online',message:'认证会话有效，互联网连接正常',internet4:'reachable',internet6:'reachable',account:'25000001',checking:1,check_message:'正在确认认证会话'},
    {id:'dorm_b',network:'wan24',device:'lan4',ipv4:'10.126.20.139',state:'authenticated_limited',message:'认证会话有效，外网探测暂未通过；保留当前登录',internet4:'unreachable',internet6:'not_configured',account:'25000002'},
  ],
  accounts:[{id:'account1',user:'25000001',enabled:1,hasPassword:1,loginType:'auto'},{id:'account2',user:'25000002',enabled:0,hasPassword:1,loginType:'lgn'}],
};
let routerRefreshDelay = 0;
let routerBillingPollDelay = 0;
let routerQuerySequence = 0;
const routerTickets = new Map<string,string>();
const recordTickets = new Map<string, any>();
const moduleTickets=new Map<string,string>();
let savedRouterInfo:any=new URL(location.href).searchParams.has('saved')?{address:'https://192.0.2.1',username:'root',certificate:'synthetic-pem',allowHttp:false}:null,routerFailures=0;
mockIPC(async (cmd, payload: any = {}) => {
  calls.push({cmd,payload,at:now()});
  if (cmd === 'discover_routers') return [{name:'测试路由器',address:'https://192.0.2.1/',version:'0.1.6',secure:true}];
  if (cmd === 'saved_router_connection') return savedRouterInfo;
  if (cmd === 'forget_router_password') {savedRouterInfo=null;return;}
  if (cmd === 'export_billing_csv') return '/mock-exports/router-usage.csv';
  if (cmd === 'reconnect_router') {
    if(payload.rememberPassword===false)savedRouterInfo=null;
    return routerSnapshot;
  }
  if (cmd === 'router_request') {
    if (payload.operation === 'billing_start') {
      const ticket=String(++routerQuerySequence).padStart(32,'0');routerTickets.set(ticket,payload.args.uplink);return {state:'loading',ticket};
    }
    if (payload.operation === 'billing_records') {const ticket=String(++routerQuerySequence).padStart(32,'0');recordTickets.set(ticket,payload.args);return {ticket};}
    if (payload.operation === 'billing_details'){const ticket=String(++routerQuerySequence).padStart(32,'0');moduleTickets.set(ticket,payload.args.module);return {ticket};}
    if (payload.operation === 'billing_action'){const ticket=String(++routerQuerySequence).padStart(32,'0');moduleTickets.set(ticket,'action');return {ticket};}
    if (payload.operation === 'billing_poll' && routerBillingPollDelay) await new Promise(resolve => setTimeout(resolve,routerBillingPollDelay));
    if (payload.operation === 'billing_poll' && recordTickets.has(payload.args.ticket)) {
      const query=recordTickets.get(payload.args.ticket),page=Number(query.page),offset=(page-1)*100;
      return {result:{state:'ready',account:'25000001',kind:query.kind,total:205,page,columns:['loginTime','flow'],rows:Array.from({length:Math.min(100,205-offset)},(_,i)=>[`2026-10-13 12:00 · ${offset+i+1}`,'1024'])}};
    }
    if (payload.operation === 'billing_poll' && moduleTickets.get(payload.args.ticket)==='action') return {result:{state:'ready',message:'操作已完成'}};
    if (payload.operation === 'billing_poll' && moduleTickets.has(payload.args.ticket)){
      const module=moduleTickets.get(payload.args.ticket);return {result:{state:'ready',account:'25000001',module:module==='action'?undefined:module,message:module==='action'?'操作已完成':'计费数据已更新',canStop:1,canReopen:0,canCancel:1,installmentFlag:'100',useMoney:'25',packages:[{id:'2',name:'测试校园套餐'}],questions:[{id:'1',name:'测试问题一'},{id:'2',name:'测试问题二'},{id:'3',name:'测试问题三'}],FLDMINLENGTH:8,mauth:'1',macs:{columns:['online','mac','device','lastLoginAt','lastIp'],rows:[['在线','aabbccddeeff','测试设备','2026-10-18','192.0.2.20']]},history:{columns:['loginTime','logoutTime','ip','flow'],rows:[['2026-10-18 12:00','2026-10-18 13:00','192.0.2.20','1024 MB']]}},sessions:[{sessionIndex:'0',ip:'192.0.2.20',mac:'aabbccddeeff',useTime:'3600'}],sessionsAvailable:true};
    }
    if (payload.operation === 'billing_poll') return {result:{state:'ready',message:'计费数据已更新',account:'25000001',balance:'42.125',totalFlow:'40960',remainingFlow:'30720',accountStatus:'正常',package:'测试套餐'},sessions:[{sessionIndex:'0',ip:'192.0.2.20',mac:'00:00:00:00:00:01'}],sessionsAvailable:true};
    if (payload.operation === 'settings') return {service:{enabled:'1',interval:'20',timeout:'3',offline_confirmations:'2',managed_routes:'1',mwan3:'1'},uplinks:[{id:'dorm_a',enabled:'1',network:'wan14',network6:'wan16',login_type:'bjut-sushe',accounts:'account1'},{id:'dorm_b',network:'wan24',enabled:'1'}]};
    if (payload.operation === 'billing_cached') return {items:[]};
    if (payload.operation === 'pushplus_test') return {state:'ready',message:'模拟测试消息已排队'};
    if (payload.operation === 'maintenance_info') return {platform:{version:'0.1.6-r14',target:'mediatek/filogic',arch:'aarch64_cortex-a53',format:'apk'},backups:[{id:'backup-1792690000-12345678',createdAt:'2026-10-23 12:00:00',version:'0.1.6-r13'}]};
    if (payload.operation === 'maintenance_start') return {state:'loading',ticket:'e'.repeat(32)};
    if (payload.operation === 'maintenance_poll') return {state:'prepared',message:'模拟安装包已校验',percent:100,release:'openwrt-v0.1.6-r14'};
    if (payload.operation === 'logs') return {text:'[测试路由器] 上游 lan3 认证已确认'};
    if (payload.operation === 'account_set') {const account=routerSnapshot.accounts.find(item=>item.id===payload.args.account)!;account.enabled=Number(payload.args.enabled);return {state:'ready'};}
    if (payload.operation === 'save_settings') return {state:'ready'};
    if (payload.operation === 'account_save') {const item=routerSnapshot.accounts.find(item=>item.id===payload.args.account);if(item){item.user=payload.args.username;item.loginType=payload.args.login_type;}return {state:'ready'};}
    if (['account_delete','account_move','uplink_save','uplink_delete','logout','switch_account'].includes(payload.operation)) return {state:'ready'};
  }

  if (cmd === 'connect_router') {
    if (!payload.certificate) throw 'ROUTER_CERT_UNTRUSTED:模拟路由器证书尚未受信任';
    savedRouterInfo=payload.rememberPassword?{address:payload.address,username:payload.username,certificate:payload.certificate,allowHttp:payload.allowHttp,autoReconnectReady:true,networkName:"测试路由网络"}:null;
    return routerSnapshot;
  }
  if (cmd === 'get_router_status') {
    if(routerFailures){routerFailures--;throw '连接暂时中断';}
    if (routerRefreshDelay) await new Promise(resolve => setTimeout(resolve, routerRefreshDelay));
    return routerSnapshot;
  }
  if (cmd === 'probe_router_certificate') return {pem:'synthetic-pem',fingerprint:Array(32).fill('AB').join(':'),address:'https://192.0.2.1/'};
  if (cmd === 'router_login') return '路由器已开始核对所选上游的登录状态';
  if (cmd === 'sync_router_accounts') return '选定账号已同步到路由器';
  if (cmd === 'trigger_manual_check') {
    await emit('network-check-progress', {id:100,generation:1,revision:0,message:'手动更新正在确认登录类型',percent:30,elapsedMs:0,complete:false});
    return null;
  }
  if (cmd === 'run_network_diagnostics') {
    diagnosticsCount++;
    await emit('network-diagnostic-step', {runId:payload.runId,step:{id:'dns',label:'DNS 解析',status:'success',message:'模拟 DNS 已完成',durationMs:3}});
    await emit('network-diagnostic-step', {runId:'old-run',step:{id:'dns',label:'DNS 解析',status:'error',message:'过期结果',durationMs:1}});
    await new Promise(resolve => setTimeout(resolve, 400));
    if (diagnosticsCount > 1) throw new Error('模拟诊断失败');
    await emit('network-diagnostic-progress', {runId:payload.runId,percent:75,label:'正在确认认证网关'});
    await new Promise(resolve => setTimeout(resolve, 400));
    return {overall:'healthy',summary:'模拟诊断完成',createdAt:'2026-09-19',ssid:'',ip:'172.26.1.2',steps:[]};
  }
  if (cmd === 'get_app_config') return cfg;
  if (cmd === 'get_network_time') return {unixMs: Date.UTC(2026,8,21,0,0),source:'模拟 NTP'};
  if (cmd === 'export_config_backup') return {payload:JSON.stringify({version:3,ciphertext:'encrypted-fixture'.repeat(80)}),accountCount:0,passwordCount:0,missingPasswordAccounts:[]};
  if (cmd === 'get_credential_storage_status') return 'available';
  if (cmd === 'get_credential_storage_health') return {backend:'模拟存储',status:'available',persistent:true,message:'测试',savedAccounts:cfg.accounts.length,missingPasswordAccounts:[]};
  if (cmd === 'get_network_adapters') {
    await new Promise(r=>setTimeout(r,1200));
    if (initialAdapterFinishedAt === Infinity) initialAdapterFinishedAt = now();
    return {adapters:[
      {id:'wlan0',name:mobileLayout?'wlan0':'USB 10/100/1000 LAN',interfaceName:'wlan0',transport:mobileLayout?'wifi':'ethernet',ipv4:['192.0.2.16'],ipv6:['2001:db8::16'],connected:true,selectable:!mobileLayout,selected:true},
      {id:'tun0',name:'tun0',interfaceName:'tun0',transport:'vpn',ipv4:['198.18.0.1'],ipv6:[],connected:true,selectable:false,selected:false},
    ],preferredInterface:'',selectionSupported:!mobileLayout};
  }
  if (cmd === 'get_network_schedule') return {intervalSeconds:15,interfacePollSeconds:4,reason:'模拟检查'};
  if (cmd === 'get_current_network_state') return {state:'BjutCampus',loginType:'lgn-wired',ip:'172.26.1.2',ssid:'',bssid:'',timestamp:'测试'};
  if (cmd === 'get_countdown_status') return {status:'ticking',seconds:15};
  if (cmd === 'get_update_target') return {currentVersion:'test',platform:mobileLayout?'android':'windows',arch:mobileLayout?'aarch64':'x86_64'};
  if (['get_logs','get_network_events','get_account_health','get_recoverable_recharges'].includes(cmd)) return [];
  if (cmd === 'plugin:window|is_visible' || cmd === 'plugin:window|is_focused') return true;
  if (cmd === 'plugin:window|is_minimized') return false;
  if (cmd === 'get_billing_center') {
    if (requireBillingCaptcha) { requireBillingCaptcha=false; await fixtureCaptcha('计费系统验证码'); }
    return center(payload.currentSession ? '25000999' : payload.accountUser);
  }
  if (cmd === 'answer_image_captcha') {
    if (pendingCaptcha?.id !== payload.id) throw new Error('验证请求已过期');
    const pending = pendingCaptcha; pendingCaptcha = null;
    await emit('image-captcha-close',{id:pending.id});
    if (payload.refresh) await publishFixtureCaptcha(pending.title,pending.resolve);
    else pending.resolve(Boolean(payload.text));
    return null;
  }
  if (cmd === 'begin_billing_webvpn') {
    await fixtureCaptcha('学校统一认证验证码');
    return {stage:'sms',challengeId:'fixture-challenge',message:'请完成学校要求的短信验证'};
  }
  if (cmd === 'verify_billing_webvpn') {
    if (payload.resend) return {stage:'sms',challengeId:'fixture-challenge',message:'验证码已请求，请查看短信'};
    if (payload.token !== '123456') throw new Error('验证码未通过');
    return {stage:'ready',challengeId:null,message:'校外连接已就绪'};
  }
  if (cmd === 'query_billing_records') return {kind:payload.query.kind,page:1,pageSize:10,table};
  return null;
}, {shouldMockEvents:true});
await import('../src/main');
const panel=document.createElement('aside'); panel.style.cssText='position:fixed;right:8px;bottom:8px;z-index:30000;padding:12px;border:1px solid #64748b;border-radius:8px;background:#fff;color:#111;max-width:380px;font-size:12px;';
const button=document.createElement('button'); button.textContent='运行本轮界面回归'; button.id='qa-run';
const hide=document.createElement('button');hide.textContent='隐藏验收面板';hide.onclick=()=>{panel.hidden=true;};panel.append(hide);
const status=document.createElement('p');status.id='qa-status';status.setAttribute('role','status');
const out=document.createElement('pre');out.id='qa-results';out.style.cssText='white-space:pre-wrap;max-height:35vh;overflow:auto';out.textContent='仅使用模拟账号与本地数据';panel.append(button,status,out);document.body.append(panel);
const assert=(value:unknown,message:string)=>{if(!value)throw new Error(message);};
const wait=async(test:()=>boolean,timeout=5000)=>{const deadline=performance.now()+timeout;while(performance.now()<deadline){if(test())return;await new Promise(r=>setTimeout(r,50));}throw new Error('等待界面超时');};
const click=(selector:string)=>(document.querySelector(selector) as HTMLElement)?.click();
button.addEventListener('click',async()=>{
  button.disabled=true;out.textContent='';const note=(text:string)=>{status.textContent=text;out.textContent+=text+'\n';};
  try {
    await wait(()=>!document.getElementById('app-loading-mask')!.classList.contains('is-visible'));
    if (!mobileLayout) assert(calls.find(c=>c.cmd==='frontend_ready')!.at < initialAdapterFinishedAt,'窗口仍在等待初始网卡查询');
    note('通过：启动界面已就绪');
    assert(document.querySelector('#override-account [data-value="1"]'),'禁用账号未保留在手动登录列表');
    click('#btn-open-billing');await wait(()=>document.getElementById('billing-center-account')!.textContent==='25000001');
    click('#billing-account-select .custom-select-trigger');
    assert(!document.querySelector('#billing-account-select .keyboard-active'),'鼠标打开后出现键盘框');
    click('#billing-account-select [data-value="25000002"]');await wait(()=>calls.some(c=>c.cmd==='get_billing_center'&&c.payload.accountUser==='25000002') && !(document.getElementById('btn-refresh-billing-center') as HTMLButtonElement).disabled);
    assert(calls.some(c=>c.cmd==='get_billing_center'&&c.payload.accountUser==='25000002'&&!c.payload.currentSession),'禁用账号请求不正确');note('通过：禁用账号可手动选择并读取计费');
    requireBillingCaptcha=true;click('#btn-refresh-billing-center');
    await wait(()=>!!document.querySelector('.image-captcha-overlay:not(.hidden)'));
    click('.image-captcha-overlay:not(.hidden) [data-cancel]');
    await wait(()=>!(document.getElementById('btn-refresh-billing-center') as HTMLButtonElement).disabled);
    assert(document.getElementById('billing-center-message')!.textContent!.includes('取消验证码'),'取消验证没有结束计费请求');
    requireBillingCaptcha=true;click('#btn-refresh-billing-center');
    await wait(()=>!!document.querySelector('.image-captcha-overlay:not(.hidden)'));
    (document.querySelector('.image-captcha-overlay:not(.hidden) input') as HTMLInputElement).value='7x2a';
    (document.querySelector('.image-captcha-overlay:not(.hidden) form') as HTMLFormElement).requestSubmit();
    await wait(()=>!(document.getElementById('btn-refresh-billing-center') as HTMLButtonElement).disabled);
    note('通过：jfself 图片验证、取消及恢复查询');
    click('#billing-account-select .custom-select-trigger');click('#billing-account-select [data-value="__current_session__"]');
    await wait(()=>document.getElementById('billing-center-account')!.textContent==='25000999');
    assert(document.getElementById('billing-center-status')!.textContent==='正常','概览未采用 dashboard 状态');
    assert(calls.some(c=>c.cmd==='get_billing_center'&&c.payload.currentSession),'当前账号未请求 SSO');
    assert(!document.querySelector('#billing-recharge-card-account [data-value="__current_session__"]'),'SSO 标记不应成为充值账号');note('通过：当前会话显示真实账号及 dashboard 状态');
    click('[data-billing-section-target="records"]');
    (document.getElementById('billing-record-start-date') as HTMLInputElement).value = '2024-01-01';
    (document.getElementById('billing-record-end-date') as HTMLInputElement).value = '2025-12-31';
    click('#btn-query-billing-records');
    await wait(()=>calls.some(c=>c.cmd==='query_billing_records'));
    assert(calls.some(c=>c.cmd==='query_billing_records'&&c.payload.currentSession&&c.payload.accountUser==='25000999'),'账单查询未绑定当前真实账号');assert(calls.some(c=>c.cmd==='query_billing_records'&&c.payload.query.startDate==='2024-01-01'&&c.payload.query.endDate==='2025-12-31'),'跨年查询仍被日期限制拦截');note('通过：跨年账单查询复用当前会话并携带真实账号');
    await wait(()=>!(document.getElementById('btn-billing-webvpn') as HTMLButtonElement).disabled);
    click('#btn-billing-webvpn');
    await wait(()=>!!document.querySelector('#billing-webvpn-modal:not(.hidden)'));
    (document.querySelector('#billing-webvpn-modal form') as HTMLFormElement).requestSubmit();
    await wait(()=>!!document.querySelector('.image-captcha-overlay:not(.hidden)'));
    const firstCaptchaId=pendingCaptcha!.id;
    click('.image-captcha-overlay:not(.hidden) [data-refresh]');
    await wait(()=>pendingCaptcha?.id!==firstCaptchaId && !!document.querySelector('.image-captcha-overlay:not(.hidden)'));
    (document.querySelector('.image-captcha-overlay:not(.hidden) input') as HTMLInputElement).value='7x2a';
    (document.querySelector('.image-captcha-overlay:not(.hidden) form') as HTMLFormElement).requestSubmit();
    await wait(()=>!!document.querySelector('#billing-webvpn-modal .webvpn-sms:not([hidden])'));
    assert(calls.some(c=>c.cmd==='answer_image_captcha' && c.payload.refresh),'WebVPN 验证码未使用换图请求');
    assert(calls.some(c=>c.cmd==='begin_billing_webvpn'),'校外入口未启动认证');
    assert(!calls.find(c=>c.cmd==='begin_billing_webvpn')!.payload.password,'WebView 不应提交已保存密码');
    click('#billing-webvpn-modal [data-send]');
    await wait(()=>calls.some(c=>c.cmd==='verify_billing_webvpn'&&c.payload.resend));
    await wait(()=>!(document.querySelector('#billing-webvpn-modal [type="submit"]') as HTMLButtonElement).disabled);
    (document.getElementById('webvpn-code') as HTMLInputElement).value='123456';
    (document.querySelector('#billing-webvpn-modal form') as HTMLFormElement).requestSubmit();
    await wait(()=>calls.some(c=>c.cmd==='get_billing_center'&&c.payload.webvpn));
    assert(!document.querySelector('#billing-account-select [data-value="__current_session__"]'),'校外模式不应使用校园网会话');
    await wait(()=>!(document.getElementById('btn-billing-direct') as HTMLButtonElement).disabled);
    click('#btn-billing-direct');
    await wait(()=>calls.filter(c=>c.cmd==='get_billing_center').slice(-1)[0]?.payload.webvpn===false);
    note('通过：校外认证、短信验证、计费路径及切回校内');
    click('[data-target="dashboard"]');
    const progress={id:99,generation:1,revision:0,message:'正在等待新的 IP 分配',percent:5,elapsedMs:0,complete:false};
    const networkPanel = document.getElementById('network-check-progress')!;
    const motion = !window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    await emit('network-check-progress',progress);
    if (motion) assert(networkPanel.classList.contains('progress-animating') && networkPanel.getAnimations().length > 0,'控制台进度缺少出现动画');
    await new Promise(r=>setTimeout(r,300));
    const before=parseFloat(document.getElementById('network-check-bar')!.style.width);
    await new Promise(r=>setTimeout(r,300));assert(parseFloat(document.getElementById('network-check-bar')!.style.width)>before,'等待时进度未增长');
    await emit('network-check-progress',{...progress,revision:1,message:'正在确认登录类型',percent:65});
    await emit('network-check-progress',{...progress,id:98,message:'过期进度'});
    assert(document.getElementById('network-check-message')!.textContent==='正在确认登录类型','旧进度覆盖了新进度');
    await emit('network-check-progress',{...progress,revision:2,message:'检测完成',percent:100,complete:true});
    note('通过：阶段、持续进度与过期事件隔离');
    if (motion) {
      await wait(()=>networkPanel.classList.contains('progress-animating'));
      assert(!networkPanel.hidden, '控制台进度未等退出动画就隐藏');
    } else await wait(()=>networkPanel.hidden);
    click('#btn-manual-update');
    await wait(()=>document.getElementById('network-check-message')!.textContent==='手动更新正在确认登录类型');
    await wait(()=>!networkPanel.classList.contains('progress-animating'));
    assert(!networkPanel.hidden && !networkPanel.classList.contains('progress-animating'),'新检测被旧退出动画隐藏或遗留动画样式');
    await emit('network-check-progress',{...progress,id:100,revision:1,message:'手动更新完成',percent:100,complete:true});
    note('通过：控制台进度出入动画与连续手动更新');
    click('[data-target="diagnostics"]');
    const diagnosticPanel = document.getElementById('diagnostic-progress')!;
    click('#btn-run-diagnostics');
    if (motion) assert(diagnosticPanel.getAnimations().length > 0, '诊断进度缺少出现动画');
    await wait(()=>document.querySelector('[data-step-id="dns"]')?.textContent?.includes('模拟 DNS 已完成') === true);
    assert((document.getElementById('btn-run-diagnostics') as HTMLButtonElement).disabled, '没有在诊断完成前显示模块结果');
    assert(!document.getElementById('diagnostic-steps')!.textContent?.includes('过期结果'), '旧诊断结果覆盖当前结果');
    assert(!(document.getElementById('account-health-panel') as HTMLDetailsElement).open, '正常账号状态未折叠');
    note('通过：诊断逐项显示、过期结果隔离、正常账号折叠');
    await wait(()=>!(document.getElementById('btn-run-diagnostics') as HTMLButtonElement).disabled);
    if (motion) {
      await wait(()=>diagnosticPanel.classList.contains('progress-animating'));
      assert(!diagnosticPanel.hidden, '诊断进度未等退出动画就隐藏');
    } else await wait(()=>diagnosticPanel.hidden);
    click('#btn-run-diagnostics');
    await wait(()=>document.getElementById('diagnostic-summary-title')!.textContent?.includes('诊断失败') === true);
    if (motion) assert(!diagnosticPanel.hidden && diagnosticPanel.classList.contains('progress-animating'), '诊断失败未播放收起动画');
    await wait(()=>diagnosticPanel.hidden);
    note('通过：诊断进度出入动画、连续诊断及失败收起');
    const { AnimatedVisibility } = await import('../src/animated-visibility');
    const matchMedia = window.matchMedia;
    const probe = document.createElement('div'); probe.textContent = '减少动态效果检查'; probe.hidden = true; document.body.append(probe);
    try {
      window.matchMedia = query => query === '(prefers-reduced-motion: reduce)' ? { matches:true } as MediaQueryList : matchMedia.call(window,query);
      const visibility = new AnimatedVisibility(probe); visibility.setVisible(true);
      assert(!probe.hidden && probe.getAnimations().length === 0, '减少动态效果模式仍播放进入动画');
      visibility.setVisible(false); assert(probe.hidden && probe.getAnimations().length === 0, '减少动态效果模式未直接收起');
    } finally { window.matchMedia = matchMedia; probe.remove(); }
    note('通过：减少动态效果设置');
    click('#btn-open-billing'); await wait(()=>document.getElementById('billing-center')!.classList.contains('active'));
    click('[data-billing-section-target="recharge"]');
    await wait(()=>document.querySelector('.billing-recharge-hours')!.classList.contains('is-open'));
    assert(document.querySelector('.billing-recharge-hours')!.textContent?.includes('08:00'), '充值时段未使用远端北京时间');
    note('通过：充值时段使用网络时间并显示绿色');
    click('[data-target="settings"]');
    assert(!document.getElementById('btn-export-config-qr'), '设置页仍显示额外备份按钮');
    click('#btn-export-config');
    await wait(()=>!!document.querySelector('#config-transfer-modal:not(.hidden)'));
    const transferDialog=document.querySelector<HTMLElement>('.config-transfer-dialog')!;
    assert(parseFloat(getComputedStyle(transferDialog).paddingLeft)>=16,'备份弹窗缺少内边距');
    if(motion) assert(document.getElementById('config-transfer-modal')!.getAnimations({subtree:true}).length>0,'备份弹窗缺少进入动画');
    (document.querySelector('#config-transfer-modal input[name="accounts"]') as HTMLInputElement).checked=false;
    (document.querySelector('#config-transfer-modal input[value="qr"]') as HTMLInputElement).checked=true;
    (document.getElementById('transfer-password') as HTMLInputElement).value='123';
    (document.getElementById('transfer-confirm') as HTMLInputElement).value='123';
    (document.querySelector('#config-transfer-modal form') as HTMLFormElement).requestSubmit();
    await wait(()=>!!document.querySelector('.config-qr-body canvas')?.getAttribute('width'));
    assert(calls.some(call=>call.cmd==='export_config_backup' && call.payload.scope.settings && !call.payload.scope.accounts),'导出范围未送往后端');
    const canvas = document.querySelector<HTMLCanvasElement>('.config-qr-body canvas')!;
    const pixels = canvas.getContext('2d')!.getImageData(0,0,canvas.width,canvas.height);
    const {default:jsQR} = await import('jsqr');
    assert(jsQR(pixels.data,pixels.width,pixels.height)?.data.startsWith('BJUTALQR1:'),'生成二维码不能解码');
    click('.config-qr-close');
    assert(!document.querySelector('.config-qr-overlay'),'关闭后二维码仍驻留');
    note('通过：备份弹窗、3 位密码、按范围导出与二维码解码');
    click('#btn-router-manual-connect');
    assert(!document.getElementById('router-connect-modal')!.classList.contains('hidden'),'手动连接弹窗未出现');
    (document.getElementById('router-address') as HTMLInputElement).value='https://192.0.2.1';
    (document.getElementById('router-certificate') as HTMLTextAreaElement).value='';
    (document.getElementById('router-password') as HTMLInputElement).value='synthetic-router-password';
    (document.getElementById('router-remember-password') as HTMLInputElement).checked=true;
    (document.getElementById('router-connect-form') as HTMLFormElement).requestSubmit();
    await wait(()=>!document.getElementById('confirm-modal')!.classList.contains('hidden'));
    assert(document.getElementById('confirm-modal-text')!.textContent!.includes('SHA-256'),'未显示证书指纹');
    assert(!calls.some(call=>call.cmd==='connect_router'&&call.payload.certificate),'证书尚未确认就发送认证');
    click('#btn-confirm-ok');
    await wait(()=>!document.getElementById('router-connected')!.hidden);
    assert(document.querySelectorAll('.router-port-card').length===2,'路由端口状态未分别显示');
    assert(document.getElementById('router-uplink-status')!.textContent!.includes('正在确认认证会话'),'未保留状态并显示检测步骤');
    assert((document.getElementById('router-password') as HTMLInputElement).value==='','连接后仍保留管理密码');
    assert(!memory.get('bjut_router_password'),'管理密码写入了 UI 存储');
    assert(savedRouterInfo && !JSON.stringify(savedRouterInfo).includes('synthetic-router-password'),'保存信息包含明文密码');
    click('#router-uplink-select .custom-select-trigger'); click('#router-uplink-select [data-value="dorm_b"]');
    click('#router-account-select .custom-select-trigger'); click('#router-account-select [data-value="account2"]');
    click('#btn-router-login');
    await wait(()=>calls.some(call=>call.cmd==='router_login'&&call.payload.uplink==='dorm_b'&&call.payload.account==='account2'));
    (document.getElementById('router-sync-details') as HTMLDetailsElement).open=true;
    await wait(()=>!!document.querySelector('#router-sync-accounts input[value="25000002"]'));
    (document.querySelector('#router-sync-accounts input[value="25000002"]') as HTMLInputElement).checked=true;
    click('#btn-router-sync');
    await wait(()=>calls.some(call=>call.cmd==='sync_router_accounts'&&call.payload.users.length===1&&call.payload.users[0]==='25000002'));
    note('通过：路由多上游状态、手动选择停用账号、按选定范围同步与密码清理');
    await wait(()=>!(document.getElementById('btn-router-sync') as HTMLButtonElement).disabled);
    assert(!document.getElementById('router-connect-modal')!.classList.contains('hidden')===false,'连接后弹窗未关闭');
    click('#btn-open-billing');
    await wait(()=>document.getElementById('billing-center-balance')!.textContent!.includes('42.13'));
    assert(!document.querySelector<HTMLElement>('.billing-workbench')!.hidden,'路由计费未使用本机计费界面');
    assert(document.getElementById('btn-billing-source-router')!.getAttribute('aria-pressed')==='true','未选中路由来源');
    assert(document.getElementById('billing-center-account')!.textContent!.includes('25000001'),'路由账号未映射');
    assert(document.getElementById('billing-center-flow')!.textContent!.includes('30.00 GB'),'剩余流量未映射');
    assert(document.getElementById('billing-mauth-badge')!.textContent==='已开启','无感认证状态未映射');
    assert(document.querySelectorAll('#billing-online-list .billing-record-card').length===1,'在线会话未映射');
    assert(calls.filter(call=>call.cmd==='router_request'&&call.payload.operation==='billing_start').length===1,'路由计费未按账号查询');
    click('.billing-mobile-nav [data-billing-section-target="records"]');
    (document.getElementById('billing-record-start-date') as HTMLInputElement).value='2026-01-01';
    (document.getElementById('billing-record-end-date') as HTMLInputElement).value='2026-10-13';
    click('#btn-query-billing-records');
    await wait(()=>document.getElementById('billing-records-list')!.textContent!.includes('2026-10-13 12:00'));
    assert(calls.filter(call=>call.cmd==='router_request'&&call.payload.operation==='billing_records').length===1,'重复查询了路由账单');
    assert(document.getElementById('billing-record-total')!.textContent==='205','路由账单合计未保留');
    click('#btn-billing-record-next');
    await wait(()=>document.getElementById('billing-records-list')!.textContent!.includes('2026-10-13 12:00 · 101'));
    assert(document.getElementById('billing-record-page-label')!.textContent!.includes('2 / 3'),'路由账单翻页未保留总页数');
    assert(calls.some(call=>call.cmd==='router_request'&&call.payload.operation==='billing_records'&&call.payload.args.page==='2'),'路由账单没有请求第二页');
    click('#btn-export-billing-records');
    await wait(()=>calls.some(call=>call.cmd==='export_billing_csv'&&call.payload.kind==='usage'&&call.payload.csv.includes('使用流量')));
    await wait(()=>!document.getElementById('alert-modal')!.classList.contains('hidden'));
    click('#btn-alert-ok');
    click('.billing-mobile-nav [data-billing-section-target="services"]');
    await wait(()=>document.getElementById('billing-service-package')!.textContent!.includes('测试套餐'));
    assert(document.getElementById('billing-service-limit')!.textContent!.includes('100'),'消费限额未映射');
    click('#btn-billing-stop-now');
    await wait(()=>!document.getElementById('confirm-modal')!.classList.contains('hidden'));
    assert(document.getElementById('confirm-modal-text')!.textContent!.includes('停机'),'计费操作未提示风险');
    click('#btn-confirm-ok');
    await wait(()=>calls.some(call=>call.cmd==='router_request' && call.payload.operation==='billing_action' && call.payload.args.operation==='stopNow'));
    await wait(()=>!document.getElementById('alert-modal')!.classList.contains('hidden'));
    click('#btn-alert-ok');
    click('.billing-mobile-nav [data-billing-section-target="devices"]');
    await wait(()=>document.getElementById('billing-device-count')!.textContent==='1');
    assert(document.getElementById('billing-device-list')!.textContent!.includes('aabbccddeeff'),'绑定设备未映射');
    assert(!document.getElementById('btn-billing-password')!.disabled,'路由账号安全未启用');
    assert(!document.getElementById('btn-billing-questions')!.disabled,'密码保护问题未映射');
    click('.billing-mobile-nav [data-billing-section-target="overview"]');
    await wait(()=>!document.getElementById('btn-toggle-billing-mauth')!.disabled);
    click('#btn-toggle-billing-mauth');
    await wait(()=>!document.getElementById('confirm-modal')!.classList.contains('hidden'));
    click('#btn-confirm-ok');
    await wait(()=>calls.some(call=>call.cmd==='router_request' && call.payload.operation==='billing_action' && call.payload.args.operation==='setMauth'));
    await wait(()=>!document.getElementById('alert-modal')!.classList.contains('hidden'));
    click('#btn-alert-ok');
    click('.billing-mobile-nav [data-billing-section-target="devices"]');
    await wait(()=>document.getElementById('billing-device-count')!.textContent==='1');
    click('.billing-mobile-nav [data-billing-section-target="recharge"]');
    assert(!document.getElementById('billing-recharge-unavailable')!.hidden,'路由来源未显示充值说明');
    assert(document.querySelector<HTMLElement>('.billing-payment-panel')!.hidden,'路由来源仍显示充值表单');
    note('通过：路由器来源复用当前设备计费界面、记录、业务与安全设置');
    click('#btn-billing-source-local');
    await wait(()=>document.getElementById('billing-center-balance')!.textContent!.includes('23 元'));
    assert(!document.getElementById('btn-billing-webvpn')!.hidden,'切回当前设备后校外访问入口未恢复');
    assert(!document.querySelector<HTMLElement>('.billing-payment-panel')!.hidden,'切回当前设备后充值表单未恢复');
    note('通过：计费来源切换保持同一界面，仅数据来源不同');
    click('[data-target="accounts"]');
    assert(!document.getElementById('router-accounts-panel')!.hidden && document.getElementById('router-accounts-list')!.textContent!.includes('25000002'),'账号管理未融合路由账号');
    Array.from(document.querySelectorAll<HTMLButtonElement>('#router-accounts-list button')).find(button=>button.textContent==='编辑')!.click();
    await wait(()=>!!document.querySelector('#router-admin-editor:not(.hidden)'));
    const editor=document.getElementById('router-admin-editor')!,user=editor.querySelector<HTMLInputElement>('input[type=text]')!;
    user.value='25000011';window.dispatchEvent(new Event('focus'));assert(user.value==='25000011','路由账号编辑草稿丢失');
    (editor.querySelector('form') as HTMLFormElement).requestSubmit();
    await wait(()=>calls.some(call=>call.cmd==='router_request'&&call.payload.operation==='account_save'&&call.payload.args.username==='25000011'&&call.payload.args.password===''));
    await wait(()=>!document.getElementById('router-admin-editor'));
    click('[data-target="logs"]');
    await wait(()=>document.getElementById('router-logs-content')!.textContent!.includes('lan3 认证'));
    click('[data-target="settings"]');
    await wait(()=>(document.getElementById('router-service-interval') as HTMLInputElement).value==='20');
    (document.getElementById('router-service-interval') as HTMLInputElement).value='30';
    (document.getElementById('router-settings-form') as HTMLFormElement).requestSubmit();
    await wait(()=>calls.some(call=>call.cmd==='router_request'&&call.payload.operation==='save_settings'&&call.payload.args.interval==='30'));
    note('通过：路由计费统一界面、账号融合、日志与设置保存');
    await wait(()=>!(document.getElementById('btn-router-settings-save') as HTMLButtonElement).disabled);
    routerFailures=1;click('#btn-router-refresh');
    await wait(()=>document.getElementById('router-message')!.textContent!.includes('连接中断'));
    await wait(()=>calls.some(call=>call.cmd==='reconnect_router') && !document.getElementById('router-connected')!.hidden,7000);
    note('通过：路由账号编辑、统一计费界面、账单查询和断线自动重连');
    const {checkRouterDropdownLayout}=await import('./motion-regression');await checkRouterDropdownLayout(note);
    click('#btn-open-billing');
    // Reconnection preserves the user's source choice. This case exercises a
    // pending router query explicitly rather than relying on a source reset.
    click('#btn-billing-source-router');
    await wait(()=>!(document.getElementById('btn-refresh-billing-center') as HTMLButtonElement).disabled);
    routerBillingPollDelay=250;
    const oldPolls=calls.filter(call=>call.cmd==='router_request'&&call.payload.operation==='billing_poll').length;
    click('#btn-refresh-billing-center');
    await wait(()=>calls.filter(call=>call.cmd==='router_request'&&call.payload.operation==='billing_poll').length>oldPolls);
    click('[data-target="settings"]');
    routerRefreshDelay=180;
    click('#btn-router-refresh'); click('#btn-router-disconnect');
    await wait(()=>document.getElementById('billing-center-balance')!.textContent!.includes('23 元'));
    await new Promise(resolve => setTimeout(resolve,350));
    routerBillingPollDelay=0;
    assert(document.getElementById('router-connected')!.hidden,'断开后旧检测结果重新显示已连接');
    assert(document.getElementById('billing-center-balance')!.textContent!.includes('23 元'),'断开后旧路由账单覆盖了本机数据');
    assert(!(document.getElementById('btn-refresh-billing-center') as HTMLButtonElement).disabled,'断开后计费刷新停留在禁用状态');
    await wait(()=>document.getElementById('router-accounts-panel')!.hidden);
    assert(document.getElementById('router-accounts-panel')!.hidden && !document.querySelector<HTMLElement>('.billing-workbench')!.hidden,'断开后路由数据或查询来源未清理');
    routerRefreshDelay=0;
    note('通过：断开路由连接后忽略尚未返回的检测结果');

    click('[data-target="accounts"]'); click('#btn-show-add');
    const draftPassword=document.getElementById('acc-password') as HTMLInputElement;
    const draftUser=document.getElementById('acc-username') as HTMLInputElement;
    draftUser.value='draft-account'; draftPassword.value='draft-password';
    Object.defineProperty(document,'hidden',{configurable:true,value:true});
    document.dispatchEvent(new Event('visibilitychange'));
    assert(draftPassword.value==='draft-password' && draftPassword.type==='password' && draftUser.value==='draft-account','进入后台丢失填写内容');
    Object.defineProperty(document,'hidden',{configurable:true,value:false}); document.dispatchEvent(new Event('visibilitychange'));
    click('#btn-cancel-add'); assert(draftPassword.value==='', '取消后未清空草稿密码');
    note('通过：切换应用保留表单，取消后清空');
    assert(!document.querySelector('.page-content'),'仍存在裁切内容的内层滚动区域');
    assert(getComputedStyle(document.getElementById('main-content')!).overflowY==='auto','主页面未恢复整体滚动');
    await emit('network-state-change',{state:'BjutCampus',loginType:'lgn-wired',transport:'wifi',ssid:'测试校园路由器',ip:'192.168.1.2',systemOnline:true});
    assert(!document.getElementById('btn-login')!.hidden && document.getElementById('btn-logout-current')!.hidden,'VPN 联网覆盖了待认证 Wi-Fi 状态');
    note('通过：完整页面滚动，系统联网与 Wi-Fi 待认证分别显示');
    const {setupAndroidKeyboard} = await import('../src/android-keyboard');
    document.body.classList.add('is-android'); setupAndroidKeyboard();
    window.__nativeKeyboardChanged?.(true,380);
    await new Promise<void>(resolve=>requestAnimationFrame(()=>resolve()));
    assert(getComputedStyle(document.getElementById('nav')!).display==='none','键盘打开后底栏仍显示');
    assert(document.documentElement.style.getPropertyValue('--app-viewport-height')==='380px','键盘可视高度未应用');
    window.__nativeKeyboardChanged?.(false,800);
    await new Promise<void>(resolve=>requestAnimationFrame(()=>resolve()));
    assert(!document.body.classList.contains('keyboard-open'),'键盘关闭后未恢复布局');
    if (!mobileLayout) document.body.classList.remove('is-android');
    note('通过：Android 键盘事件隐藏底栏并恢复布局');
    const {runMotionRegression}=await import('./motion-regression');await runMotionRegression(note);
    note('全部通过');
  }catch(error){note('失败：'+String(error));}finally{button.disabled=false;}
});
