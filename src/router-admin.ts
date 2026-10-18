import { CustomSelect } from './custom-select';
import { animateResize, toggleExpanded } from './layout-motion';
import type { RouterAccount, RouterSnapshot } from './router-model';

export interface RouterConfiguration { service: Record<string,string>; uplinks: Record<string,string>[]; networks?: {id:string}[]; }
type Field = {key:string; label:string; default?:string; min?:number; max?:number; flag?:boolean; choices?:[string,string][]};
const protocols: [string,string][] = [['auto','自动'],['bjut-sushe','bjut-sushe'],['bjut_wifi','bjut_wifi'],['lgn','lgn']];
const transport: [string,string][] = [['auto','自动'],['ethernet','有线'],['wifi','Wi-Fi']];
const fields: Field[] = [
  {key:'enabled',label:'自动登录',flag:true,default:'0'},
  {key:'interval',label:'检测间隔（秒）',default:'20',min:5,max:3600},
  {key:'timeout',label:'请求超时（秒）',default:'3',min:1,max:30},
  {key:'offline_confirmations',label:'离线确认次数',default:'2',min:1,max:10},
  {key:'managed_routes',label:'固定上游出口',flag:true,default:'1'},
  {key:'mwan3',label:'兼容 mwan3',flag:true,default:'1'},
  {key:'max_backoff',label:'最大重试间隔（秒）',default:'300',min:5,max:86400},
  {key:'credential_cooldown',label:'账号拒绝后等待（秒）',default:'600',min:30,max:86400},
  {key:'post_login_delay',label:'登录后复核延迟（秒）',default:'2',min:0,max:15},
  {key:'login_type',label:'认证方式',default:'auto',choices:protocols},
  {key:'transport',label:'上游类型',default:'auto',choices:transport},
  {key:'compatibility',label:'连接方式',default:'secure',choices:[['secure','HTTPS'],['legacy_http','旧式 HTTP']]},
  {key:'strict_network',label:'核对校园网身份',flag:true,default:'1'},
  {key:'expected_ssid',label:'指定 Wi-Fi 名称'}, {key:'expected_bssid',label:'指定 Wi-Fi 地址'},
  {key:'online_url',label:'联网检测地址',default:'http://connect.rom.miui.com/generate_204'},
  {key:'online_code',label:'联网成功状态码',default:'204',min:100,max:599},
  {key:'log_target',label:'日志保存位置',default:'system',choices:[['system','系统日志'],['file','自定义目录']]},
  {key:'log_directory',label:'日志目录',default:'/tmp/bjut-autologin-log'},
  {key:'log_web',label:'创建网页日志链接（局域网可访问）',flag:true,default:'0'},
  {key:'log_max_kb',label:'单个日志大小（KB）',min:64,max:4096,default:'256'},
  {key:'log_level',label:'日志级别',default:'info',choices:[['trace','详细调试'],['debug','调试'],['info','信息'],['notice','通知'],['warning','警告'],['error','仅错误']]},
];
const legacy = new Set(['enabled','interval','timeout','offline_confirmations','managed_routes','mwan3','log_level']);
const inputIds: Record<string,string> = {interval:'router-service-interval',timeout:'router-service-timeout',offline_confirmations:'router-service-offline',enabled:'router-service-enabled',managed_routes:'router-service-routes',mwan3:'router-service-mwan'};
const protocol = (value:string) => ({type1:'bjut-sushe',type2:'bjut_wifi',type3:'lgn'}[value] || value);
let sequence = 0;
const element = (tag:string,text='',className='') => { const e=document.createElement(tag);e.textContent=text;e.className=className;return e; };
function action(text:string,click:()=>void,className='btn btn-secondary btn-sm'): HTMLButtonElement { const b=document.createElement('button');b.type='button';b.textContent=text;b.className=className;b.addEventListener('click',click);return b; }
interface Control { read(): string; close(): void; }
function field(parent:HTMLElement, definition:Field, value:string, id?:string): Control {
  const label=element('label',definition.label,definition.flag?'router-setting-switch':'');
  if (definition.choices) {
    const wrapper=element('div','','custom-select');wrapper.id=id || `router-editor-select-${++sequence}`;wrapper.setAttribute('aria-label',definition.label);
    wrapper.innerHTML='<div class="custom-select-trigger"><span></span><span aria-hidden="true">⌄</span></div><div class="custom-select-options"></div>';
    label.append(wrapper);parent.append(label);
    const select=new CustomSelect(wrapper.id);select.setOptions(definition.choices.map(([value,text])=>({value,text})));select.value=protocol(value);
    return {read:()=>select.value,close:()=>select.setDisabled(true)};
  }
  const input=document.createElement('input');input.type=definition.flag?'checkbox':definition.min!==undefined?'number':'text';
  input.id=id || `router-editor-input-${++sequence}`;
  if (definition.flag) input.checked=value==='1'; else input.value=value;
  if (definition.min!==undefined) {input.min=String(definition.min);input.max=String(definition.max);input.required=true;}
  label.setAttribute('for',input.id);label.prepend(input);if (!definition.flag) label.append(input);
  parent.append(label);return {read:()=>definition.flag?(input.checked?'1':'0'):input.value.trim(),close:()=>{}};
}

export class RouterAdmin {
  private snapshot: RouterSnapshot | null=null;
  private configuration: RouterConfiguration | null=null;
  private generation=0;
  private loading=false;
  private controls=new Map<string,Control>();
  private editor: HTMLElement | null=null;
  constructor(private options:{ request:<T>(operation:string,args?:Record<string,unknown>)=>Promise<T>; refresh:()=>Promise<void>; confirm:(message:string,title:string,label:string)=>Promise<boolean>; log:(message:string)=>void; }) {
    document.getElementById('router-settings-form')!.addEventListener('submit',event=>{event.preventDefault();void this.saveSettings();});
    document.getElementById('btn-router-settings-advanced')!.addEventListener('click',()=>{void this.showMore();});
    document.getElementById('btn-router-add-account')!.addEventListener('click',()=>this.editAccount());
  }
  update(snapshot:RouterSnapshot) { this.snapshot=snapshot; }
  disconnect() {this.generation++;this.snapshot=null;this.configuration=null;this.controls.forEach(c=>c.close());this.controls.clear();this.closeEditor();}
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
    this.controls.forEach(c=>c.close());this.controls.clear();common.replaceChildren();more.replaceChildren();
    for (const definition of fields) {
      if (definition.key.startsWith('log_') && definition.key!=='log_level' && !this.supports('log_settings')) continue;
      if (!this.supports('uplink_manage') && !legacy.has(definition.key)) continue;
      const target=legacy.has(definition.key)?common:more;
      this.controls.set(definition.key,field(target,definition,this.configuration!.service[definition.key] || definition.default || '',inputIds[definition.key]));
    }
  }
  private async showMore() {
    await this.load();
    toggleExpanded(document.getElementById('router-service-advanced') as HTMLDetailsElement);
  }
  private async saveSettings() {
    if(!this.snapshot || !this.configuration)return;
    const button=document.getElementById('btn-router-settings-save') as HTMLButtonElement;
    const generation=this.generation;button.disabled=true;
    try {
      const args=Object.fromEntries([...this.controls].map(([key,control])=>[key,control.read()]));
      await this.options.request('save_settings',args);
      if(generation!==this.generation)return;
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
      const text=element('div');text.append(element('strong',account.user),element('p',`${account.enabled?'参与自动登录':'仅手动使用'} · ${protocol(account.loginType || 'auto')==='auto'?'所有认证方式':protocol(account.loginType || 'auto')}`));
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
    editor.modal.addEventListener('router-editor-close',()=>{password.value='';type.close();});
    const generation=this.generation;
    editor.form.addEventListener('submit',async event=>{
      event.preventDefault();editor.save.disabled=true;
      try{await this.options.request('account_save',{account:account?.id || '',username:user.read(),password:password.value,enabled:enabled.read(),login_type:type.read()});
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
    const group=element('fieldset','','router-sync-accounts');group.append(element('legend','自动登录账号（不选则使用全部启用账号）'));
    const selected=new Set((uplink?.accounts || '').split(/\s+/));
    for(const account of this.snapshot?.accounts || []) {const label=element('label',account.user);const input=document.createElement('input');input.type='checkbox';input.value=account.id;input.checked=selected.has(account.id);label.prepend(input);group.append(label);}
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
