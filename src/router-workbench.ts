import { invoke } from '@tauri-apps/api/core';
import { DeviceUsageView } from './device-usage-view';
import { recordRouterQuality } from './network-quality';
import type { UsageReport } from './device-usage-model';
import { RouterAdmin } from './router-admin';
import { renderRouterBillingSummary } from './router-billing-summary';
import { RouterBillingCenter } from './router-billing-center';
import { CustomSelect } from './custom-select';
import { animateResize, setVisible } from './layout-motion';
import { routerPortIcon } from './router-port-icon';
import type { DiagnosticStep } from './models';
import { routerBytes, routerCounter, routerStatus, routerAdvice } from './router-model';
import type { RouterSnapshot, RouterCandidate, RouterUplink } from './router-model';
import { setBillingSource, setRouterBillingBridge } from './router-billing-bridge';

const byId = (id: string) => document.getElementById(id)!;
function node(tag: string, text = '', className = ''): HTMLElement {
  const element = document.createElement(tag); element.textContent = text; element.className = className; return element;
}
function button(text: string, action: () => void, className = 'btn btn-secondary btn-sm'): HTMLButtonElement {
  const element = node('button', text, className) as HTMLButtonElement; element.type = 'button'; element.addEventListener('click', action); return element;
}

export class RouterWorkbench {
  snapshot: RouterSnapshot | null = null;
  billingSelected = false;
  private candidates: RouterCandidate[] = [];
  private discoveryBusy = false;
  private generation = 0;
  private lastLogsPoll = 0;
  private accountsStamp = '';
  private admin: RouterAdmin;
  private billingCenter: RouterBillingCenter;
  private ports = new Map<string, HTMLElement>();
  private portAccounts = new Map<string, CustomSelect>();
  private samples = new Map<string, {at: number; tx: bigint; rx: bigint}>();
  private logSnapshot = '';
  private logsBusy = false;
  private disconnected=false;
  private receivedAt=Date.now();
  private trafficView=new DeviceUsageView(byId('router-device-usage-panel'),'router',async(enabled)=>{await this.request('save_settings',{traffic_monitor:enabled?'1':'0'});this.admin.invalidateSettings();await this.refreshTraffic();},()=>{void this.refreshTraffic();});
  private trafficBusy=false;
  private lastTraffic=0;

  constructor(private options: {
    connect: (address?: string) => void;
    refresh: () => Promise<void>;
    login: (uplink: string, account?: string) => Promise<void>;
    log: (message: string) => void;
    confirm: (message:string,title:string,label:string)=>Promise<boolean>;
  }) {
    for (const id of ['btn-router-discover','btn-router-settings-discover']) byId(id).addEventListener('click', () => { void this.discover(); });
    byId('btn-router-auto-connect').addEventListener('click', () => options.connect(this.candidates[0]?.address));
    byId('btn-router-manual-connect').addEventListener('click', () => options.connect());
    byId('btn-billing-source-router').addEventListener('click', () => this.selectBillingSource(true));
    byId('btn-billing-source-local').addEventListener('click', () => this.selectBillingSource(false));
    byId('btn-router-diagnostics-refresh').addEventListener('click', () => { void options.refresh(); });
    byId('btn-router-logs-refresh').addEventListener('click', () => { void this.refreshLogs(); });
    this.admin = new RouterAdmin({request:<T>(name:string,args?:Record<string,unknown>)=>this.request<T>(name,args),refresh:options.refresh,confirm:options.confirm,log:options.log});
    this.billingCenter = new RouterBillingCenter(
      <T>(name:string,args?:Record<string,unknown>)=>this.request<T>(name,args),
      () => this.disconnected ? null : this.snapshot,
      jobs => renderRouterBillingSummary(byId('router-billing-summary-content'),jobs,this.snapshot),
    );
    setRouterBillingBridge(this.billingCenter);
    window.setInterval(() => {
      if (document.hidden || !this.snapshot) return;
      const page = document.querySelector('.page.active')?.id;
      if(page==='dashboard'&&!this.disconnected&&this.supports('traffic_charts')&&Date.now()-this.lastTraffic>=5000)void this.refreshTraffic();
      if (page === 'logs' && Date.now() - this.lastLogsPoll >= 5000) void this.refreshLogs();
    },1000);
    window.addEventListener('focus', () => { if (!this.snapshot && !this.discoveryBusy) void this.discover(); });
    void this.discover();
  }

  async discover(): Promise<void> {
    if (this.discoveryBusy || this.snapshot) return;
    this.discoveryBusy = true;
    byId('router-settings-connection').textContent = '正在查找局域网路由器…';
    try {
      const result = await invoke<RouterCandidate[]>('discover_routers', {lastAddress: localStorage.getItem('bjut_router_address')});
      this.candidates = Array.isArray(result) ? result : [];
      if (this.snapshot) return;
      const found = this.candidates[0];
      setVisible(byId('router-dashboard'), !!found);
      byId('router-discovery-message').textContent = found ? `${found.name} · ${new URL(found.address).host}` : '';
      byId('router-settings-connection').textContent = found ? `发现 ${new URL(found.address).host}，可在控制台连接。` : '未发现路由器，可手动填写地址连接。';
    } catch { if (!this.snapshot) byId('router-settings-connection').textContent = '暂未发现路由器，可使用手动连接。'; }
    finally { this.discoveryBusy = false; }
  }

  render(data: RouterSnapshot): void {
    const reconnect=this.disconnected;
    this.disconnected=false;this.receivedAt=Date.now();
    this.setControlsAvailable(true);
    const firstConnection = !this.snapshot;
    this.snapshot = data; this.admin.update(data);
    setVisible(byId('router-device-usage-panel'),this.supports('traffic_charts'));
    if(firstConnection){this.lastTraffic=0;void this.refreshTraffic();}
    if(reconnect)this.admin.resume();
    for (const id of ['router-dashboard','router-accounts-panel','router-diagnostics-panel','router-logs-panel','router-billing-source','dashboard-local-label','router-local-accounts-title']) setVisible(byId(id), true);
    byId('btn-router-auto-connect').hidden = true;
    byId('btn-router-discover').hidden = true;
    byId('router-discovery-message').textContent = new URL(data.managementUrl).host;
    byId('router-settings-connection').textContent = `已连接 ${new URL(data.managementUrl).host}`;
    const stamp = JSON.stringify(data.accounts);
    if (stamp !== this.accountsStamp) {
      this.accountsStamp = stamp;
      this.renderAccounts();
    }
    this.renderPorts(); this.renderDiagnostics();
    setVisible(byId('router-settings-form'), this.supports('settings'));
    if (firstConnection) this.selectBillingSource(true);
    if(reconnect && this.billingSelected && document.querySelector('.page.active')?.id==='billing-center') {
      const summary=!byId('router-billing-summary-content').hidden;
      void (summary?this.billingCenter.refreshSummary():this.billingCenter.refresh()).catch(error=>this.options.log(`路由计费：${String(error)}`));
    }
    this.onPage(document.querySelector('.page.active')?.id || 'dashboard');
  }

  disconnect(): void {
    setVisible(byId('router-device-usage-panel'),false);this.trafficView.render([],'已断开路由器');this.lastTraffic=0;
    this.billingCenter.invalidate();
    this.generation++; this.snapshot = null;
    this.accountsStamp = ''; this.admin.disconnect(); this.logSnapshot = '';
    this.portAccounts.forEach(select=>select.close());this.portAccounts.clear();
    this.ports.clear(); this.samples.clear(); byId('router-uplink-status').replaceChildren();
    for (const id of ['router-accounts-panel','router-diagnostics-panel','router-logs-panel','router-billing-source','router-settings-form','dashboard-local-label','router-local-accounts-title']) setVisible(byId(id), false);
    this.selectBillingSource(false);
    byId('btn-router-auto-connect').hidden = false; byId('btn-router-discover').hidden = false;
    byId('router-settings-connection').textContent = '已断开，可重新查找或手动连接。';
    setVisible(byId('router-dashboard'), !!this.candidates.length);
    byId('router-discovery-message').textContent = this.candidates[0]?.name || '';
  }

  markDisconnected(reason:string):void {
    if(!this.snapshot)return;
    if(!this.disconnected)this.billingCenter.invalidate();
    this.disconnected=true;
    this.trafficView.render([],'连接已中断，重新连接后恢复统计。');
    if(!byId('router-billing-summary-content').hidden)byId('router-billing-summary-content').replaceChildren(node('p','路由连接已中断，请等待重新连接。'));
    this.updatePorts();this.setControlsAvailable(false);
    byId('router-settings-connection').textContent='连接已中断，以下为上次读取的数据。';
    byId('router-discovery-message').textContent=reason;
    byId('btn-router-auto-connect').hidden=false;
  }
  private setControlsAvailable(available:boolean) {
    for(const id of ['router-uplink-status','router-accounts-panel','router-settings-form']) {
      byId(id).querySelectorAll<HTMLButtonElement|HTMLInputElement>('button,input').forEach(control=>{
        if(!available){control.dataset.wasDisabled=String(control.disabled);control.disabled=true;}
        else if(control.dataset.wasDisabled!==undefined){control.disabled=control.dataset.wasDisabled==='true';delete control.dataset.wasDisabled;}
      });
    }
  }

  private supports(capability: string): boolean { return this.snapshot?.capabilities?.includes(capability) || false; }
  private async refreshTraffic():Promise<void> {
    if(this.trafficBusy||!this.snapshot||this.disconnected||!this.supports('traffic_charts'))return;
    this.trafficBusy=true;this.lastTraffic=Date.now();const router=this.snapshot.managementUrl;
    try{const data=await this.request<UsageReport>('traffic',{...this.trafficView.query()});if(this.snapshot?.managementUrl===router&&!this.disconnected)this.trafficView.update(data);}
    catch{if(this.snapshot?.managementUrl===router&&!this.disconnected)this.trafficView.showError('暂时无法读取路由器设备流量。');}
    finally{this.trafficBusy=false;}
  }
  private async request<T>(operation: string, args: Record<string,unknown> = {}): Promise<T> {
    if(this.disconnected)throw new Error('路由连接已中断，请等待重新连接。');
    return invoke<T>('router_request',{operation,args});
  }
  private renderPorts(): void {
    animateResize(byId('router-uplink-status'), () => this.updatePorts());
  }
  private updatePorts(): void {
    if (!this.snapshot) return;
    const live = new Set(this.snapshot.uplinks.map(item=>item.id));
    for (const [id,card] of this.ports) if (!live.has(id)) { this.portAccounts.get(id)?.close();this.portAccounts.delete(id);card.remove(); this.ports.delete(id); this.samples.delete(id); }
    for (const item of this.snapshot.uplinks) {
      if(!item.stale)recordRouterQuality(item.device||item.id,(item.updated_at||0)*1000,item.internet4,item.internet6);
      let card = this.ports.get(item.id);
      if (!card) {
        card = node('article','', 'router-port-card router-enter'); card.dataset.uplink = item.id;
        const title = node('strong','','router-port-title'); title.dataset.field = 'title';
        const icon = node('div','','router-port-icon'); icon.innerHTML = routerPortIcon;
        const speed = node('span','','router-port-speed'); speed.dataset.field = 'speed';
        const paths = node('div','','router-port-paths');
        for (const family of ['ipv4','ipv6']) { const pill=node('span','','router-path'); pill.dataset.field=family; paths.append(pill); }
        const state=node('strong','','router-port-state'); state.dataset.field='state';
        const account=node('p','','router-port-account'); account.dataset.field='account';
        const progress=node('p','','router-port-progress'); progress.dataset.field='progress';
        const traffic=node('div','','router-port-traffic');
        for (const field of ['tx','rx']) { const row=node('span'); row.dataset.field=field; traffic.append(row); }
        const detail=node('details','','router-port-details'); detail.append(node('summary','网络详情'));
        const description=node('p'); description.dataset.field='description'; detail.append(description);
        const choice=node('div','','custom-select');choice.id=`router-port-account-${item.id}`;choice.setAttribute('aria-label',`${item.device || item.id} 登录账号`);
        choice.innerHTML='<div class="custom-select-trigger"><span></span><span aria-hidden="true">⌄</span></div><div class="custom-select-options"></div>';
        const tools=node('div','','router-port-actions');
        const login=button('登录',()=>{ void this.options.login(item.id,this.portAccounts.get(item.id)?.value); },'btn btn-secondary btn-sm router-port-login');
        const change=button('切换账号',()=>{void this.portAction(item.id,'switch_account');});
        const logout=button('注销',()=>{void this.portAction(item.id,'logout');});
        change.dataset.requires=logout.dataset.requires='uplink_actions';tools.append(login,change,logout);
        card.append(title,icon,speed,paths,state,account,progress,traffic,choice,tools,detail);
        this.ports.set(item.id,card); byId('router-uplink-status').append(card);
        this.portAccounts.set(item.id,new CustomSelect(choice.id));
      }
      const selector=this.portAccounts.get(item.id)!;
      const stamp=JSON.stringify(this.snapshot.accounts);
      if(card.dataset.accounts!==stamp){const previous=selector.value;selector.setOptions([{value:'',text:'自动选择账号'},...this.snapshot.accounts.filter(a=>a.hasPassword).map(a=>({value:a.id,text:a.user+(a.enabled?'':'（手动）')}))]);selector.value=this.snapshot.accounts.some(a=>a.id===previous)?previous:'';card.dataset.accounts=stamp;}
      card.querySelectorAll<HTMLElement>('[data-requires]').forEach(element=>element.hidden=!this.supports(element.dataset.requires!));
      const field=(key:string)=>card!.querySelector<HTMLElement>(`[data-field="${key}"]`)!;
      const port=this.snapshot.ports?.find(port=>port.uplink===item.id);
      field('title').textContent = item.device || port?.device || item.network || item.id;
      const speed=Number(port?.speedMbps || 0);
      field('speed').textContent = port?.linkUp === 0 ? '已断开' : speed ? (speed>=1000 ? `${speed/1000} GbE` : `${speed} Mbps`) : item.transport==='wifi'?'Wi-Fi':'有线';
      for (const [family,value] of [['ipv4',item.internet4],['ipv6',item.internet6]]) {
        const stale=routerStatus(item,this.disconnected)==='数据已过期';
        const element=field(family!); element.textContent=`${stale?'上次 ':''}${value==='reachable'?'✓':value==='unreachable'?'×':'—'} ${family==='ipv4'?'IPv4':'IPv6'}`;
        element.classList.toggle('success',!stale && value==='reachable'); element.classList.toggle('error',!stale && value==='unreachable');
      }
      field('state').textContent = routerStatus(item,this.disconnected,(Date.now()-this.receivedAt)/1000);
      field('account').textContent = item.account ? `账号 ${item.account}` : '暂无认证账号';
      field('progress').textContent = item.checking ? item.check_message || '正在检测…' : ['queued','running'].includes(item.operation?.state || '') ? item.operation?.phase || '正在处理…' : item.message || '';
      field('description').textContent = [item.network,item.network6,item.ipv4,item.ipv6,`IPv6 前缀：${item.ipv6_pd || '未委派'}`,`认证：${item.session==='online'?'已认证':item.session==='offline'?'未认证':'待确认'}`,item.message,routerAdvice(item),item.updated_at?`检测时间：${new Date(item.updated_at*1000).toLocaleString()}`:''].filter(Boolean).join('\n');
      const now=performance.now(),tx=routerCounter(port?.txBytes),rx=routerCounter(port?.rxBytes),previous=this.samples.get(item.id);
      const elapsed=previous?(now-previous.at)/1000:0;
      const rate=(current:bigint,last:bigint)=>elapsed>.2 && current>=last ? Number(current-last)/elapsed : 0;
      field('tx').textContent=port?.txBytes===undefined?'▲ --':`▲ ${routerBytes(tx)} · ${routerBytes(previous?rate(tx,previous.tx):0)}/s`;
      field('rx').textContent=port?.rxBytes===undefined?'▼ --':`▼ ${routerBytes(rx)} · ${routerBytes(previous?rate(rx,previous.rx):0)}/s`;
      this.samples.set(item.id,{at:now,tx,rx});
    }
  }

  private async portAction(uplink:string,operation:'logout'|'switch_account') {
    const account=this.portAccounts.get(uplink)?.value || '';
    if(operation==='switch_account' && !account){this.ports.get(uplink)!.querySelector('[data-field="progress"]')!.textContent='请先选择要切换的账号';return;}
    const description=operation==='logout'?'注销该上游并暂停自动登录？再次点击登录可恢复。':`将该上游切换到账号 ${this.snapshot?.accounts.find(a=>a.id===account)?.user || ''}？`;
    if(!await this.options.confirm(description,operation==='logout'?'注销上游':'切换账号','继续'))return;
    try{await this.request(operation,{uplink,account});this.options.log(operation==='logout'?'已提交上游注销':'已提交上游账号切换');await this.options.refresh();}
    catch(error){this.ports.get(uplink)?.querySelector('[data-field="progress"]')?.replaceChildren(document.createTextNode(String(error)));}
  }

  private renderAccounts(): void { this.admin.renderAccounts(); }

  onPage(page: string): void {
    if (!this.snapshot) return;
    if (page==='logs' && Date.now()-this.lastLogsPoll>=5000) void this.refreshLogs();
    if (page==='settings') void this.admin.load();
  }

  selectBillingSource(router: boolean): void {
    this.billingSelected = router && !!this.snapshot;
    byId('billing-center').classList.toggle('is-router-source', this.billingSelected);
    byId('btn-billing-source-router').setAttribute('aria-pressed',String(this.billingSelected));
    byId('btn-billing-source-local').setAttribute('aria-pressed',String(!this.billingSelected));
    byId('billing-source-current').textContent = this.billingSelected ? `当前：路由器 · ${new URL(this.snapshot!.managementUrl).host}` : '当前：本机查询';
    setBillingSource(this.billingSelected ? 'router' : 'local');
  }

  private renderDiagnostics(): void {
    if (!this.snapshot) return;
    const uplinks=this.snapshot.uplinks;
    animateResize(byId('router-diagnostics-results'),()=>byId('router-diagnostics-results').replaceChildren(...uplinks.map(item=>{
      const row=node('div','','router-diagnostic-row');row.append(node('strong',[item.device,item.network].filter(Boolean).join(' · ') || item.id),node('p',this.diagnosticMessage(item)));return row;
    })));
  }
  private diagnosticMessage(item: RouterUplink): string {
    const family=(name:string,state?:string)=>`${name} ${state==='reachable'?'通过':state==='unreachable'?'未通过':state==='not_configured'?'未取得地址':'待检测'}`;
    const route=(value?:string)=>({managed:'固定出口',kernel:'系统路由',missing_default:'缺少默认路由',failed:'出口未就绪',not_configured:'未取得地址'}[value || ''] || '待检查');
    return [routerStatus(item,this.disconnected),`认证会话：${item.session==='online'?'已认证':item.session==='offline'?'未认证':'待确认'}`,`${family('IPv4',item.internet4)} · ${family('IPv6',item.internet6)}`,`IPv4 ${route(item.route4)} · IPv6 ${route(item.route6)}`,item.message,routerAdvice(item),item.updated_at?`更新于 ${new Date(item.updated_at*1000).toLocaleTimeString()}`:'尚无检测结果'].filter(Boolean).join('\n');
  }
  async diagnosticSteps(): Promise<DiagnosticStep[]> {
    if (!this.snapshot) return [];
    try { await this.options.refresh(); }
    catch { /* Keep explicitly dated last results available. */ }
    return this.snapshot?.uplinks.map(item=>({id:`router_${item.id}`,label:`路由上游 ${item.device || item.network || item.id}`,status:routerStatus(item,this.disconnected)==='互联网已连接'?'success':'warning',message:this.diagnosticMessage(item),durationMs:0})) || [];
  }
  jointDiagnosticStep(localHealthy:boolean):DiagnosticStep|null {
    if(!this.snapshot)return null;
    const live=this.snapshot.uplinks.filter(item=>routerStatus(item,this.disconnected)!=='数据已过期');
    const online=live.filter(item=>item.state==='online').length;
    const message=!live.length?'路由器结果已过期，请恢复协作连接后重新诊断。':online && !localHealthy?'路由器存在可用上游，当前设备探测异常；请优先检查本机 VPN、DNS 和局域网连接。':!online && localHealthy?'当前设备联网正常，路由上游尚未确认可用；可能使用了其他网络出口，请检查各上游认证与路由。':online?`${online} 条路由上游可用，当前设备联网正常。`:'当前设备与路由上游均未确认联网，请先检查上游接口和校园网认证。';
    return {id:'router_joint',label:'本机与路由器联合判断',status:online && localHealthy?'success':'warning',message,durationMs:0};
  }
  diagnosticBundle():unknown {
    if(!this.snapshot)return null;
    let logs=this.logSnapshot;
    for(const account of this.snapshot.accounts)if(account.user)logs=logs.split(account.user).join('[账号]');
    for(const item of this.snapshot.uplinks)for(const value of [item.account,item.ssid,item.bssid,item.ipv6])if(value)logs=logs.split(value).join('[网络标识]');
    logs=logs.replace(/\b(?:\d{1,3}\.){3}\d{1,3}\b/g,'[IPv4]').replace(/\b(?:[0-9a-f]{0,4}:){3,}[0-9a-f:]{0,39}\b/gi,'[IPv6/BSSID]').replace(/\b[0-9a-f]{1,4}::[0-9a-f:]+\b/gi,'[IPv6]');
    return {version:this.snapshot.version,disconnected:this.disconnected,recordedAt:new Date(this.receivedAt).toISOString(),uplinks:this.snapshot.uplinks.map(item=>({id:item.id,state:item.state,session:item.session,internet4:item.internet4,internet6:item.internet6,route4:item.route4,route6:item.route6,stale:item.stale,ageSeconds:item.age_seconds,updatedAt:item.updated_at,operationState:item.operation?.state,advice:routerAdvice(item)})),logs:logs.split('\n').slice(-200)};
  }

  async refreshLogs(): Promise<void> {
    if (!this.snapshot || this.logsBusy) return;
    if (!this.supports('logs')) {byId('router-logs-content').textContent='请更新路由端后查看日志。';return;}
    const generation=this.generation;this.lastLogsPoll=Date.now();this.logsBusy=true;
    try {
      const response=await this.request<{text:string}>('logs');
      if (generation===this.generation && this.snapshot) {this.logSnapshot=response.text || '';byId('router-logs-content').textContent=this.logSnapshot || '暂无路由器日志。';}
    } catch(error) {if (generation===this.generation) byId('router-logs-content').textContent=String(error);}
    finally {this.logsBusy=false;}
  }
  logText(): string {return this.logSnapshot;}

}
