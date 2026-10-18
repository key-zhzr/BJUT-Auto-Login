import { animateResize } from './layout-motion';
import { CustomSelect } from './custom-select';
import { invoke } from '@tauri-apps/api/core';
import { IS_ANDROID } from './platform';

export interface RouterRecordTarget { uplink: string; account: string; title: string; }
interface RecordResult {state:string;message?:string;percent?:number;image?:string;account?:string;kind?:string;total?:number;page?:number;columns?:string[];rows?:string[][];}
interface RecordJob {target:RouterRecordTarget;ticket:string;running:boolean;item:RecordResult;card:HTMLElement;}
const kinds=[['usage','上网明细'],['monthly','月账单'],['payments','缴费记录'],['operations','操作记录'],['stopLogs','停机记录'],['reopenLogs','复机记录'],['packageLogs','套餐变更']];
const labels:Record<string,string>={loginTime:'登录时间',logoutTime:'下线时间',time:'在线时长（分钟）',flow:'流量（MB）',costMoney:'费用（元）',internetUpFlow:'互联网上传',internetDownFlow:'互联网下载',chinanetUpFlow:'校园网上传',chinanetDownFlow:'校园网下载',userIp:'IPv4',userIp1:'IPv6',macAddress:'MAC',nasIp:'网关',nasPort:'端口',startAt:'开始日期',endAt:'结束日期',package:'套餐',baseFee:'基本费（元）',usageFee:'用量费（元）',durationMinutes:'在线分钟',flowMb:'流量（MB）',billedAt:'结算时间',paidAt:'缴费时间',type:'类型',amount:'金额（元）',terminal:'终端',note:'备注',operatedAt:'操作时间',description:'说明',unused:'其他',fldoperatedate:'操作日期',fldoperateid:'操作',fldadminid:'操作员',fldmemo:'备注',fldnewvalue:'内容',fldchangedate:'变更日期',fldexcutedate:'生效日期',flddefaultname1:'原套餐',flddefaultname2:'新套餐',fldstate:'状态',fldstatedate:'状态日期',fldextend:'备注'};
const el=(tag:string,text='',className='')=>{const e=document.createElement(tag);e.textContent=text;e.className=className;return e;};
function btn(text:string,action:()=>void){const b=el('button',text,'btn btn-secondary btn-sm') as HTMLButtonElement;b.type='button';b.addEventListener('click',action);return b;}

export class RouterRecords {
  private jobs:RecordJob[]=[];
  private generation=0;
  private busy=false;
  private polling=false;
  private supported=false;
  private kind:CustomSelect;
  private start=document.createElement('input');
  private end=document.createElement('input');
  private year=document.createElement('input');
  private query:HTMLButtonElement;
  private message=el('p','','router-billing-status');
  private results=el('div','','router-record-results');
  constructor(root:HTMLElement,private options:{request:<T>(name:string,args?:Record<string,unknown>)=>Promise<T>;targets:()=>RouterRecordTarget[]}){
    const select=el('div','','custom-select');select.id='router-record-kind';select.setAttribute('aria-label','账单类型');select.innerHTML='<div class="custom-select-trigger"><span>上网明细</span></div><div class="custom-select-options"></div>';
    const controls=el('div','','router-record-controls');root.append(controls,this.message,this.results);
    const field=(name:string,input:HTMLElement)=>{const wrapper=el('label','','router-editor-field');wrapper.append(el('span',name),input);return wrapper;};
    this.start.type=this.end.type='date';this.start.id='router-record-start';this.end.id='router-record-end';this.year.type='number';this.year.min='2000';this.year.max='2199';this.year.value=String(new Date().getFullYear());this.year.id='router-record-year';
    const now=new Date(),first=new Date(now.getFullYear(),now.getMonth(),1);const date=(d:Date)=>`${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`;
    this.start.value=date(first);this.end.value=date(now);
    this.query=btn('查询',()=>{void this.load();});this.query.id='btn-router-record-query';
    controls.append(field('类型',select),field('开始日期',this.start),field('结束日期',this.end),field('年份',this.year),this.query);
    this.kind=new CustomSelect(select.id);this.kind.setOptions(kinds.map(([value,text])=>({value,text})));this.kind.value='usage';
    const clear=()=>{if(this.busy)return;this.jobs=[];this.results.replaceChildren();this.message.textContent='选择类型和日期后查询。';};
    this.kind.addEventListener('change',()=>{this.fields();clear();});for(const input of [this.start,this.end,this.year])input.addEventListener('change',clear);this.fields();
    window.setInterval(()=>{if(!document.hidden && this.supported)void this.poll();},2000);
  }
  setSupported(value:boolean){if(this.supported===value && this.message.textContent)return;this.supported=value;this.query.disabled=!value || this.busy;this.message.textContent=value?'选择类型和日期后查询。':'请更新路由端后查询用量与账单。';}
  disconnect(){this.generation++;this.jobs=[];this.busy=false;this.supported=false;this.query.disabled=true;this.kind.setDisabled(false);for(const input of [this.start,this.end,this.year])input.disabled=false;this.message.textContent='';this.results.replaceChildren();}
  private fields(){const dates=['usage','payments','operations'].includes(this.kind.value);this.start.parentElement!.hidden=this.end.parentElement!.hidden=!dates;this.year.parentElement!.hidden=this.kind.value!=='monthly';}
  private async load(target?:RouterRecordTarget,page=1){
    if(this.busy || !this.supported)return;
    const kind=this.kind.value,dates=['usage','payments','operations'].includes(kind);
    if(dates && (!this.start.value || !this.end.value || this.start.value>this.end.value)){this.message.textContent='请检查日期范围。';return;}
    if(kind==='monthly' && (!/^\d{4}$/.test(this.year.value) || +this.year.value<2000 || +this.year.value>2199)){this.message.textContent='请检查年份。';return;}
    const targets=target?[target]:this.options.targets();if(!targets.length){this.message.textContent='请先配置上游并查询计费账号。';return;}
    const generation=++this.generation;this.busy=true;this.query.disabled=true;
    const previous=target?this.jobs.filter(job=>job.target.uplink!==target.uplink):[];
    this.jobs=[...previous,...targets.map(item=>({target:item,ticket:'',running:true,item:{state:'loading',message:'正在准备查询',percent:2},card:el('article','','glass-card router-record-card router-enter')}))];
    this.results.replaceChildren(...this.jobs.map(job=>job.card));this.draw();
    await Promise.all(this.jobs.filter(job=>job.running).map(async job=>{
      try{const response=await this.options.request<{ticket:string}>('billing_records',{uplink:job.target.uplink,account:job.target.account,kind,start_date:dates?this.start.value:'',end_date:dates?this.end.value:'',year:kind==='monthly'?this.year.value:'',page:String(page)});if(generation===this.generation)job.ticket=response.ticket;}
      catch(error){if(generation===this.generation){job.running=false;job.item={state:'error',message:String(error)};}}
    }));
    if(generation===this.generation){this.draw();await this.poll();}
  }
  private async poll(){
    if(this.polling || !this.jobs.some(job=>job.running && job.ticket))return;
    const generation=this.generation;this.polling=true;
    try{await Promise.all(this.jobs.filter(job=>job.running && job.ticket).map(async job=>{
      try{const response=await this.options.request<{result:RecordResult}>('billing_poll',{ticket:job.ticket});if(generation!==this.generation)return;job.item=response.result;job.running=job.item.state==='loading';}
      catch(error){if(generation===this.generation){job.running=false;job.item={state:'error',message:String(error)};}}
    }));if(generation===this.generation)this.draw();}finally{this.polling=false;}
  }
  private draw(){animateResize(this.results,()=>this.drawRows());}
  private drawRows(){
    this.busy=this.jobs.some(job=>job.running);this.query.disabled=this.busy || !this.supported;this.kind.setDisabled(this.busy);for(const input of [this.start,this.end,this.year])input.disabled=this.busy;
    this.message.textContent=this.busy?'正在查询用量与账单…':'查询已完成';
    const seen=new Set<string>();
    for(const job of this.jobs){
      const item=job.item,stamp=JSON.stringify(item);
      if(item.state==='ready' && item.account && seen.has(item.account)){job.card.hidden=true;continue;}if(item.state==='ready' && item.account)seen.add(item.account);job.card.hidden=false;
      if(job.card.dataset.result===stamp)continue;
      job.card.dataset.result=stamp;job.card.replaceChildren(el('h3',item.account?`账号 ${item.account}`:job.target.title),el('p',item.message || '等待查询','router-billing-status'));
      if(item.state==='loading'){const progress=document.createElement('progress');progress.max=100;progress.value=item.percent || 2;job.card.append(progress);}
      if(item.state==='captcha')this.captcha(job);
      if(item.state==='ready'){
        const columns=item.columns || [],rows=item.rows || [],table=el('table','','router-record-table'),head=el('thead'),body=el('tbody');
        const heading=el('tr');for(const key of columns)heading.append(el('th',labels[key] || '其他'));head.append(heading);
        for(const row of rows){const tr=el('tr');for(let i=0;i<columns.length;i++)tr.append(el('td',String(row[i] || '--')));body.append(tr);}table.append(head,body);const scroll=el('div','','router-table-scroll');scroll.append(table);job.card.append(scroll);
        if(!rows.length)job.card.append(el('p','暂无记录'));
        const page=item.page || 1,pages=Math.max(1,Math.ceil((item.total || 0)/100)),tools=el('div','','router-actions');
        const prev=btn('上一页',()=>{void this.load(job.target,page-1);}),next=btn('下一页',()=>{void this.load(job.target,page+1);});prev.disabled=page<=1 || this.busy;next.disabled=page>=pages || this.busy;
        tools.append(el('span',`${item.total || 0} 条 · ${page} / ${pages}`),prev,next,btn('导出本页',()=>this.export(item)));job.card.append(tools);
      }
    }
  }
  private captcha(job:RecordJob){
    const image=document.createElement('img');image.alt='计费系统验证码';if(job.item.image && /^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/=]+$/.test(job.item.image))image.src=job.item.image;
    const input=document.createElement('input');input.placeholder='验证码';input.maxLength=12;input.autocomplete='off';input.className='router-captcha-input';
    const generation=this.generation;const answer=async(refresh:boolean)=>{if(job.running || (!refresh && !input.value.trim()))return;job.running=true;
      try{await this.options.request('billing_captcha',{ticket:job.ticket,code:input.value.trim(),refresh});if(generation===this.generation){job.item={state:'loading',message:'正在核对验证码',percent:40};this.draw();}}
      catch(error){if(generation===this.generation){job.running=false;this.message.textContent=String(error);}}
    };const actions=el('div','','router-actions');actions.append(btn('换一张',()=>{void answer(true);}),btn('继续',()=>{void answer(false);}));job.card.append(image,input,actions);
  }
  private async export(item:RecordResult){
    const quote=(value:string)=>`"${(/^[=+\-@\t\r]/.test(value)?`'${value}`:value).replaceAll('"','""')}"`;
    const lines=[(item.columns || []).map(key=>labels[key] || '其他'),...(item.rows || [])].map(row=>row.map(quote).join(',')).join('\r\n');
    if(window.__TAURI__){
      try{
        const path=await invoke<string>('export_billing_csv',{kind:item.kind,csv:'\ufeff'+lines});
        if(IS_ANDROID && window.AndroidBridge?.shareExportFile){if(!window.AndroidBridge.shareExportFile(path,'BJUT-AL 路由计费记录'))throw new Error('系统分享窗口启动失败');}
        else this.message.textContent=`CSV 已导出到：${path}`;
      }catch(error){this.message.textContent=`导出未完成：${String(error)}`;}
      return;
    }
    const url=URL.createObjectURL(new Blob(['\ufeff',lines],{type:'text/csv;charset=utf-8'})),link=document.createElement('a');link.href=url;link.download=`计费记录-${item.kind}-第${item.page || 1}页.csv`;link.click();window.setTimeout(()=>URL.revokeObjectURL(url),1000);
  }
}
