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
    box.style.bottom='';box.style.top='56px';trigger.click();await frame();
    const ancestor=document.getElementById('main-content')!,beforeScroll=ancestor.scrollTop;
    for(const delta of [5000,5000,-5000,-5000]) {
      menu.dispatchEvent(new WheelEvent('wheel',{deltaY:delta,bubbles:true,cancelable:true}));await frame();
      assert(wrapper.classList.contains('open'),'菜单滚动到边界后收起');assert(ancestor.scrollTop===beforeScroll,'菜单边界滚动传递到了页面');
    }
    select.close();await until(()=>menu.hidden);
    note('通过：下拉菜单在顶端、底端继续滚动时保持展开且不带动页面');

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
    const main=document.getElementById('main-content')!,previous=main.querySelector<HTMLElement>('.page.active');
    const page=document.createElement('section');page.className='page active';page.innerHTML='<div class="glass-card">滚动布局</div>';
    previous?.classList.remove('active');main.append(page);
    try {
      for(const value of ['basic','apple27','winui']) {
        document.documentElement.dataset.theme=value;await frame();await frame();
        // Theme changes animate navigation geometry. Measure scrollbars only
        // after that unrelated transition has reached its final width.
        await document.fonts.ready;
        await until(()=>document.getElementById('nav')!.getAnimations().length===0);
        const card=page.firstElementChild as HTMLElement,short=card.getBoundingClientRect(),bounds=main.getBoundingClientRect();
        card.style.height='2400px';await frame();await frame();const tall=card.getBoundingClientRect();
        assert(Math.abs(short.left-tall.left)<1 && Math.abs(short.width-tall.width)<1,`${value} 长短页面宽度或横向位置不同：${JSON.stringify({short:{left:short.left,width:short.width},tall:{left:tall.left,width:tall.width}})}`);
        assert(Math.abs((tall.left+tall.right-bounds.left-bounds.right)/2)<1.5,`${value} 滚动条影响内容居中`);
        card.style.height='';
        const overlay=document.createElement('div');overlay.className='modal-overlay';overlay.innerHTML='<div class="modal-content glass-card"><h3>弹窗</h3><div>内容</div></div>';document.body.append(overlay);
        try {
          for(const height of ['20px','1800px']) {
            const content=overlay.querySelector<HTMLElement>('.modal-content')!,body=content.lastElementChild as HTMLElement;body.style.height=height;await frame();await frame();
            const outer=content.getBoundingClientRect(),inner=body.getBoundingClientRect();
            assert(Math.abs((inner.left-outer.left)-(outer.right-inner.right))<1.5,`${value} 弹窗左右内边距不等宽`);
          }
        } finally {overlay.remove();}
      }
    } finally {page.remove();previous?.classList.add('active');}
    note('通过：三种主题长短页面宽度一致、内容居中，弹窗有无滚动条时边距等宽');
  }finally{select.close();box.remove();document.documentElement.dataset.theme=theme;}
}
