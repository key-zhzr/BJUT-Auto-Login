import { animateResize } from './layout-motion';

type Data = Record<string, any>;
export interface BillingTarget {uplink:string;account:string;title:string;ticket:string;expectedAccount?:string;}
interface Job {target:BillingTarget;ticket:string;running:boolean;result:Data;card:HTMLElement;stamp:string;}
interface Options {
  request:<T=Data>(method:string,args:Data)=>Promise<T>;
  confirm:(message:string)=>Promise<boolean>;
  select?:(parent:HTMLElement,label:string,choices:{value:string;text:string}[])=>{value:()=>string;close:()=>void};
  changed:()=>void;
}
const node=(tag:string,text='',className='')=>{const el=document.createElement(tag);el.textContent=text;el.className=className;return el;};
const labels:Record<string,string>={online:'状态',mac:'MAC',device:'设备',lastLoginAt:'最近登录',lastIp:'最近 IP',loginTime:'登录时间',logoutTime:'下线时间',ip:'IPv4',ipv6:'IPv6',useTime:'在线时长（秒）',durationMinutes:'在线时长（分钟）',flowMb:'流量（MB）',billingMode:'计费方式',amount:'费用（元）',flow:'流量',cost:'费用',reason:'说明'};

/** Shared view logic is also shipped in LuCI; transport and selects are injected. */
export class RouterBillingTools {
  private jobs:Job[]=[];
  private generation=0;
  private busy=false;
  private module='services';
  private pending=new Set<Job>();
  private controls:(()=>void)[]=[];
  constructor(private root:HTMLElement,private options:Options){}
  clear(clearDom=true){this.generation++;this.jobs=[];this.controls.forEach(close=>close());this.controls=[];if(clearDom)animateResize(this.root,()=>this.root.replaceChildren());}
  async load(module:string,targets:BillingTarget[]){
    this.clear(false);this.module=module;const generation=this.generation;
    this.jobs=targets.map(target=>({target,ticket:'',running:true,result:{state:'loading',message:'正在查询…',percent:2},card:node('article','','glass-card router-billing-card router-enter bj-card bj-enter'),stamp:''}));
    animateResize(this.root,()=>this.root.replaceChildren(...this.jobs.map(job=>job.card)));
    if(!targets.length){this.root.append(node('p','请先完成计费账号查询。'));return;}
    this.jobs.forEach(job=>this.draw(job,module));
    await Promise.all(this.jobs.map(async job=>{
      try{const response=await this.options.request('billing_details',{uplink:job.target.uplink,account:job.target.account,module});if(generation!==this.generation)return;job.ticket=response.ticket;}
      catch(error){if(generation===this.generation){job.running=false;job.result={state:'error',message:String(error)};this.draw(job,module);}}
    }));
    if(generation===this.generation)await this.poll(module);
  }
  async poll(module=this.module){
    if(this.busy || !this.jobs.some(job=>job.running && job.ticket))return;
    this.busy=true;const generation=this.generation;
    try{await Promise.all(this.jobs.filter(job=>job.running && job.ticket).map(async job=>{
      try{const response=await this.options.request('billing_poll',{ticket:job.ticket});if(generation!==this.generation)return;
        if(response.result?.state==='ready' && job.target.expectedAccount && response.result.account!==job.target.expectedAccount)throw new Error('计费账号已变化，请重新查询概览。');
        job.result={...response.result,sessions:response.sessions,sessionsAvailable:response.sessionsAvailable};job.running=job.result.state==='loading';
      }catch(error){if(generation!==this.generation)return;job.running=false;job.result={state:'error',message:String(error)};}
      this.draw(job,module);
    }));}finally{this.busy=false;}
  }
  private button(text:string,run:()=>void){const b=node('button',text,'btn btn-secondary btn-sm') as HTMLButtonElement;b.type='button';b.addEventListener('click',run);return b;}
  private field(parent:HTMLElement,text:string,type='text',value=''){
    const label=node('label',text),input=document.createElement('input');input.type=type;input.value=value;input.required=true;input.autocomplete=type==='password'?'new-password':'off';label.append(input);parent.append(label);return input;
  }
  private choice(parent:HTMLElement,text:string,choices:{value:string;text:string}[]){
    if(this.options.select){const control=this.options.select(parent,text,choices);this.controls.push(control.close);return control.value;}
    const label=node('label',text),select=document.createElement('select');select.className='cbi-input-select';
    choices.forEach(item=>{const option=document.createElement('option');option.value=item.value;option.textContent=item.text;select.append(option);});label.append(select);parent.append(label);return ()=>select.value;
  }
  private form(card:HTMLElement,title:string,build:(form:HTMLFormElement)=>()=>{action:string;payload:Data},job:Job,module:string){
    const details=document.createElement('details');details.className='router-billing-disclosure';details.append(node('summary',title));
    const form=document.createElement('form');form.className='router-connect-form bj-form';details.append(form);card.append(details);const read=build(form);
    const submit=this.button('提交',()=>{});submit.type='submit';form.append(submit);
    form.addEventListener('submit',event=>{event.preventDefault();if(!form.reportValidity())return;const request=read();void this.action(job,request.action,request.payload,module,title);});
  }
  private row(parent:HTMLElement,label:string,value:unknown){const row=node('p','','router-billing-row bj-bill-row');row.append(node('span',label),node('strong',value==null || value===''?'--':String(value)));parent.append(row);}
  private table(parent:HTMLElement,data:Data){
    if(!data?.rows?.length){parent.append(node('p','暂无记录。'));return;}
    const wrap=node('div','','router-table-scroll bj-table-scroll'),table=document.createElement('table');table.className='bj-table router-record-table';const head=document.createElement('tr');
    (data.columns || []).forEach((key:string)=>head.append(node('th',labels[key] || key)));const thead=document.createElement('thead');thead.append(head);table.append(thead);
    const body=document.createElement('tbody');data.rows.forEach((row:unknown[])=>{const tr=document.createElement('tr');row.forEach((value,index)=>{
      let text=String(value ?? '--');
      if(['loginTime','logoutTime','lastLoginAt'].includes(data.columns?.[index]) && /^\d{10}(\d{3})?$/.test(text)) {
        const date=new Date(Number(text)*(text.length===10?1000:1));
        if(Number.isFinite(date.getTime()))text=date.toLocaleString('zh-CN',{hour12:false});
      }
      if(data.columns?.[index]==='billingMode')text=({'1':'时长','2':'流量','3':'包月'} as Record<string,string>)[text] || text;
      tr.append(node('td',text));
    });body.append(tr);});table.append(body);wrap.append(table);parent.append(wrap);
  }
  private draw(job:Job,module:string){
    const result=job.result,stamp=JSON.stringify(result);if(job.stamp===stamp)return;job.stamp=stamp;
    animateResize(job.card,()=>{
      const card=job.card;card.replaceChildren(node('h3',`账号 ${result.account || job.target.title}`),node('p',result.message || '等待查询','router-billing-status'));
      if(result.state==='loading'){const bar=document.createElement('progress');bar.max=100;bar.value=result.percent || 2;card.append(bar);return;}
      if(result.state==='captcha'){this.captcha(job,module);return;}
      if(result.state!=='ready'){card.append(this.button('重新查询',()=>{void this.load(module,this.jobs.map(j=>j.target));}));return;}
      if(!result.module){this.options.changed();card.append(this.button('刷新此模块',()=>{void this.load(module,this.jobs.map(j=>j.target));}));return;}
      if(module==='services'){
        this.row(card,'当前套餐',result.currentPackage);
        this.row(card,'消费限额',result.installmentFlag==='999999'?'不限制':result.installmentFlag?`${result.installmentFlag} 元`:'--');this.row(card,'本周期消费',result.useMoney?`${result.useMoney} 元`:'--');
        if(result.scheduledPackage)this.row(card,'已预约套餐',result.scheduledPackage);
        const tools=node('div','','router-actions bj-actions');
        if(result.canStop)tools.append(this.button('立即报停',()=>{void this.action(job,'stopNow',{},module,'立即报停');}));
        if(result.canReopen)tools.append(this.button('立即复通',()=>{void this.action(job,'reopenNow',{},module,'立即复通');}));
        if(result.canCancel)tools.append(this.button('取消套餐预约',()=>{void this.action(job,'cancelPackage',{},module,'取消套餐预约');}));card.append(tools);
        if(result.packages?.length) {
          card.append(node('h4','预约套餐'));
          const normalize=(value:unknown)=>String(value || '').replace(/\s+/g,'').toLowerCase();
          const current=String(result.currentPackageId || result.packages.find((p:Data)=>normalize(p.name)===normalize(result.currentPackage))?.id || '');
          const next=String(result.packages.find((p:Data)=>normalize(p.name)===normalize(result.scheduledPackage))?.id || current);
          let selected=next;
          const list=node('div','','billing-package-grid bj-package-grid');list.setAttribute('role','radiogroup');list.setAttribute('aria-label','下周期套餐');
          const submit=this.button('确认预约套餐',()=>{void this.action(job,'schedulePackage',{packageId:selected},module,'预约套餐');});
          const sync=()=>{submit.disabled=!selected || selected===next || selected===current;list.querySelectorAll<HTMLButtonElement>('button').forEach(button=>{button.setAttribute('aria-checked',String(button.dataset.package===selected));button.classList.toggle('selected',button.dataset.package===selected);});};
          for(const p of result.packages) {
            const choice=this.button('',()=>{selected=String(p.id);sync();});choice.className='billing-package-option bj-package-option';choice.dataset.package=String(p.id);choice.setAttribute('role','radio');
            choice.append(node('strong',String(p.name)),node('p',String(p.description || '计费系统未提供套餐说明')));
            const badges=node('div','','billing-package-badges');if(String(p.id)===current)badges.append(node('span','当前周期','billing-package-badge'));if(String(p.id)===next)badges.append(node('span','下一周期','billing-package-badge next'));choice.append(badges);list.append(choice);
          }
          sync();card.append(list,submit);
        }
        this.form(card,'消费保护',form=>{const limit=this.field(form,'消费限额（元，999999 为不限制）','number',result.installmentFlag || '999999');limit.min='0';limit.max='999999';limit.step='0.01';return ()=>({action:'setConsumeLimit',payload:{consumeLimit:limit.value}});},job,module);
      }else if(module==='devices'){
        const sessions=result.sessions || [];card.append(node('h4','在线设备'));
        if(!sessions.length)card.append(node('p',result.sessionsAvailable?'暂无在线设备。':'在线设备暂时无法查询。'));
        for(const session of sessions){const device=node('div','','router-device-item bj-device');this.row(device,'IP',[session.ip,session.ipv6].filter(Boolean).join(' · '));this.row(device,'MAC',session.mac);this.row(device,'在线时长',session.useTime?`${Math.round(Number(session.useTime)/60)} 分钟`:'--');device.append(this.button('下线',()=>{void this.action(job,'disconnectSession',{sessionIndex:String(session.sessionIndex)},module,'下线此设备');}));card.append(device);}
        if(result.mauth==='0' || result.mauth==='1'){this.row(card,'无感认证',result.mauth==='1'?'已开启':'已关闭');card.append(this.button(result.mauth==='1'?'关闭无感认证':'开启无感认证',()=>{void this.action(job,'setMauth',{enabled:result.mauth==='1'?'0':'1'},module,'更改无感认证');}));}
        card.append(node('h4','已绑定设备'));this.table(card,result.macs);
        for(const row of result.macs?.rows || [])if(row[1])card.append(this.button(`解绑 ${row[1]}`,()=>{void this.action(job,'unbindMac',{mac:String(row[1])},module,'解绑此设备');}));
        this.form(card,'绑定设备',form=>{const mac=this.field(form,'MAC 地址');mac.maxLength=32;return ()=>({action:'bindMac',payload:{mac:mac.value}});},job,module);
        card.append(this.button('账号安全',()=>{void this.load('security',this.jobs.map(j=>j.target));}));
      }else if(module==='security'){
        this.form(card,'修改计费密码',form=>{const rules=[result.FLDREQUIREUPPER?'大写字母':'',result.FLDREQUIRELOWER?'小写字母':'',result.FLDREQUIREDIGIT?'数字':'',result.FLDREQUIRESPECIAL?'特殊符号':''].filter(Boolean);form.append(node('p',`${result.FLDMINLENGTH || 8}–16 位，不含空格${rules.length?'，含'+rules.join('、'):''}`));const old=this.field(form,'当前密码','password'),next=this.field(form,'新密码','password');next.minLength=Math.max(1,result.FLDMINLENGTH || 8);next.maxLength=16;const repeat=this.field(form,'再次输入新密码','password');repeat.maxLength=16;repeat.addEventListener('input',()=>repeat.setCustomValidity(repeat.value===next.value?'':'两次密码不一致'));next.addEventListener('input',()=>repeat.setCustomValidity(repeat.value===next.value?'':'两次密码不一致'));return ()=>({action:'changePassword',payload:{oldPassword:old.value,newPassword:next.value}});},job,module);
        if(result.questions?.length)this.form(card,'密码保护',form=>{const old=this.field(form,'当前密码','password'),questions:(()=>string)[]=[],answers:HTMLInputElement[]=[];for(let i=0;i<3;i++){questions.push(this.choice(form,`问题 ${i+1}`,result.questions.map((q:Data)=>({value:q.id,text:q.name}))));const answer=this.field(form,`答案 ${i+1}`);answer.maxLength=16;answers.push(answer);}return ()=>({action:'updateQuestions',payload:Object.fromEntries([['oldPassword',old.value],...questions.map((q,i)=>[`question${i+1}`,q()]),...answers.map((a,i)=>[`answer${i+1}`,a.value])])});},job,module);
        card.append(this.button('返回设备列表',()=>{void this.load('devices',this.jobs.map(j=>j.target));}));
      }else if(module==='history')this.table(card,result.history);
    });
  }
  private async action(job:Job,operation:string,payload:Data,module:string,title:string){
    if(job.running || this.pending.has(job))return;this.pending.add(job);
    const generation=this.generation,sourceTicket=job.ticket;
    try{if(!await this.options.confirm(`账号 ${job.result.account || job.target.title}：${title}？`) || generation!==this.generation)return;const response=await this.options.request('billing_action',{ticket:sourceTicket,operation,payload:JSON.stringify(payload)});if(generation!==this.generation)return;job.ticket=response.ticket;job.running=true;job.result={state:'loading',message:'正在提交操作…',percent:2};this.draw(job,module);}
    catch(error){if(generation===this.generation)job.card.append(node('p',String(error),'error'));}
    finally{this.pending.delete(job);}
  }
  private captcha(job:Job,module:string){
    const image=document.createElement('img');image.alt='计费验证码';if(/^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/=]+$/.test(job.result.image || ''))image.src=job.result.image;
    const input=this.field(job.card,'验证码');input.maxLength=12;job.card.append(image);
    const answer=async(refresh:boolean)=>{if(job.running || (!refresh && !input.value.trim()))return;const generation=this.generation;
      try{await this.options.request('billing_captcha',{ticket:job.ticket,code:input.value.trim(),refresh});if(generation!==this.generation)return;job.running=true;job.result={state:'loading',message:'正在核对验证码…',percent:40};this.draw(job,module);}catch(error){job.card.append(node('p',String(error),'error'));}};
    job.card.append(this.button('换一张',()=>{void answer(true);}),this.button('继续',()=>{void answer(false);}));
  }
}
