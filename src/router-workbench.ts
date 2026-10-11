import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { CustomSelect } from './custom-select';
import { routerPortIcon } from './router-port-icon';
import type { DiagnosticStep } from './models';
import { routerLabels, routerBytes, routerCounter, routerBillingSummary } from './router-model';
import type { RouterSnapshot, RouterCandidate, RouterBillingJob, RouterBillingItem, RouterUplink } from './router-model';

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
  private billingGeneration = 0;
  private billingJobs: RouterBillingJob[] = [];
  private billingPolling = false;
  private lastBillingPoll = 0;
  private lastLogsPoll = 0;
  private settingsLoaded = false;
  private settingsLoading = false;
  private optionsStamp = '';
  private ports = new Map<string, HTMLElement>();
  private samples = new Map<string, {at: number; tx: bigint; rx: bigint}>();
  private billingUplink = new CustomSelect('router-billing-uplink');
  private billingAccount = new CustomSelect('router-billing-account');
  private logSnapshot = '';
  private logsBusy = false;

  constructor(private options: {
    connect: (address?: string) => void;
    refresh: () => Promise<void>;
    login: (uplink: string, account?: string) => Promise<void>;
    log: (message: string) => void;
  }) {
    for (const id of ['btn-router-discover','btn-router-settings-discover']) byId(id).addEventListener('click', () => { void this.discover(); });
    byId('btn-router-auto-connect').addEventListener('click', () => options.connect(this.candidates[0]?.address));
    byId('btn-router-manual-connect').addEventListener('click', () => options.connect());
    byId('btn-billing-source-router').addEventListener('click', () => this.selectBillingSource(true));
    byId('btn-billing-source-local').addEventListener('click', () => this.selectBillingSource(false));
    byId('btn-router-billing-refresh').addEventListener('click', () => { void this.refreshBilling(); });
    for (const select of [this.billingUplink,this.billingAccount]) select.addEventListener('change', () => { void this.refreshBilling(); });
    byId('btn-router-diagnostics-refresh').addEventListener('click', () => { void options.refresh(); });
    byId('btn-router-logs-refresh').addEventListener('click', () => { void this.refreshLogs(); });
    byId('router-settings-form').addEventListener('submit', event => { event.preventDefault(); void this.saveSettings(); });
    byId('btn-router-settings-advanced').addEventListener('click', () => {
      if (this.snapshot) {
        const url = new URL(this.snapshot.managementUrl); url.pathname = '/cgi-bin/luci/admin/network/bjut-autologin/config';
        void openUrl(url.toString());
      }
    });
    window.setInterval(() => {
      if (document.hidden || !this.snapshot) return;
      const page = document.querySelector('.page.active')?.id;
      if (page === 'billing-center' && Date.now() - this.lastBillingPoll >= 2000) void this.pollBilling();
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
      byId('router-dashboard').hidden = !found;
      byId('router-discovery-message').textContent = found ? `${found.name} · ${new URL(found.address).host}` : '';
      byId('router-settings-connection').textContent = found ? `发现 ${new URL(found.address).host}，可在控制台连接。` : '未发现路由器，可手动填写地址连接。';
    } catch { if (!this.snapshot) byId('router-settings-connection').textContent = '暂未发现路由器，可使用手动连接。'; }
    finally { this.discoveryBusy = false; }
  }

  render(data: RouterSnapshot): void {
    const firstConnection = !this.snapshot;
    this.snapshot = data;
    for (const id of ['router-dashboard','router-accounts-panel','router-diagnostics-panel','router-logs-panel','router-billing-source','dashboard-local-label','router-local-accounts-title']) byId(id).hidden = false;
    byId('btn-router-auto-connect').hidden = true;
    byId('btn-router-discover').hidden = true;
    byId('router-discovery-message').textContent = new URL(data.managementUrl).host;
    byId('router-settings-connection').textContent = `已连接 ${new URL(data.managementUrl).host}`;
    const stamp = JSON.stringify([data.accounts,data.uplinks.map(item => [item.id,item.network])]);
    if (stamp !== this.optionsStamp) {
      this.optionsStamp = stamp;
      const previousUplink = this.billingUplink.value || 'all', previousAccount = this.billingAccount.value || 'current';
      this.billingUplink.setOptions([{value:'all',text:'所有上游'},...data.uplinks.map(item=>({value:item.id,text:[item.device,item.network].filter(Boolean).join(' · ') || item.id}))]);
      this.billingUplink.value = previousUplink === 'all' || data.uplinks.some(item=>item.id === previousUplink) ? previousUplink : 'all';
      this.billingAccount.setOptions([{value:'current',text:'当前认证账号'},...data.accounts.filter(item=>item.hasPassword).map(item=>({value:item.id,text:item.user+(item.enabled?'':'（手动）')}))]);
      this.billingAccount.value = previousAccount === 'current' || data.accounts.some(item=>item.id === previousAccount) ? previousAccount : 'current';
      this.renderAccounts();
    }
    this.renderPorts(); this.renderDiagnostics();
    byId('router-settings-form').hidden = !this.supports('settings');
    if (firstConnection) this.selectBillingSource(true, false);
    this.onPage(document.querySelector('.page.active')?.id || 'dashboard');
  }

  disconnect(): void {
    this.generation++; this.billingGeneration++; this.snapshot = null; this.billingJobs = [];
    this.optionsStamp = ''; this.settingsLoaded = false; this.settingsLoading = false; this.logSnapshot = '';
    this.ports.clear(); this.samples.clear(); byId('router-uplink-status').replaceChildren();
    for (const id of ['router-accounts-panel','router-diagnostics-panel','router-logs-panel','router-billing-source','router-settings-form','dashboard-local-label','router-local-accounts-title']) byId(id).hidden = true;
    this.selectBillingSource(false, false);
    byId('btn-router-auto-connect').hidden = false; byId('btn-router-discover').hidden = false;
    byId('router-settings-connection').textContent = '已断开，可重新查找或手动连接。';
    byId('router-dashboard').hidden = !this.candidates.length;
    byId('router-discovery-message').textContent = this.candidates[0]?.name || '';
  }

  private supports(capability: string): boolean { return this.snapshot?.capabilities?.includes(capability) || false; }
  private async request<T>(operation: string, args: Record<string,unknown> = {}): Promise<T> {
    return invoke<T>('router_request',{operation,args});
  }
  private renderPorts(): void {
    if (!this.snapshot) return;
    const live = new Set(this.snapshot.uplinks.map(item=>item.id));
    for (const [id,card] of this.ports) if (!live.has(id)) { card.remove(); this.ports.delete(id); this.samples.delete(id); }
    for (const item of this.snapshot.uplinks) {
      let card = this.ports.get(item.id);
      if (!card) {
        card = node('article','', 'router-port-card'); card.dataset.uplink = item.id;
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
        const login=button('登录',()=>{ void this.options.login(item.id); },'btn btn-secondary btn-sm router-port-login');
        card.append(title,icon,speed,paths,state,account,progress,traffic,login,detail);
        this.ports.set(item.id,card); byId('router-uplink-status').append(card);
      }
      const field=(key:string)=>card!.querySelector<HTMLElement>(`[data-field="${key}"]`)!;
      const port=this.snapshot.ports?.find(port=>port.uplink===item.id);
      field('title').textContent = item.device || port?.device || item.network || item.id;
      const speed=Number(port?.speedMbps || 0);
      field('speed').textContent = port?.linkUp === 0 ? '已断开' : speed ? (speed>=1000 ? `${speed/1000} GbE` : `${speed} Mbps`) : item.transport==='wifi'?'Wi-Fi':'有线';
      for (const [family,value] of [['ipv4',item.internet4],['ipv6',item.internet6]]) {
        const element=field(family!); element.textContent=`${value==='reachable'?'✓':value==='unreachable'?'×':'—'} ${family==='ipv4'?'IPv4':'IPv6'}`;
        element.classList.toggle('success',value==='reachable'); element.classList.toggle('error',value==='unreachable');
      }
      field('state').textContent = routerLabels[item.state] || item.state;
      field('account').textContent = item.account ? `账号 ${item.account}` : '暂无认证账号';
      field('progress').textContent = item.checking ? item.check_message || '正在检测…' : '';
      field('description').textContent = [item.network,item.network6,item.ipv4,item.ipv6,item.message].filter(Boolean).join('\n');
      const now=performance.now(),tx=routerCounter(port?.txBytes),rx=routerCounter(port?.rxBytes),previous=this.samples.get(item.id);
      const elapsed=previous?(now-previous.at)/1000:0;
      const rate=(current:bigint,last:bigint)=>elapsed>.2 && current>=last ? Number(current-last)/elapsed : 0;
      field('tx').textContent=port?.txBytes===undefined?'▲ --':`▲ ${routerBytes(tx)} · ${routerBytes(previous?rate(tx,previous.tx):0)}/s`;
      field('rx').textContent=port?.rxBytes===undefined?'▼ --':`▼ ${routerBytes(rx)} · ${routerBytes(previous?rate(rx,previous.rx):0)}/s`;
      this.samples.set(item.id,{at:now,tx,rx});
    }
  }

  private renderAccounts(): void {
    if (!this.snapshot) return;
    byId('router-accounts-count').textContent = `${this.snapshot.accounts.length} 个账号`;
    byId('router-accounts-list').replaceChildren(...this.snapshot.accounts.map(item=>{
      const row=node('div','','router-account-row');
      const text=node('div'); text.append(node('strong',item.user),node('p',item.enabled?'参与自动登录':'仅手动使用'));
      const control=button(item.enabled?'停用自动登录':'启用自动登录',()=>{ void toggle(); });
      control.disabled=!this.supports('account_set');
      const toggle=async()=>{
        control.disabled=true; const generation=this.generation;
        try { await this.request('account_set',{account:item.id,enabled:item.enabled?'0':'1'}); if (generation===this.generation) await this.options.refresh(); }
        catch(error) { if (generation===this.generation) control.textContent=String(error).slice(0,80); }
        finally { if (generation===this.generation) control.disabled=false; }
      };
      row.append(text,control); return row;
    }));
  }

  onPage(page: string): void {
    if (!this.snapshot) return;
    if (page==='billing-center' && this.billingSelected && !this.billingJobs.length) void this.refreshBilling();
    if (page==='logs' && Date.now()-this.lastLogsPoll>=5000) void this.refreshLogs();
    if (page==='settings' && !this.settingsLoaded && !this.settingsLoading) void this.loadSettings();
  }

  selectBillingSource(router: boolean, load=true): void {
    this.billingSelected = router && !!this.snapshot;
    const local=byId('billing-center').querySelector<HTMLElement>('.billing-workbench')!;
    local.hidden=this.billingSelected;
    byId('billing-center').querySelector<HTMLElement>('.billing-header-tools')!.hidden=this.billingSelected;
    byId('router-billing-panel').hidden=!this.billingSelected;
    byId('btn-billing-source-router').setAttribute('aria-pressed',String(this.billingSelected));
    byId('btn-billing-source-local').setAttribute('aria-pressed',String(!this.billingSelected));
    if (load && this.billingSelected) void this.refreshBilling();
    if (load && !this.billingSelected) window.dispatchEvent(new Event('router-billing-local-selected'));
  }

  async refreshBilling(): Promise<void> {
    if (!this.snapshot || !this.billingSelected || this.billingJobs.some(job=>job.running || job.starting)) return;
    if (!this.supports('billing')) { byId('router-billing-message').textContent='请更新路由端后使用计费汇总。'; return; }
    const generation=++this.billingGeneration,connection=this.generation;
    const uplinks=this.billingUplink.value==='all'?this.snapshot.uplinks:this.snapshot.uplinks.filter(item=>item.id===this.billingUplink.value);
    this.billingJobs=uplinks.map(item=>({uplink:item.id,account:this.billingAccount.value || 'current',ticket:'',running:false,starting:true,item:{state:'loading',message:'正在准备查询',percent:2}}));
    this.renderBilling();
    await Promise.all(this.billingJobs.map(async job=>{
      try {
        const result=await this.request<{ticket:string}>('billing_start',{uplink:job.uplink,account:job.account});
        if (generation!==this.billingGeneration || connection!==this.generation) return;
        job.ticket=result.ticket; job.running=true;
      } catch(error) { if (generation===this.billingGeneration && connection===this.generation) job.item={state:'error',message:String(error)}; }
      finally { if (generation===this.billingGeneration && connection===this.generation) job.starting=false; }
    }));
    if (generation===this.billingGeneration && connection===this.generation) { this.renderBilling(); await this.pollBilling(); }
  }

  private async pollBilling(): Promise<void> {
    if (this.billingPolling || !this.snapshot || !this.billingJobs.some(job=>job.running && job.ticket)) return;
    this.billingPolling=true; this.lastBillingPoll=Date.now();
    const generation=this.billingGeneration,connection=this.generation;
    try {
      await Promise.all(this.billingJobs.filter(job=>job.running && job.ticket).map(async job=>{
        try {
          const response=await this.request<{result:RouterBillingItem;sessions?:Record<string,string>[];sessionsAvailable?:boolean}>('billing_poll',{ticket:job.ticket});
          if (generation!==this.billingGeneration || connection!==this.generation) return;
          job.item=response.result; job.sessions=response.sessions; job.sessionsAvailable=response.sessionsAvailable;
          job.running=response.result.state==='loading';
        } catch(error) { if (generation===this.billingGeneration && connection===this.generation) { job.running=false; job.item={state:'error',message:String(error)}; } }
      }));
      if (generation===this.billingGeneration && connection===this.generation) this.renderBilling();
    } finally { this.billingPolling=false; }
  }

  private renderBilling(): void {
    const busy=this.billingJobs.some(job=>job.running || job.starting);
    this.billingUplink.setDisabled(busy); this.billingAccount.setDisabled(busy);
    (byId('btn-router-billing-refresh') as HTMLButtonElement).disabled=busy;
    byId('router-billing-message').textContent=busy?'正在查询各条上游…':'路由器查询已完成';
    const summary=routerBillingSummary(this.billingJobs);
    const metric=(label:string,value:string)=>{const card=node('article','','glass-card router-billing-metric');card.append(node('span',label),node('strong',value));return card;};
    byId('router-billing-summary').replaceChildren(metric('已查询账号',String(summary.accounts)),metric('余额合计',summary.balance===null?'--':`${summary.balance.toFixed(2)} 元`));
    const container=byId('router-billing-results');
    for (const job of this.billingJobs) {
      let card=Array.from(container.children).find(child=>(child as HTMLElement).dataset.uplink===job.uplink) as HTMLElement | undefined;
      if (!card) {card=node('article','','glass-card router-billing-card');card.dataset.uplink=job.uplink;container.append(card);}
      if (job.item.state==='captcha' && card.dataset.captcha===job.item.image) continue;
      card.dataset.captcha=job.item.state==='captcha'?job.item.image || '':'';
      const item=this.snapshot?.uplinks.find(item=>item.id===job.uplink);
      const title=node('h3',[item?.device,item?.network].filter(Boolean).join(' · ') || job.uplink);
      card.replaceChildren(title,node('p',job.item.message || '等待查询','router-billing-status'));
      if (job.item.state==='loading') {const progress=document.createElement('progress');progress.max=100;progress.value=job.item.percent || 2;card.append(progress);}
      if (job.item.account) {
        const fields=[['账号',job.item.account],['余额',job.item.balance?.trim()?`${Number(job.item.balance).toFixed(2)} 元`:'--'],['剩余流量',job.item.remainingFlow?.trim()?`${(Number(job.item.remainingFlow)/1024).toFixed(2)} GB`:'--'],['状态',job.item.accountStatus || '--'],['套餐',job.item.package || '--']];
        for (const [label,value] of fields) {const row=node('p','','router-billing-row');row.append(node('span',label),node('strong',value));card.append(row);}
      }
      if (job.item.state==='captcha') this.showCaptcha(card,job);
      if (job.item.state==='ready') {
        const details=node('details','','router-port-details');details.append(node('summary','在线设备'));
        if (!job.sessions?.length) details.append(node('p',job.sessionsAvailable?'暂无在线设备':'在线设备暂时无法查询'));
        for (const session of job.sessions || []) details.append(node('p',[session.ip,session.ipv6,session.mac].filter(Boolean).join(' · ')));
        card.append(details);
      }
    }
    const live=new Set(this.billingJobs.map(job=>job.uplink));
    for (const child of Array.from(container.children)) if (!live.has((child as HTMLElement).dataset.uplink || '')) child.remove();
  }

  private showCaptcha(card: HTMLElement, job: RouterBillingJob): void {
    const image=document.createElement('img'); image.alt='计费系统验证码';
    if (job.item.image && /^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/=]+$/.test(job.item.image)) image.src=job.item.image;
    const input=document.createElement('input');input.type='text';input.maxLength=12;input.autocomplete='off';input.placeholder='验证码';input.className='router-captcha-input';
    const generation=this.billingGeneration,connection=this.generation;
    const answer=async(refresh:boolean)=>{
      if (job.running || (!refresh && !input.value.trim())) return;
      job.running=true;
      try {
        await this.request('billing_captcha',{ticket:job.ticket,code:input.value.trim(),refresh});
        if (generation===this.billingGeneration && connection===this.generation) {job.item={state:'loading',message:'正在核对验证码',percent:40};this.renderBilling();}
      } catch(error) {if (generation===this.billingGeneration && connection===this.generation) {job.running=false;byId('router-billing-message').textContent=String(error);}}
    };
    const actions=node('div','','router-actions');actions.append(button('换一张',()=>{void answer(true);}),button('继续',()=>{void answer(false);},'btn btn-primary btn-sm'));
    card.append(image,input,actions);
  }

  private renderDiagnostics(): void {
    if (!this.snapshot) return;
    byId('router-diagnostics-results').replaceChildren(...this.snapshot.uplinks.map(item=>{
      const row=node('div','','router-diagnostic-row');row.append(node('strong',[item.device,item.network].filter(Boolean).join(' · ') || item.id),node('p',this.diagnosticMessage(item)));return row;
    }));
  }
  private diagnosticMessage(item: RouterUplink): string {
    const family=(name:string,state?:string)=>`${name} ${state==='reachable'?'通过':state==='unreachable'?'未通过':state==='not_configured'?'未取得地址':'待检测'}`;
    const route=(value?:string)=>({managed:'固定出口',kernel:'系统路由',missing_default:'缺少默认路由',failed:'出口未就绪',not_configured:'未取得地址'}[value || ''] || '待检查');
    return [routerLabels[item.state] || item.state,`${family('IPv4',item.internet4)} · ${family('IPv6',item.internet6)}`,`IPv4 ${route(item.route4)} · IPv6 ${route(item.route6)}`,item.message,item.updated_at?`更新于 ${new Date(item.updated_at*1000).toLocaleTimeString()}`:'尚无检测结果'].filter(Boolean).join('\n');
  }
  async diagnosticSteps(): Promise<DiagnosticStep[]> {
    if (!this.snapshot) return [];
    try { await this.options.refresh(); }
    catch { /* Keep explicitly dated last results available. */ }
    return this.snapshot?.uplinks.map(item=>({id:`router_${item.id}`,label:`路由上游 ${item.device || item.network || item.id}`,status:item.state==='online'?'success':'warning',message:this.diagnosticMessage(item),durationMs:0})) || [];
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

  private async loadSettings(): Promise<void> {
    if (!this.supports('settings')) return;
    this.settingsLoading=true;const generation=this.generation;
    try {
      const response=await this.request<{service:Record<string,string>}>('settings');
      if (generation!==this.generation || !this.snapshot) return;
      const values=response.service;
      for (const [id,key,fallback] of [['router-service-interval','interval','20'],['router-service-timeout','timeout','3'],['router-service-offline','offline_confirmations','2']]) (byId(id) as HTMLInputElement).value=values[key] || fallback;
      for (const [id,key] of [['router-service-enabled','enabled'],['router-service-routes','managed_routes'],['router-service-mwan','mwan3']]) (byId(id) as HTMLInputElement).checked=values[key]==='1';
      this.settingsLoaded=true;byId('router-settings-message').textContent='';
    } catch(error) {if (generation===this.generation) byId('router-settings-message').textContent=String(error);}
    finally {this.settingsLoading=false;}
  }
  private async saveSettings(): Promise<void> {
    if (!this.snapshot || !this.settingsLoaded) return;
    const generation=this.generation,save=byId('btn-router-settings-save') as HTMLButtonElement;save.disabled=true;
    const args:Record<string,string>={};
    for (const [id,key] of [['router-service-interval','interval'],['router-service-timeout','timeout'],['router-service-offline','offline_confirmations']]) args[key]=(byId(id) as HTMLInputElement).value;
    for (const [id,key] of [['router-service-enabled','enabled'],['router-service-routes','managed_routes'],['router-service-mwan','mwan3']]) args[key]=(byId(id) as HTMLInputElement).checked?'1':'0';
    try {await this.request('save_settings',args);if (generation===this.generation) {byId('router-settings-message').textContent='路由器设置已保存';this.options.log('路由器设置已保存');await this.options.refresh();}}
    catch(error) {if (generation===this.generation) byId('router-settings-message').textContent=String(error);}
    finally {save.disabled=false;}
  }
}
