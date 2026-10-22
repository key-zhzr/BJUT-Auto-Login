import { animateResize } from './layout-motion';

type Info={platform:{version:string;target:string;arch:string;format:string;lastJob?:string;rollback?:string};backups:{id:string;createdAt:string;version:string}[]};
type Job={state:string;message:string;percent?:number;release?:string;ticket?:string};
export class RouterMaintenance {
  private root:HTMLElement|null=null;
  private info:Info|null=null;
  private prepared='';
  private generation=0;
  private busy=false;
  constructor(private options:{request:<T>(operation:string,args?:Record<string,unknown>)=>Promise<T>;confirm:(message:string,title:string,label:string)=>Promise<boolean>}){}
  close(){this.generation++;this.root=null;this.info=null;this.prepared='';this.busy=false;}
  resume(){if(this.root && !this.busy)void this.load();}
  mount(parent:HTMLElement){
    const root=document.createElement('section');root.className='router-maintenance';
    const title=document.createElement('h4');title.textContent='路由器备份与更新';
    const text=document.createElement('p');text.dataset.info='';
    const status=document.createElement('p');status.setAttribute('role','status');status.dataset.status='';
    const progress=document.createElement('progress');progress.max=100;progress.value=0;progress.hidden=true;
    const actions=document.createElement('div');actions.className='router-actions';
    for(const [action,label] of [['backup','备份配置'],['check','检查更新'],['prepare','准备安装包'],['upgrade','安装更新'],['rollback','回退版本']]) {
      const button=document.createElement('button');button.type='button';button.className='btn btn-secondary btn-sm';button.textContent=label;button.dataset.action=action;
      button.addEventListener('click',()=>{void this.run(action);});actions.append(button);
    }
    const backups=document.createElement('div');backups.dataset.backups='';
    root.append(title,text,actions,progress,status,backups);parent.append(root);this.root=root;
    void this.load();
  }
  private setBusy(busy:boolean){this.busy=busy;this.root?.querySelectorAll<HTMLButtonElement>('button').forEach(button=>button.disabled=busy || button.dataset.action==='upgrade' && !this.prepared || button.dataset.action==='rollback' && !this.info?.platform.rollback);}
  private message(text:string){const target=this.root?.querySelector('[data-status]');if(target)target.textContent=text;}
  private async load(){
    const generation=this.generation;
    try{
      const data=await this.options.request<Info>('maintenance_info');if(generation!==this.generation || !this.root)return;
      this.info=data;this.root.querySelector('[data-info]')!.textContent=`${data.platform.version} · ${data.platform.target} · ${data.platform.arch} · ${data.platform.format.toUpperCase()}`;
      const container=this.root.querySelector<HTMLElement>('[data-backups]')!;
      animateResize(container,()=>{
        container.replaceChildren();
        for(const backup of data.backups.slice().reverse()){
          const row=document.createElement('div');row.className='router-account-row';
          const text=document.createElement('span');text.textContent=`${backup.createdAt} · ${backup.version}`;
          const button=document.createElement('button');button.type='button';button.className='btn btn-secondary btn-sm';button.textContent='恢复配置';button.addEventListener('click',()=>{void this.run('restore',backup.id);});
          row.append(text,button);container.append(row);
        }
      });this.setBusy(this.busy);
      if(data.platform.lastJob && !this.busy){
        const job=await this.options.request<Job>('maintenance_poll',{ticket:data.platform.lastJob});
        if(generation!==this.generation)return;
        this.message(job.message);
        if(job.state==='prepared'){this.prepared=data.platform.lastJob;this.setBusy(false);}
        if(job.state==='loading'){this.setBusy(true);void this.wait(data.platform.lastJob,generation).catch(error=>{if(generation===this.generation)this.message(String(error));}).finally(()=>{if(generation===this.generation)this.setBusy(false);});}
      }
    }catch(error){if(generation===this.generation)this.message(String(error));}
  }
  private async run(action:string,backup=''){
    if(this.busy || !this.root)return;
    if(['upgrade','rollback','restore'].includes(action) && !await this.options.confirm(action==='restore'?'恢复该备份中的路由配置？当前配置会先自动备份。':action==='upgrade'?'安装已校验的配套安装包？路由协作可能短暂断开。':'回退到升级前的配套版本？当前配置会先备份。','路由器维护','继续'))return;
    this.setBusy(true);const generation=this.generation;
    try{
      const start=await this.options.request<Job>('maintenance_start',{action,backup,prepared:this.prepared});
      if(!start.ticket)throw new Error(start.message || '无法开始维护');
      await this.wait(start.ticket,generation);
      if(generation===this.generation)await this.load();
    }catch(error){if(generation===this.generation)this.message(String(error));}
    finally{if(generation===this.generation)this.setBusy(false);}
  }
  private async wait(ticket:string,generation:number){
    const deadline=Date.now()+15*60*1000;
    while(Date.now()<deadline && generation===this.generation && this.root){
      const result=await this.options.request<Job>('maintenance_poll',{ticket});
      if(generation!==this.generation || !this.root)return;
      this.message(`${result.release?result.release+' · ':''}${result.message}`);
      const progress=this.root.querySelector('progress')!;animateResize(this.root,()=>{progress.hidden=result.state!=='loading';progress.value=result.percent || 0;});
      if(result.state==='error')throw new Error(result.message);
      if(result.state==='prepared'){this.prepared=ticket;return;}
      if(result.state==='ready')return;
      await new Promise(resolve=>window.setTimeout(resolve,1500));
    }
    if(generation===this.generation)throw new Error('维护等待超时，请重新读取路由器状态。');
  }
}
