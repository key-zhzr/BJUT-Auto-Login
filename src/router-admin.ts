import {usageStorageEstimate} from './device-usage-model';
import { CustomSelect } from './custom-select';
import { animateResize } from './layout-motion';
import type { RouterAccount, RouterSnapshot } from './router-model';
import {routerSettingVisible} from './router-model';
import { RouterMaintenance } from './router-maintenance';

export interface RouterConfiguration { service: Record<string,string>; uplinks: Record<string,string>[]; networks?: {id:string}[]; }
type Field = {key:string; label:string; hint?:string; default?:string; min?:number; max?:number; flag?:boolean; secret?:boolean; choices?:[string,string][]};
const protocols: [string,string][] = [['auto','自动'],['bjut-sushe','bjut-sushe'],['bjut_wifi','bjut_wifi'],['lgn','lgn']];
const transport: [string,string][] = [['auto','自动'],['ethernet','有线'],['wifi','Wi-Fi']];
const fields: Field[] = [
  {key:'enabled',label:'自动登录',hint:'检测到校园网断开时自动登录',flag:true,default:'0'},
  {key:'interval',label:'检测间隔（秒）',hint:'前台检测校园网状态的基础间隔',default:'20',min:5,max:3600},
  {key:'timeout',label:'请求超时（秒）',hint:'单次探测与认证请求的等待上限',default:'3',min:1,max:30},
  {key:'offline_confirmations',label:'离线确认次数',hint:'连续离线达到该次数后才切换账号或重试',default:'2',min:1,max:10},
  {key:'managed_routes',label:'固定上游出口',hint:'将上游流量绑定到对应接口的出口路由',flag:true,default:'1'},
  {key:'mwan3',label:'兼容 mwan3',hint:'与 mwan3 策略路由共存',flag:true,default:'1'},
  {key:'max_backoff',label:'最大重试间隔（秒）',hint:'断网后指数退避的上限',default:'300',min:5,max:86400},
  {key:'credential_cooldown',label:'账号拒绝后等待（秒）',hint:'账号被明确拒绝后暂停尝试的时长',default:'600',min:30,max:86400},
  {key:'post_login_delay',label:'登录后复核延迟（秒）',hint:'登录成功后再次核对状态的延迟',default:'2',min:0,max:15},
  {key:'login_type',label:'认证方式',hint:'自动会按当前网络选择协议',default:'auto',choices:protocols},
  {key:'transport',label:'上游类型',hint:'上游接口的物理连接类型',default:'auto',choices:transport},
  {key:'compatibility',label:'连接方式',hint:'HTTPS 为推荐的安全连接方式',default:'secure',choices:[['secure','HTTPS'],['legacy_http','旧式 HTTP']]},
  {key:'strict_network',label:'核对校园网身份',hint:'校验 SSID/BSSID 与校园网身份',flag:true,default:'1'},
  {key:'expected_ssid',label:'指定 Wi-Fi 名称',hint:'留空表示不限制 Wi-Fi 名称'}, {key:'expected_bssid',label:'指定 Wi-Fi 地址',hint:'留空表示不限制接入点'},
  {key:'online_url',label:'联网检测地址',hint:'用于判断互联网是否可用的探测地址',default:'http://connect.rom.miui.com/generate_204'},
  {key:'online_code',label:'联网成功状态码',hint:'探测地址返回的可接受状态码',default:'204',min:100,max:599},
  {key:'log_target',label:'日志保存位置',hint:'系统日志或自定义目录',default:'system',choices:[['system','系统日志'],['file','自定义目录']]},
  {key:'log_directory',label:'日志目录',hint:'自定义日志目录的绝对路径',default:'/tmp/bjut-autologin-log'},
  {key:'log_web',label:'创建网页日志链接（局域网可访问）',hint:'在局域网内提供只读网页日志',flag:true,default:'0'},
  {key:'log_max_kb',label:'单个日志大小（KB）',hint:'达到大小后轮换日志文件',min:64,max:4096,default:'256'},
  {key:'log_level',label:'日志级别',hint:'记录到日志的最低级别',default:'info',choices:[['trace','详细调试'],['debug','调试'],['info','信息'],['notice','通知'],['warning','警告'],['error','仅错误']]},
  {key:'pushplus_enabled',label:'PushPlus 消息推送',hint:'接收登录、恢复联网和上游异常提醒',flag:true,default:'0'},
  {key:'pushplus_token',label:'PushPlus 令牌',hint:'填写用户令牌或消息令牌；留空保留已保存的令牌',secret:true},
  {key:'pushplus_topic',label:'群组编码（可选）',hint:'留空时仅发送给自己'},
  {key:'pushplus_online',label:'登录与恢复提醒',flag:true,default:'1'},
  {key:'pushplus_failure',label:'异常提醒',flag:true,default:'1'},
  {key:'pushplus_delay',label:'异常持续时间（秒）',hint:'短暂断网不提醒；账号被拒绝等明确错误会立即提醒',min:10,max:3600,default:'60'},
  {key:'pushplus_interval',label:'重复提醒间隔（秒）',min:60,max:86400,default:'3600'},
  {key:'billing_background',label:'后台计费检查',hint:'App 关闭后，路由器仍定期检查各账号余额和流量',flag:true,default:'0'},
  {key:'traffic_monitor',label:'设备流量统计',hint:'每分钟从计费系统读取各设备已用流量',flag:true,default:'0'},
  {key:'traffic_scope',label:'流量统计范围',default:'local',choices:[['local','仅路由器上游'],['all','全部在线设备']]},
  {key:'traffic_minute_hours',label:'分钟明细保留（小时）',default:'48',min:1,max:8760},
  {key:'traffic_hour_days',label:'小时汇总保留（天）',default:'90',min:1,max:3660},
  {key:'traffic_day_days',label:'每日汇总保留（天）',default:'366',min:1,max:36600},
  {key:'traffic_storage_mb',label:'统计存储上限（MB）',default:'2',min:1,max:128,hint:'达到上限时优先保留较新记录；保存时还需临时空间'},
  {key:'prefer_remaining_flow',label:'优先使用余量充足的账号',hint:'依据已开启统计取得的新鲜计费数据排序，独立账号顺序优先',flag:true,default:'0'},
  {key:'billing_interval',label:'计费检查间隔（秒）',min:900,max:86400,default:'21600'},
  {key:'billing_alerts',label:'用量不足时推送',hint:'通过已配置的 PushPlus 提醒，每个账号每天最多一次',flag:true,default:'1'},
  {key:'balance_threshold',label:'余额提醒线（元）',min:0,max:999999,default:'5'},
  {key:'flow_threshold',label:'流量提醒线（GB）',min:0,max:999999,default:'5'},
];
const legacy = new Set(['enabled','interval','timeout','offline_confirmations','managed_routes','mwan3','log_level']);
const inputIds: Record<string,string> = {interval:'router-service-interval',timeout:'router-service-timeout',offline_confirmations:'router-service-offline',enabled:'router-service-enabled',managed_routes:'router-service-routes',mwan3:'router-service-mwan'};
const protocol = (value:string) => ({type1:'bjut-sushe',type2:'bjut_wifi',type3:'lgn'}[value] || value);
let sequence = 0;
const element = (tag:string,text='',className='') => { const e=document.createElement(tag);e.textContent=text;e.className=className;return e; };
function action(text:string,click:()=>void,className='btn btn-secondary btn-sm'): HTMLButtonElement { const b=document.createElement('button');b.type='button';b.textContent=text;b.className=className;b.addEventListener('click',click);return b; }
interface Control { read(): string; close(): void; row?:HTMLElement; onChange?:(callback:()=>void)=>void; setActive?:(active:boolean)=>void; }
interface FieldControl extends Control { element: HTMLElement; attach?: () => void; }
function createFieldControl(definition:Field, value:string, id?:string): FieldControl {
  if (definition.choices) {
    const wrapper=element('div','','custom-select');wrapper.id=id || `router-editor-select-${++sequence}`;wrapper.setAttribute('aria-label',definition.label);
    wrapper.innerHTML='<div class="custom-select-trigger"><span></span><span aria-hidden="true">⌄</span></div><div class="custom-select-options"></div>';
    let select: CustomSelect | null=null;
    return {
      element:wrapper,
      attach:()=>{if(select)return;select=new CustomSelect(wrapper.id);select.setOptions(definition.choices!.map(([value,text])=>({value,text})));select.value=protocol(value);},
      read:()=>select ? select.value : '',
      onChange:callback=>select?.addEventListener('change',callback),
      setActive:active=>select?.setDisabled(!active),
      close:()=>{select?.setDisabled(true);},
    };
  }
  const input=document.createElement('input');input.type=definition.flag?'checkbox':definition.secret?'password':definition.min!==undefined?'number':'text';
  input.id=id || `router-editor-input-${++sequence}`;
  if (definition.flag) input.checked=value==='1'; else input.value=value;
  input.setAttribute('aria-label',definition.label);
  if (definition.secret) {input.autocomplete='new-password';input.maxLength=256;}
  if (definition.min!==undefined) {input.min=String(definition.min);input.max=String(definition.max);input.required=true;}
  return {element:input,read:()=>definition.flag?(input.checked?'1':'0'):input.value.trim(),close:()=>{if(definition.secret)input.value='';},onChange:callback=>input.addEventListener('change',callback),setActive:active=>{input.disabled=!active;}};
}
function field(parent:HTMLElement, definition:Field, value:string, id?:string): Control {
  const control=createFieldControl(definition,value,id);
  const label=element('label',definition.label,definition.flag?'router-setting-switch':'');
  if (definition.flag) label.prepend(control.element);
  else {
    if (control.element instanceof HTMLInputElement) label.setAttribute('for',control.element.id);
    label.append(control.element);
  }
  parent.append(label);control.attach?.();return control;
}
function settingField(parent:HTMLElement, definition:Field, value:string, id?:string): Control {
  const control=createFieldControl(definition,value,id);
  const row=element('div','','setting-item router-setting-item'),info=element('div','','setting-info');
  info.append(element('h4',definition.label));
  if (definition.hint) info.append(element('p',definition.hint));
  const slot=element('div','','router-setting-control');
  if (definition.flag) {
    const toggle=element('label','','switch');toggle.append(control.element,element('span','','slider round'));slot.append(toggle);
  } else {
    control.element.classList.add(definition.choices?'router-setting-select':definition.min!==undefined?'input-inline':'router-setting-input');
    slot.append(control.element);
  }
  row.append(info,slot);parent.append(row);control.attach?.();return {...control,row};
}

export class RouterAdmin {
  private snapshot: RouterSnapshot | null=null;
  private configuration: RouterConfiguration | null=null;
  private generation=0;
  private loading=false;
  private controls=new Map<string,Control>();
  private editor: HTMLElement | null=null;
  private maintenance:RouterMaintenance;
  constructor(private options:{ request:<T>(operation:string,args?:Record<string,unknown>)=>Promise<T>; refresh:()=>Promise<void>; confirm:(message:string,title:string,label:string)=>Promise<boolean>; log:(message:string)=>void; }) {
    this.maintenance=new RouterMaintenance(options);
    document.getElementById('router-settings-form')!.addEventListener('submit',event=>{event.preventDefault();void this.saveSettings();});
    document.getElementById('btn-router-add-account')!.addEventListener('click',()=>this.editAccount());
  }
  update(snapshot:RouterSnapshot) {
    if(this.snapshot && JSON.stringify(this.snapshot.capabilities)!==JSON.stringify(snapshot.capabilities))this.configuration=null;
    this.snapshot=snapshot;
  }
  invalidateSettings(){this.configuration=null;}
  resume(){this.maintenance.resume();}
  disconnect() {this.generation++;this.snapshot=null;this.configuration=null;this.controls.forEach(c=>c.close());this.controls.clear();this.closeEditor();this.maintenance.close();}
  private supports(capability:string) {return this.snapshot?.capabilities?.includes(capability) || false;}
  async load():Promise<void> {
    if (this.loading || !this.snapshot || this.configuration) return;
    this.loading=true;const generation=this.generation;
    try {
      const data=await this.options.request<RouterConfiguration>('settings');
      if (generation!==this.generation || !this.snapshot) return;
      this.configuration=data;this.buildService();this.renderUplinks();
      document.getElementById('router-settings-message')!.textContent='';
    } catch(error) {if(generation===this.generation)document.getElementById('router-settings-message')!.textContent=String(error);}
    finally {this.loading=false;}
  }
  private buildService() {
    const common=document.getElementById('router-service-common')!, more=document.getElementById('router-service-more')!;
    this.maintenance.close();this.controls.forEach(c=>c.close());this.controls.clear();common.replaceChildren();more.replaceChildren();
    for (const definition of fields) {
      if(definition.key==='traffic_scope'&&!this.supports('traffic_charts'))continue;
      if (definition.key.startsWith('pushplus_') && !this.supports('pushplus')) continue;
      if(['traffic_monitor','traffic_scope','prefer_remaining_flow'].includes(definition.key)&&!this.supports('device_usage'))continue;
      if (/^(billing_|balance_threshold|flow_threshold)/.test(definition.key) && !this.supports('billing_background')) continue;
      if (definition.key.startsWith('log_') && definition.key!=='log_level' && !this.supports('log_settings')) continue;
      if (!this.supports('uplink_manage') && !legacy.has(definition.key)) continue;
      const target=legacy.has(definition.key)?common:more;
      this.controls.set(definition.key,settingField(target,definition,this.configuration!.service[definition.key] || definition.default || '',inputIds[definition.key]));
    }
    if(this.supports('traffic_charts')){
      const estimate=element('p','','traffic-note');more.append(estimate);
      const update=()=>{const read=(key:string,fallback:number)=>Number(this.controls.get(key)?.read()||fallback),n=Math.max(1,this.snapshot?.uplinks.length||1),retention={minuteHours:read('traffic_minute_hours',48),hourDays:read('traffic_hour_days',90),dayDays:read('traffic_day_days',366)};
        estimate.textContent=`按 ${n} 条上游预计约 ${(usageStorageEstimate(n,retention)/1048576).toFixed(1)} MB（含临时空间${this.supports('traffic_compression')?'；文件压缩保存':''}）。当前容量上限 ${read('traffic_storage_mb',2)} MB，按压缩前估算。全部设备的占用会更多。`;};
      more.oninput=update;update();
    }
    if (this.supports('pushplus')) {
      const row=element('div','','setting-item router-setting-item'),info=element('div','','setting-info');
      info.append(element('h4','测试推送'),element('p','请先保存设置。'));
      const button=action('发送测试消息',()=>{void this.testPushPlus(button);});
      if(this.supports('pushplus_status'))info.append(action('查看测试状态',()=>{void this.readPushPlusStatus(this.generation).catch(error=>{document.getElementById('router-settings-message')!.textContent=String(error);});}));
      row.append(info,button);row.dataset.routerDependency='_pushplus_test';more.append(row);
    }
    if(this.supports('maintenance'))this.maintenance.mount(more);
    const updateVisibility=(animate=false)=>{
      const read=(key:string)=>this.controls.get(key)?.read()||this.configuration?.service[key]||'';
      const changes=[...this.controls].filter(([key,control])=>control.row&&control.row.hidden===routerSettingVisible(key,read));
      const test=more.querySelector<HTMLElement>('[data-router-dependency]');
      const changeTest=test&&test.hidden===routerSettingVisible('_pushplus_test',read);
      const change=()=>{for(const [key,control] of changes){const visible=routerSettingVisible(key,read);control.row!.hidden=!visible;control.setActive?.(visible);}if(test)test.hidden=!routerSettingVisible('_pushplus_test',read);};
      if(animate&&(changes.length||changeTest))animateResize(more,change);else change();
    };
    updateVisibility();
    for(const key of ['log_target','pushplus_enabled','billing_background'])this.controls.get(key)?.onChange?.(()=>updateVisibility(true));
  }
  private async testPushPlus(button:HTMLButtonElement) {
    const generation=this.generation;button.disabled=true;
    try {
      const result=await this.options.request<{message:string}>('pushplus_test');
      if(generation===this.generation)document.getElementById('router-settings-message')!.textContent=result.message;
      if(this.supports('pushplus_status'))for(let attempt=0;attempt<8&&generation===this.generation;attempt++){
        await new Promise(resolve=>setTimeout(resolve,1000));
        if(generation!==this.generation)break;
        const status=await this.readPushPlusStatus(generation);
        if(status!=='queued')break;
      }
    } catch(error) {if(generation===this.generation)document.getElementById('router-settings-message')!.textContent=String(error);}
    finally {button.disabled=false;}
  }
  private async readPushPlusStatus(generation:number):Promise<string>{
    const status=await this.options.request<{state:string;message:string}>('pushplus_status');
    if(generation===this.generation)document.getElementById('router-settings-message')!.textContent=status.message;
    return status.state;
  }
  private async saveSettings() {
    if(!this.snapshot || !this.configuration)return;
    const button=document.getElementById('btn-router-settings-save') as HTMLButtonElement;
    const generation=this.generation;button.disabled=true;
    try {
      const args=Object.fromEntries([...this.controls].filter(([,control])=>!control.row?.hidden).map(([key,control])=>[key,control.read()]));
      await this.options.request('save_settings',args);
      if(generation!==this.generation)return;
      this.controls.get('pushplus_token')?.close();
      document.getElementById('router-settings-message')!.textContent='路由器设置已保存';this.options.log('路由器设置已保存');await this.options.refresh();
    } catch(error){if(generation===this.generation)document.getElementById('router-settings-message')!.textContent=String(error);}
    finally{button.disabled=false;}
  }
  renderAccounts() {
    if(!this.snapshot)return;
    const container=document.getElementById('router-accounts-list')!;
    const accounts=this.snapshot.accounts;
    document.getElementById('router-accounts-count')!.textContent=`${accounts.length} 个账号`;
    (document.getElementById('btn-router-add-account') as HTMLButtonElement).disabled=!this.supports('account_manage');
    animateResize(container, () => container.replaceChildren(...accounts.map((account,index)=>{
      const row=element('article','','router-account-row router-enter');
      const text=element('div');text.append(element('strong',account.user),element('p',`${account.enabled?'参与自动登录':'仅手动使用'} · ${protocol(account.loginType || 'auto')==='auto'?'所有认证方式':protocol(account.loginType || 'auto')} · 占用 ${account.onlineCount || 0}/${account.maxSessions || '不限'}`));
      const tools=element('div','','router-actions');
      tools.append(action(account.enabled?'停用自动登录':'启用自动登录',()=>{void this.mutate('account_set',{account:account.id,enabled:account.enabled?'0':'1'});}));
      if(this.supports('account_manage')) {
        const up=action('上移',()=>{void this.mutate('account_move',{account:account.id,direction:'up'});});up.disabled=index===0;
        const down=action('下移',()=>{void this.mutate('account_move',{account:account.id,direction:'down'});});down.disabled=index===accounts.length-1;
        tools.append(action('编辑',()=>this.editAccount(account)),up,down,action('删除',()=>{void this.remove('account_delete',{account:account.id},`删除路由账号 ${account.user}？`);}));
      }
      row.append(text,tools);return row;
    })));
  }
  private async mutate(operation:string,args:Record<string,unknown>) {
    const generation=this.generation;
    try {await this.options.request(operation,args);if(generation===this.generation)await this.options.refresh();}
    catch(error){if(generation===this.generation)document.getElementById('router-accounts-message')!.textContent=String(error);}
  }
  private async remove(operation:string,args:Record<string,unknown>,message:string) {
    if(!await this.options.confirm(message,'确认删除','删除'))return;
    await this.mutate(operation,args);
    if(operation==='uplink_delete'){this.configuration=null;await this.load();}
  }
  private closeEditor() {
    const editor=this.editor;if(!editor)return;
    this.editor=null;editor.removeAttribute('id');editor.inert=true;editor.classList.add('hidden');editor.dispatchEvent(new Event('router-editor-close'));
    window.setTimeout(()=>editor.remove(),300);
  }
  private createEditor(title:string) {
    this.closeEditor();
    const modal=element('div','','modal-overlay hidden');modal.id='router-admin-editor';
    const panel=element('div','','modal-content glass-card router-admin-modal');panel.setAttribute('role','dialog');panel.setAttribute('aria-modal','true');panel.setAttribute('aria-label',title);
    const form=document.createElement('form');form.className='router-connect-form';
    const message=element('p','','error');message.setAttribute('role','status');
    const tools=element('div','','modal-actions');const cancel=action('取消',()=>this.closeEditor(),'btn btn-secondary');const save=document.createElement('button');save.type='submit';save.className='btn btn-primary';save.textContent='保存';tools.append(cancel,save);
    panel.append(element('h3',title),form);form.append(message,tools);modal.append(panel);document.body.append(modal);this.editor=modal;
    modal.addEventListener('keydown',event=>{if(event.key==='Escape'){event.stopPropagation();this.closeEditor();}});
    requestAnimationFrame(()=>modal.classList.remove('hidden'));
    return {form,message,save,fields:message,modal};
  }
  private editAccount(account?:RouterAccount) {
    if(!this.supports('account_manage'))return;
    const editor=this.createEditor(account?'编辑路由账号':'添加路由账号');
    const body=element('div','','router-editor-grid');editor.form.insertBefore(body,editor.fields);
    const user=field(body,{key:'username',label:'账号'},account?.user || '');
    const label=element('label',account?'新密码（留空保留原密码）':'密码');const password=document.createElement('input');password.type='password';password.autocomplete='new-password';password.required=!account;label.append(password);body.append(label);
    const enabled=field(body,{key:'enabled',label:'参与自动登录',flag:true},String(account?.enabled ?? 1));
    const type=field(body,{key:'login_type',label:'认证方式',choices:protocols},account?.loginType || 'auto');
    const limit=field(body,{key:'max_sessions',label:'同时在线上限（0 自动：普通 3 / VIP 5）',min:0,max:32},String(account?.configuredMaxSessions || 0));
    editor.modal.addEventListener('router-editor-close',()=>{password.value='';type.close();});
    const generation=this.generation;
    editor.form.addEventListener('submit',async event=>{
      event.preventDefault();editor.save.disabled=true;
      try{await this.options.request('account_save',{account:account?.id || '',username:user.read(),password:password.value,enabled:enabled.read(),login_type:type.read(),max_sessions:limit.read()});
        if(generation===this.generation){password.value='';this.closeEditor();await this.options.refresh();}}
      catch(error){if(generation===this.generation)editor.message.textContent=String(error);}
      finally{editor.save.disabled=false;}
    });
    body.querySelector<HTMLInputElement>('input')?.focus({preventScroll:true});
  }
  private renderUplinks() {
    const container=document.getElementById('router-configured-uplinks')!;animateResize(container,()=>{container.replaceChildren();
    if(!this.supports('uplink_manage')){container.append(element('p','更新路由端后，可在这里编辑上游接口。'));return;}
    const list=element('div','','router-admin-uplink-list');
    for(const uplink of this.configuration?.uplinks || []) {
      const card=element('article','','router-account-row router-enter');
      const text=element('div');text.append(element('strong',`${uplink.network} / ${uplink.network6 || '跟随 IPv4'}`),element('p',`${uplink.enabled==='0'?'已停用':'已启用'} · ${protocol(uplink.login_type || 'auto')==='auto'?'自动探测':protocol(uplink.login_type)}`));
      const controls=element('div','','router-actions');controls.append(action('编辑',()=>this.editUplink(uplink)),action('删除',()=>{void this.remove('uplink_delete',{uplink:uplink.id},`删除上游 ${uplink.network}？`);}));card.append(text,controls);list.append(card);
    }
    container.append(list,action('添加上游',()=>this.editUplink(),'btn btn-secondary'));
    });
  }
  private editUplink(uplink?:Record<string,string>) {
    const editor=this.createEditor(uplink?'编辑路由上游':'添加路由上游');
    const body=element('div','','router-editor-grid');editor.form.insertBefore(body,editor.fields);
    const defs:Field[]=[{key:'enabled',label:'启用上游',flag:true,default:'1'}, {key:'network',label:'IPv4 接口'}, {key:'network6',label:'IPv6 接口（留空跟随）'}, {key:'device',label:'IPv4 设备（可选）'}, {key:'device6',label:'IPv6 设备（可选）'}, {key:'source4',label:'IPv4 地址（可选）'}, {key:'source6',label:'IPv6 地址（可选）'}, {key:'transport',label:'上游类型',choices:transport,default:'auto'}, {key:'login_type',label:'认证方式',choices:protocols,default:'auto'}, {key:'expected_ssid',label:'指定 Wi-Fi 名称'}, {key:'expected_bssid',label:'指定 Wi-Fi 地址'}];
    const controls=new Map(defs.map(def=>[def.key,field(body,def,uplink?.[def.key] || def.default || '')]));
    const group=element('fieldset','','router-sync-accounts');group.append(element('legend','自动登录顺序'),element('p','勾选账号并用箭头调整此接口的顺序；不选则跟随账号管理。'));
    const selectedIds=(uplink?.accounts || '').split(/\s+/).filter(Boolean),selected=new Set(selectedIds);
    const choices=new Map((this.snapshot?.accounts || []).map(account=>[account.id,account]));
    const list=element('div','','router-account-order');group.append(list);
    const updateArrows=()=>[...list.children].forEach((row,index)=>{row.querySelector<HTMLButtonElement>('[data-move=up]')!.disabled=index===0;row.querySelector<HTMLButtonElement>('[data-move=down]')!.disabled=index===list.children.length-1;});
    const move=(row:HTMLElement,direction:number)=>{const other=direction<0?row.previousElementSibling:row.nextElementSibling;if(!other)return;animateResize(list,()=>{if(direction<0)list.insertBefore(row,other);else list.insertBefore(other,row);updateArrows();});};
    for(const id of new Set([...selectedIds,...choices.keys()])) {
      const account=choices.get(id);if(!account)continue;
      const row=element('div','','router-account-order-row'),label=element('label',account.user+(account.enabled?'':'（仅手动）'));
      const input=document.createElement('input');input.type='checkbox';input.value=id;input.checked=selected.has(id);label.prepend(input);
      const up=action('↑',()=>move(row,-1)),down=action('↓',()=>move(row,1));up.dataset.move='up';down.dataset.move='down';up.setAttribute('aria-label',`上移 ${account.user}`);down.setAttribute('aria-label',`下移 ${account.user}`);
      row.append(label,up,down);list.append(row);
    }
    updateArrows();
    body.append(group);editor.modal.addEventListener('router-editor-close',()=>controls.forEach(c=>c.close()));
    const generation=this.generation;
    editor.form.addEventListener('submit',async event=>{
      event.preventDefault();editor.save.disabled=true;
      try {
        const payload=Object.fromEntries([...controls].map(([key,control])=>[key,control.read()]));payload.accounts=[...group.querySelectorAll<HTMLInputElement>('input:checked')].map(i=>i.value).join(' ');
        await this.options.request('uplink_save',{uplink:uplink?.id || '',payload:JSON.stringify(payload)});
        if(generation===this.generation){this.closeEditor();this.configuration=null;await this.options.refresh();await this.load();}
      }catch(error){if(generation===this.generation)editor.message.textContent=String(error);}finally{editor.save.disabled=false;}
    });
  }
}
