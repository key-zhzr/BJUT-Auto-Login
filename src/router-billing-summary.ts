import type { RouterBillingJob, RouterSnapshot } from './router-model';
import { routerBillingSummary } from './router-model';
import { animateResize } from './layout-motion';

const node=(tag:string,text='',className='')=>{const element=document.createElement(tag);element.textContent=text;element.className=className;return element;};
const stamps=new WeakMap<HTMLElement,string>();
const number=(value:number|null,unit:string,scale=1)=>value===null || !Number.isFinite(value)?'--':`${(value/scale).toFixed(2)} ${unit}`;
export function renderRouterBillingSummary(root:HTMLElement,jobs:RouterBillingJob[],snapshot:RouterSnapshot|null):void {
  const stamp=JSON.stringify([jobs.map(job=>[job.item,job.running,job.starting]),snapshot?.uplinks.map(item=>[item.id,item.device,item.network])]);
  if(stamps.get(root)===stamp && root.childElementCount>0)return;stamps.set(root,stamp);
  const totals=routerBillingSummary(jobs),metrics=node('div','','billing-center-summary');
  for(const [label,value] of [['已查询账号',String(totals.accounts)],['余额合计',number(totals.balance,'元')],['总流量',number(totals.totalFlow,'GB',1024)],['剩余流量',number(totals.remainingFlow,'GB',1024)]]) {
    const card=node('article','','billing-summary-card glass-card');card.append(node('span',label),node('strong',value));metrics.append(card);
  }
  const cards=node('div','','router-summary-grid');
  for(const job of jobs) {
    const uplink=snapshot?.uplinks.find(item=>item.id===job.uplink),item=job.item,card=node('article','','glass-card router-billing-card');
    card.append(node('h3',[uplink?.device,uplink?.network].filter(Boolean).join(' · ') || job.uplink),node('p',item.message || '正在查询…','router-billing-status'));
    if(job.running || job.starting){const bar=document.createElement('progress');bar.max=100;bar.value=item.percent || 2;card.append(bar);}
    const metric=(key:string,value:string|undefined)=>{const row=node('div','','router-billing-row');row.append(node('span',key),node('strong',value || '--'));card.append(row);};
    metric('账号',item.account);metric('余额',item.balanceText || (item.balance?.trim()?number(Number(item.balance),'元'):''));
    metric('总流量',item.totalFlow?.trim()?number(Number(item.totalFlow),'GB',1024):'');
    metric('剩余流量',item.remainingFlowText || (item.remainingFlow?.trim()?number(Number(item.remainingFlow),'GB',1024):''));
    metric('状态',item.accountStatus);metric('套餐',item.package);cards.append(card);
  }
  animateResize(root,()=>root.replaceChildren(metrics,cards));
}
