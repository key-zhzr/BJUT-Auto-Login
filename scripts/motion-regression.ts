import { CustomSelect } from '../src/custom-select';
import { AnimatedVisibility } from '../src/animated-visibility';
import { setExpanded } from '../src/layout-motion';

export async function checkRouterDropdownLayout(note:(text:string)=>void){
  const assert=(value:boolean,text:string)=>{if(!value)throw new Error(text);};
  const frame=()=>new Promise<void>(resolve=>requestAnimationFrame(()=>resolve()));
  const main=document.getElementById('main-content')!,header=document.querySelector<HTMLElement>('#settings > .page-header')!;
  const details=document.getElementById('router-service-advanced') as HTMLDetailsElement;
  const trigger=document.querySelector<HTMLElement>('#router-service-more [aria-label="认证方式"] .custom-select-trigger')!;
  const theme=document.documentElement.dataset.theme,scroll=main.scrollTop;
  document.querySelector<HTMLElement>('[data-target="settings"]')!.click();
  setExpanded(details,true);
  while(details.getAnimations().length)await frame();
  try{
    for(const value of ['basic','apple27','winui']){
      document.documentElement.dataset.theme=value;await frame();
      trigger.scrollIntoView({block:'center'});await frame();const before=main.scrollTop;
      trigger.click();for(let i=0;i<22;i++)await frame();
      const menu=trigger.parentElement!.querySelector<HTMLElement>('.custom-select-options')!,option=menu.querySelector<HTMLElement>('.selected')!;
      const title=header.querySelector('h1')!.getBoundingClientRect(),hit=document.elementFromPoint(title.left+title.width/2,title.top+title.height/2);
      assert(!!hit && header.contains(hit),`${value} 路由设置菜单遮住顶栏`);
      assert(Math.abs(main.scrollTop-before)<1,`${value} 展开菜单移动了页面`);
      const outer=menu.getBoundingClientRect(),inner=option.getBoundingClientRect();
      const style=getComputedStyle(menu);
      assert(inner.left-outer.left-parseFloat(style.paddingLeft)<=2 && outer.right-inner.right-parseFloat(style.paddingRight)<=2,`${value} 菜单选项两侧出现空白`);
      trigger.click();for(let i=0;i<22;i++)await frame();
      assert(Math.abs(main.scrollTop-before)<1,`${value} 收起菜单移动了页面`);
    }
    const body=document.body.getBoundingClientRect();
    assert(Math.abs(body.left)<1 && Math.abs(body.right-window.innerWidth)<1,'应用未铺满窗口，两侧露出背景');
    note('通过：三种主题展开路由设置菜单时顶栏可见、页面不跳动、选项无白边，窗口无侧边留白');
  }finally{
    if(trigger.getAttribute('aria-expanded')==='true')trigger.click();
    document.documentElement.dataset.theme=theme;main.scrollTop=scroll;
  }
}

export async function runMotionRegression(note:(text:string)=>void){
  const assert=(value:boolean,text:string)=>{if(!value)throw new Error(text);};
  const frame=()=>new Promise<void>(resolve=>requestAnimationFrame(()=>resolve()));
  const until=async(test:()=>boolean)=>{const end=performance.now()+1500;while(!test()){if(performance.now()>end)throw new Error('动画没有完成');await frame();}};
  const box=document.createElement('section');box.className='glass-card';box.style.cssText='position:fixed;left:50%;bottom:36px;transform:translateX(-50%);width:300px;padding:16px;z-index:40000';
  const wrapper=document.createElement('div');wrapper.id='qa-placement';wrapper.className='custom-select';wrapper.innerHTML='<div class="custom-select-trigger"><span></span><span>⌄</span></div><div class="custom-select-options"></div>';
  box.append(wrapper);document.body.append(box);const select=new CustomSelect(wrapper.id);
  select.setOptions(Array.from({length:12},(_,i)=>({value:String(i),text:`选项 ${i+1}`})));
  const trigger=wrapper.querySelector<HTMLElement>('.custom-select-trigger')!,menu=wrapper.querySelector<HTMLElement>('.custom-select-options')!;
  const theme=document.documentElement.dataset.theme;
  try{
    for(const value of ['basic','apple27','winui']){
      document.documentElement.dataset.theme=value;
      trigger.click();assert(wrapper.classList.contains('opens-up'),`${value} 空间不足时没有预先向上定位`);
      for(let i=0;i<12;i++){await frame();assert(menu.getBoundingClientRect().bottom<=trigger.getBoundingClientRect().top+12,`${value} 展开时改变了方向`);}
      select.close();assert(wrapper.classList.contains('opens-up'),`${value} 收起时丢失方向`);
      await frame();assert(menu.getBoundingClientRect().bottom<=trigger.getBoundingClientRect().top+12,`${value} 收起时跳到了下方`);
      trigger.click();await frame();assert(!menu.hidden,`${value} 快速重开被旧计时器隐藏`);
      select.close();await until(()=>menu.hidden);
      box.style.bottom='';box.style.top='56px';trigger.click();assert(!wrapper.classList.contains('opens-up'),`${value} 充足空间时未向下展开`);select.close();await until(()=>menu.hidden);box.style.top='';box.style.bottom='36px';
    }
    note('通过：三种主题的下拉方向在打开、收起和快速重开时保持稳定');
    document.documentElement.dataset.theme='basic';box.style.top='80px';box.style.bottom='';
    const details=document.createElement('details');details.innerHTML='<summary>展开详细信息</summary><div style="height:180px;padding:16px">测试内容</div>';box.append(details);
    const closed=details.getBoundingClientRect().height;
    setExpanded(details,true);await frame();assert(details.getAnimations().length>0,'详细信息缺少高度动画');
    setExpanded(details,false);await frame();setExpanded(details,true);await until(()=>details.getAnimations().length===0);
    assert(details.open && details.getBoundingClientRect().height>closed+100,'连续点击展开时最终状态错误');
    setExpanded(details,false);await until(()=>!details.open);assert(Math.abs(details.getBoundingClientRect().height-closed)<1,'收起后留下空白');
    assert(details.style.overflow==='','动画结束后仍在裁切阴影');
    const panel=document.createElement('section');panel.className='glass-card';panel.hidden=true;panel.innerHTML='<p>显示内容</p>';box.append(panel);const visibility=new AnimatedVisibility(panel);
    visibility.setVisible(true);await frame();visibility.setVisible(false);await frame();visibility.setVisible(true);await until(()=>panel.getAnimations().length===0);assert(!panel.hidden,'显隐反向时被过期动画隐藏');
    visibility.setVisible(false);await until(()=>panel.hidden);assert(!panel.classList.contains('progress-animating'),'收起后保留动画样式');
    note('通过：详细信息和状态模块可平滑反向，结束后无空白和阴影裁切');
    const main=document.getElementById('main-content')!;assert(getComputedStyle(main).scrollbarGutter.includes('both-edges'),'滚动条未预留对称空间');
    note('通过：滚动条使用对称预留空间');
  }finally{select.close();box.remove();document.documentElement.dataset.theme=theme;}
}
