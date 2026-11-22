import {test,expect} from '@playwright/test';
for (const platform of ['macos', 'android']) {
  test(`an open dropdown follows its anchor on a narrow ${platform} window`, async ({page}) => {
    await page.setViewportSize({width: 1024, height: 768});
    await page.goto(`http://127.0.0.1:1433/scripts/app-regression.html?platform=${platform}`);
    await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
    await page.locator('[data-target=settings]').first().click();
    const appearance = page.locator('[data-settings-target=appearance]');
    if (await appearance.isVisible()) await appearance.click();
    if (platform === 'android') {
      await page.addStyleTag({content: `
        body.is-android #nav { transition: width 160ms ease, height 160ms ease !important; }
        @media (min-width: 769px) { body.is-android #nav { height: 100vh; } }
      `});
    }
    const select = page.locator('#setting-theme');
    const trigger = select.getByRole('combobox');
    const options = select.getByRole('listbox');
    await trigger.scrollIntoViewIfNeeded();
    await trigger.click();
    await expect(options).toBeVisible();
    const geometry = () => select.evaluate(element => {
      const trigger = element.querySelector('.custom-select-trigger')!.getBoundingClientRect();
      const menu = element.querySelector('.custom-select-options')!;
      const bounds = menu.getBoundingClientRect();
      const covers = bounds.top <= trigger.bottom + 1 && bounds.bottom >= trigger.top - 1;
      return {
        triggerLeft: Math.round(trigger.left), triggerRight: Math.round(trigger.right),
        menuLeft: Math.round(bounds.left), menuRight: Math.round(bounds.right),
        inside: bounds.left >= 0 && bounds.right <= innerWidth + 1 && bounds.top >= 0 && bounds.bottom <= innerHeight + 1,
        gap: covers ? 0 : Math.min(Math.abs(bounds.top - trigger.bottom), Math.abs(trigger.top - bounds.bottom)),
        open: menu.matches(':popover-open'),
      };
    });
    const before = await geometry();
    expect(before.open).toBe(true);
    await page.setViewportSize({width: 390, height: 844});
    await page.locator('#nav').evaluate(async nav => {
      await new Promise(requestAnimationFrame);
      await Promise.all(nav.getAnimations().map(animation => animation.finished.catch(() => undefined)));
    });
    await expect.poll(async () => {
      const after = await geometry();
      return after.open && after.inside && after.gap <= 8 && Math.abs(after.triggerLeft - after.menuLeft) <= 1
        && Math.abs(after.triggerRight - after.menuRight) <= 1 && after.triggerLeft !== before.triggerLeft;
    }).toBe(true);
    await trigger.press('End');
    await trigger.press('Enter');
    await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  });
}

test('the router card stays hidden without a detected plugin, including in onboarding', async ({page}) => {
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=macos&saved&no-router');
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  const card = page.locator('#router-dashboard');
  await expect(page.locator('#router-settings-connection')).toContainText('未发现路由器');
  await expect(page.locator('#router-message')).toContainText('当前网络与记住的路由器不匹配');
  await expect(card).toBeHidden();
  await page.locator('[data-target=settings]').first().click();
  await page.locator('[data-settings-target=advanced]').click();
  await page.locator('#btn-onboarding').click();
  const guide = page.locator('#onboarding-dialog');
  await guide.getByRole('button', {name: /第 4 步/}).click();
  await expect(card).toBeHidden();
  await guide.getByRole('button', {name: '打开路由协作设置'}).click();
  await expect(page.locator('[data-settings-subpage=router]')).toBeVisible();
  await expect(page.locator('#btn-router-manual-connect')).toBeVisible();
});

test('app update checks detect a newer build of the installed version', async ({page}) => {
  const base = 'https://github.com/key-zhzr/BJUT-Auto-Login/releases';
  const download = `${base}/download/v0.1.6`;
  const packageName = 'BJUT-Auto-Login_0.1.6_Windows_x64.exe';
  let releaseReads = 0;
  await page.route('https://api.github.com/repos/key-zhzr/BJUT-Auto-Login/releases?per_page=10', route => {
    releaseReads++;
    return route.fulfill({json: [{
      tag_name: 'v0.1.6', name: 'v0.1.6', body: '更新说明', html_url: `${base}/tag/v0.1.6`,
      prerelease: false, draft: false,
      assets: [
        {name: 'latest.json', browser_download_url: `${download}/latest.json`, size: 100},
        {name: packageName, browser_download_url: `${download}/${packageName}`, size: 1024},
      ],
    }]});
  });
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=windows');
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await page.locator('[data-target=settings]').first().click();
  const about = page.locator('[data-settings-target=about]');
  if (await about.isVisible()) await about.click();
  await expect(page.locator('#app-build-time')).toHaveAttribute('datetime', '2026-11-17T16:00:00.000Z');
  const check = page.locator('#btn-check-update');
  await check.click();
  await expect(page.locator('#alert-modal')).toContainText('当前已是最新版本');
  await page.locator('#btn-alert-ok').click();
  await check.click();
  await expect(page.locator('#update-modal-title')).toHaveText('发现同版本新构建 v0.1.6');
  await expect(page.locator('#update-modal-meta')).toContainText('当前编译时间');
  await expect(page.locator('#update-modal-meta')).toContainText('更新包编译时间');
  await expect(page.locator('#btn-update-confirm')).toBeVisible();
  expect(releaseReads).toBe(1);
});

test('layout motion in all themes',async({page})=>{
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=macos');
  await expect(page.locator('#qa-run')).toBeVisible();
  const lines = await page.evaluate(async()=>{
    const path='/scripts/motion-regression.ts';const module=await import(/* @vite-ignore */ path);
    const result:string[]=[];await module.runMotionRegression((text:string)=>result.push(text));return result;
  });
  expect(lines.length).toBeGreaterThan(0);
});
for(const [platform,width,height] of [['macos',1080,780],['windows',1280,900],['android',390,844]] as const){
  test(`${platform}: complete isolated workbench regression`,async({page})=>{
    await page.setViewportSize({width,height});
    await page.route('**/*',route=>new URL(route.request().url()).hostname==='127.0.0.1'?route.continue():route.abort());
    await page.goto(`http://127.0.0.1:1433/scripts/app-regression.html?platform=${platform}`);
    await page.locator('#qa-run').click();
    await expect(page.locator('#qa-results')).toContainText(/全部通过|失败：/,{timeout:150_000});
    await expect(page.locator('#qa-results')).not.toContainText('失败：');
  });
}
test('first-use guide and tray settings keep reversible drafts',async({page})=>{
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=macos');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.locator('[data-target=settings]').first().click();
  await page.locator('[data-settings-target=advanced]').click();
  await page.locator('#btn-onboarding').click();
  await expect(page.locator('#onboarding-dialog')).toBeVisible();
  await page.locator('#onboarding-dialog').getByRole('button',{name:'下一步',exact:true}).click();
  await expect(page.locator('#onboarding-dialog h4')).toHaveText('选择连接方式');
  await page.keyboard.press('Escape');
  await expect(page.locator('#onboarding-dialog')).toHaveCount(0);
  await page.locator('#btn-settings-back').click();
  await page.locator('[data-settings-target=appearance]').click();
  await page.locator('#btn-tray-preferences').click();
  await expect(page.locator('#tray-preferences-dialog')).toBeVisible();
  await page.locator('#tray-preferences-dialog select[name=icon]').selectOption('dot');
  await page.locator('#tray-preferences-dialog').getByRole('button',{name:'取消',exact:true}).click();
  await expect(page.locator('#tray-preferences-dialog')).toHaveCount(0);
  await expect(page.locator('#settings .page-header')).toBeVisible();
});
for(const [platform,width,height] of [['macos',1280,900],['android',390,844]] as const){
  test(`${platform}: opt-in billing traffic curves, interfaces and scope`,async({page})=>{
    await page.setViewportSize({width,height});
    await page.route('**/*',route=>new URL(route.request().url()).hostname==='127.0.0.1'?route.continue():route.abort());
    await page.goto(`http://127.0.0.1:1433/scripts/app-regression.html?platform=${platform}`);
    await expect(page.locator('#qa-run')).toBeVisible();
    await page.getByRole('button',{name:'隐藏验收面板',exact:true}).click();
    const panel=page.locator('#device-usage-panel');
    await expect(panel.getByRole('button',{name:'开启统计',exact:true})).toBeVisible();
    await expect(panel.locator('.traffic-content')).toBeHidden();
    await panel.getByRole('button',{name:'开启统计',exact:true}).click();
    await page.locator('#usage-confirm-dialog').getByRole('button',{name:'取消',exact:true}).click();
    await expect(panel.locator('.traffic-content')).toBeHidden();
    await panel.getByRole('button',{name:'开启统计',exact:true}).click();
    await page.locator('#usage-confirm-dialog').getByRole('button',{name:'开启统计',exact:true}).click();
    await expect(page.locator('#usage-confirm-dialog')).toHaveCount(0);
    await expect(panel.locator('.traffic-line')).toHaveCount(1);
    await expect(panel.locator('.traffic-gap')).toHaveCount(1);
    await panel.locator('.traffic-plot').scrollIntoViewIfNeeded();
    const marker=await panel.locator('.traffic-switch').boundingBox();
    const pointer={x:marker!.x,y:marker!.y+marker!.height*.9};
    await page.mouse.move(pointer.x,pointer.y);
    await expect(panel.locator('.traffic-tooltip')).toContainText('25000002 → 25000001');
    await expect(panel.locator('.traffic-tooltip')).toContainText('原账号流量耗尽');
    const tip=await panel.locator('.traffic-tooltip').boundingBox();
    expect(tip!.x>=pointer.x+28||tip!.x+tip!.width<=pointer.x-28||tip!.y>=pointer.y+28||tip!.y+tip!.height<=pointer.y-28).toBeTruthy();
    await expect(panel.locator('.traffic-tooltip')).toBeVisible();
    await page.screenshot({path:`artifacts.local/traffic-hover-${platform}.png`});
    const choose=async(name:string,value:string)=>{
      await panel.evaluate(async root=>{await Promise.all(root.getAnimations().map(animation=>animation.finished.catch(()=>{})));});
      await panel.getByRole('combobox',{name,exact:true}).click();
      await page.locator('main').dispatchEvent('scroll');
      const option=panel.locator(`.custom-select.open [data-value="${value}"]`);
      await option.evaluate(async node=>{const list=node.parentElement!;await Promise.all(list.getAnimations().map(animation=>animation.finished.catch(()=>{})));await new Promise(requestAnimationFrame);list.scrollTop=(node as HTMLElement).offsetTop;});
      await option.click({timeout:5000});
    };
    await choose('曲线','interfaces');await expect(panel.locator('.traffic-line')).toHaveCount(2);
    await panel.getByText('USB LAN (en7)',{exact:true}).first().click();
    await expect(panel.locator('.traffic-line')).toHaveCount(1);
    await choose('流量显示','cumulative');
    await panel.getByRole('button',{name:'全部在线设备',exact:true}).click();
    await expect(panel.locator('.traffic-content')).toContainText('可在设置中更改统计范围');
    expect(await page.evaluate(()=>localStorage.getItem('bjut_traffic_monitor_scope'))).toBe('local');
    await page.locator('[data-target=settings]').first().click();
    await page.locator('[data-settings-target="notifications-usage"]').click();
    await page.locator('input[name=setting-traffic-scope][value=all]').check();
    await page.locator('[data-target=dashboard]').first().click();
    await expect(panel.locator('.traffic-interfaces')).toContainText('其他设备');
    await page.waitForTimeout(5500);
    await expect(panel.getByRole('combobox',{name:'流量显示',exact:true})).toContainText('所选范围累计');
    await expect(panel.locator('.traffic-line')).toHaveCount(1);
    await panel.locator('.traffic-plot').focus();await page.keyboard.press('End');
    await expect(panel.locator('.traffic-tooltip')).toBeVisible();
    await page.keyboard.press('Escape');
    await choose('流量显示','monthly');
    await expect(panel.getByRole('combobox',{name:'流量显示',exact:true})).toContainText('当月累计');
    await choose('时间范围','custom');
    await expect(panel.getByLabel('开始时间',{exact:true})).toBeVisible();
    const beforeZoom=await panel.getByLabel('开始时间',{exact:true}).inputValue();
    await panel.locator('.traffic-plot').scrollIntoViewIfNeeded();
    const plot=await panel.locator('.traffic-plot').boundingBox();
    await page.mouse.move(plot!.x+plot!.width*.6,plot!.y+plot!.height*.5);await page.mouse.wheel(0,-500);
    await expect(panel.getByLabel('开始时间',{exact:true})).not.toHaveValue(beforeZoom);
    await choose('统计尺度','month');
    await expect(panel.locator('.traffic-line')).toHaveCount(1);
    await expect(panel.locator('.traffic-content')).toContainText('2.00 GB');
    await choose('统计尺度','minute');
    await panel.screenshot({path:`artifacts.local/traffic-${platform}.png`});
    if(platform==='macos'){
      const before=await panel.evaluate(node=>getComputedStyle(node).color);
      await page.locator('[data-target=settings]').first().click();
      await page.locator('[data-settings-target=appearance]').click();
      await page.locator('#setting-color-mode .custom-select-trigger').click();
      await page.locator('#setting-color-mode .custom-option[data-value=dark]').click();
      await page.locator('[data-target=dashboard]').first().click();
      expect(await panel.evaluate(node=>getComputedStyle(node).color)).not.toBe(before);
      const selected=await panel.getByRole('button',{name:'全部在线设备',exact:true}).evaluate(node=>getComputedStyle(node).backgroundColor);
      const unselected=await panel.getByRole('button',{name:'仅本机设备',exact:true}).evaluate(node=>getComputedStyle(node).backgroundColor);
      expect(selected).not.toBe(unselected);
      await panel.screenshot({path:'artifacts.local/traffic-macos-dark.png'});
    }
    expect(await page.evaluate(()=>document.documentElement.scrollWidth-window.innerWidth)).toBeLessThanOrEqual(1);
    await panel.getByRole('button',{name:'暂停统计',exact:true}).click();await page.locator('#usage-confirm-dialog').getByRole('button',{name:'暂停统计',exact:true}).click();await expect(panel.locator('.traffic-content')).toBeHidden();
  });
}

test('settings subpages, diagnostics full-width grid, and Android tablet sidebar without titlebar or hover',async({page})=>{
  await page.setViewportSize({width:1024,height:768});
  await page.route('**/*',route=>new URL(route.request().url()).hostname==='127.0.0.1'?route.continue():route.abort());
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button',{name:'隐藏验收面板',exact:true}).click();

  const layout=await page.evaluate(()=>{
    const titlebar=document.querySelector<HTMLElement>('.titlebar')!;
    const container=document.querySelector<HTMLElement>('.app-container')!;
    const nav=document.getElementById('nav')!;
    return {
      titlebarDisplay:getComputedStyle(titlebar).display,
      titlebarHeight:titlebar.getBoundingClientRect().height,
      containerTop:getComputedStyle(container).paddingTop,
      navRect:nav.getBoundingClientRect(),
      overflow:document.documentElement.scrollWidth-window.innerWidth,
    };
  });
  expect(layout.titlebarDisplay).toBe('none');
  expect(layout.titlebarHeight).toBe(0);
  expect(layout.containerTop).toBe('0px');
  expect(layout.navRect.width).toBeGreaterThan(180);
  expect(layout.navRect.height).toBeGreaterThan(600);
  expect(layout.overflow).toBeLessThanOrEqual(1);

  await page.locator('[data-target=diagnostics]').first().click();
  const diagGrid=await page.evaluate(()=>{
    const grid=document.querySelector<HTMLElement>('.diagnostics-grid')!;
    const health=document.getElementById('account-health-panel')!;
    return {
      gridWidth:grid.getBoundingClientRect().width,
      healthWidth:health.getBoundingClientRect().width,
      routerPanels:document.querySelectorAll('#router-diagnostics-panel').length,
      qualityPanels:document.querySelectorAll('#network-quality-panel').length,
    };
  });
  expect(diagGrid.routerPanels).toBe(0);
  expect(diagGrid.qualityPanels).toBe(0);
  expect(Math.abs(diagGrid.gridWidth-diagGrid.healthWidth)).toBeLessThan(2);

  await page.locator('[data-target=settings]').first().click();
  await expect(page.locator('#settings-home')).toBeVisible();
  await expect(page.locator('#btn-settings-back')).toBeHidden();

  const settingsItem=page.locator('[data-settings-target=appearance]');
  const hoverBefore=await settingsItem.evaluate(node=>{
    const s=getComputedStyle(node);return {bg:s.backgroundColor,transform:s.transform,filter:s.filter};
  });
  await settingsItem.hover();
  const hoverAfter=await settingsItem.evaluate(node=>{
    const s=getComputedStyle(node);return {bg:s.backgroundColor,transform:s.transform,filter:s.filter};
  });
  expect(hoverAfter).toEqual(hoverBefore);

  await settingsItem.click();
  await expect(page.locator('#btn-settings-back')).toBeVisible();
  await expect(page.locator('[data-settings-subpage=appearance]')).toBeVisible();
  const titleAlign=await page.evaluate(async()=>{
    const settings=document.getElementById('settings')!;
    await Promise.all(settings.getAnimations({subtree:true}).map(a=>a.finished.catch(()=>{})));
    const title=document.getElementById('settings-page-title')!.getBoundingClientRect();
    const card=document.querySelector<HTMLElement>('[data-settings-subpage=appearance] .glass-card')!.getBoundingClientRect();
    return Math.abs(title.left-card.left);
  });
  expect(titleAlign).toBeLessThan(2);
  await page.locator('#btn-settings-back').click();
  await expect(page.locator('#settings-home')).toBeVisible();

  await page.setViewportSize({width:390,height:844});
  const mobileRow=await page.evaluate(()=>{
    const row=document.querySelector<HTMLElement>('[data-settings-target=router]')!;
    const copy=row.querySelector<HTMLElement>('.settings-nav-copy')!.getBoundingClientRect();
    const chevron=row.querySelector<HTMLElement>('.settings-nav-chevron')!.getBoundingClientRect();
    return {copyRight:copy.right,chevronLeft:chevron.left};
  });
  expect(mobileRow.copyRight).toBeLessThanOrEqual(mobileRow.chevronLeft);
  await page.setViewportSize({width:1024,height:768});

  await page.locator('[data-target=settings]').first().click();
  await expect(page.locator('#settings-home')).toBeVisible();
  const backHandled=await page.evaluate(async()=>{
    document.querySelector<HTMLElement>('[data-settings-target=security]')!.click();
    const first=window.__handleAndroidBack?.();
    const homeVisible=!document.getElementById('settings-home')!.hidden;
    const second=window.__handleAndroidBack?.();
    return {first,homeVisible,second,dashboardActive:document.getElementById('dashboard')!.classList.contains('active')};
  });
  expect(backHandled).toEqual({first:true,homeVisible:true,second:true,dashboardActive:true});
});

test('light startup mask, full-viewport accounts modal, single billing dock, blur header, slide & switch drag',async({page})=>{
  await page.emulateMedia({colorScheme:'light'});
  await page.addInitScript(()=>{
    localStorage.setItem('bjut_theme','basic');
    localStorage.setItem('bjut_color_mode','light');
  });
  await page.setViewportSize({width:390,height:844});
  await page.route('**/*',route=>new URL(route.request().url()).hostname==='127.0.0.1'?route.continue():route.abort());
  await page.goto('http://127.0.0.1:1433/index.html',{waitUntil:'domcontentloaded'});
  const lightMask=await page.evaluate(()=>{
    const mask=document.getElementById('app-loading-mask');
    const maskBg=mask?getComputedStyle(mask).backgroundColor:'';
    const rootBg=getComputedStyle(document.documentElement).backgroundColor;
    return {scheme:document.documentElement.dataset.colorScheme,maskBg,rootBg};
  });
  expect(lightMask.scheme).toBe('light');
  expect(lightMask.maskBg).not.toBe('rgb(15, 23, 42)');
  expect(lightMask.rootBg).not.toBe('rgb(15, 23, 42)');

  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button',{name:'隐藏验收面板',exact:true}).click();

  expect(await page.locator('.page > .page-header p').count()).toBe(0);

  const selectionAndSlide=await page.evaluate(async()=>{
    document.querySelector<HTMLElement>('#nav .nav-item[data-target="accounts"]')!.click();
    const accountsSlideRight=document.getElementById('accounts')!.classList.contains('page-slide-from-right');
    const dashboardLeaveLeft=document.getElementById('dashboard')!.classList.contains('page-slide-leave-left');
    document.querySelector<HTMLElement>('#nav .nav-item[data-target="dashboard"]')!.click();
    const dashboardSlideLeft=document.getElementById('dashboard')!.classList.contains('page-slide-from-left');
    const accountsLeaveRight=document.getElementById('accounts')!.classList.contains('page-slide-leave-right');
    const addAccount = document.getElementById('btn-show-add')!;
    addAccount.focus();
    const leavingPageIgnoresFocus = document.activeElement !== addAccount;
    await new Promise(r=>setTimeout(r,420));
    const settledAnimName=getComputedStyle(document.getElementById('dashboard')!).animationName;
    document.querySelector<HTMLElement>('#nav .nav-item[data-target="accounts"]')!.click();
    return {
      accountsSlideRight,
      dashboardLeaveLeft,
      dashboardSlideLeft,
      accountsLeaveRight,
      leavingPageIgnoresFocus,
      settledAnimName,
      h1Select:getComputedStyle(document.querySelector('#accounts .page-header h1')!).userSelect,
      btnSelect:getComputedStyle(document.querySelector('#btn-show-add')!).userSelect,
      logsSelect:getComputedStyle(document.getElementById('logs-content')!).userSelect,
      inputSelect:getComputedStyle(document.getElementById('acc-username')!).userSelect,
    };
  });
  expect(selectionAndSlide).toEqual({
    accountsSlideRight:true,
    dashboardLeaveLeft:true,
    dashboardSlideLeft:true,
    accountsLeaveRight:true,
    leavingPageIgnoresFocus:true,
    settledAnimName:'none',
    h1Select:'none',
    btnSelect:'none',
    logsSelect:'text',
    inputSelect:'text',
  });

  await page.locator('#btn-show-add').click();
  await expect(page.locator('#add-modal')).toBeVisible();
  const modalCover=await page.evaluate(()=>{
    const header=document.querySelector('#accounts .page-header')!.getBoundingClientRect();
    const nav=document.getElementById('nav')!.getBoundingClientRect();
    const topHit=document.elementFromPoint(header.left+header.width/2,header.top+header.height/2);
    const navHit=document.elementFromPoint(nav.left+nav.width/2,nav.top+nav.height/2);
    return {
      topCovered:Boolean(topHit?.closest('#add-modal')),
      navCovered:Boolean(navHit?.closest('#add-modal')),
      modalParent:document.getElementById('add-modal')?.parentElement?.tagName,
    };
  });
  expect(modalCover).toEqual({topCovered:true,navCovered:true,modalParent:'BODY'});
  await page.locator('#btn-cancel-add').click();
  await expect(page.locator('#add-modal')).toBeHidden();

  await page.locator('#nav .nav-item[data-target="dashboard"]').click();
  await page.locator('#btn-open-billing').click();
  await expect(page.locator('#billing-center')).toBeVisible();
  const billingMobile=await page.evaluate(()=>{
    const nav=document.getElementById('nav')!;
    const billingDock=document.querySelector<HTMLElement>('.billing-mobile-nav')!;
    const backBtn=document.getElementById('btn-close-billing')!;
    const backRect=backBtn.getBoundingClientRect();
    const dockRect=billingDock.getBoundingClientRect();
    return {
      activeBodyClass:document.body.classList.contains('billing-center-active'),
      primaryNavVisibility:getComputedStyle(nav).visibility,
      dockVisible:dockRect.width>0 && dockRect.height>0 && dockRect.bottom<=innerHeight+1 && dockRect.top>innerHeight-120,
      unifiedBackPill:backBtn.classList.contains('settings-back-button') && backRect.height>=36 && getComputedStyle(backBtn).borderRadius==='999px',
    };
  });
  expect(billingMobile).toEqual({
    activeBodyClass:true,
    primaryNavVisibility:'hidden',
    dockVisible:true,
    unifiedBackPill:true,
  });

  await page.locator('#btn-close-billing').click();
  await expect(page.locator('#dashboard')).toBeVisible();

  await page.locator('#nav .nav-item[data-target="settings"]').click();
  await page.locator('[data-settings-target="appearance"]').click();
  await page.locator('#setting-theme .custom-select-trigger').click();
  const dropdownLayer=await page.evaluate(()=>{
    const options=document.querySelector<HTMLElement>('#setting-theme .custom-select-options')!;
    const mainEl=document.getElementById('main-content')!;
    const popoverOpen=options.matches(':popover-open');
    const style=getComputedStyle(options);
    return {
      popoverOpen,
      position:style.position,
      optionsScrollbarWidth:style.scrollbarWidth,
      mainScrollbarWidth:getComputedStyle(mainEl).scrollbarWidth,
      optionValues:Array.from(options.querySelectorAll<HTMLElement>('.custom-option')).map(el=>el.dataset.value),
    };
  });
  expect(dropdownLayer.popoverOpen).toBe(true);
  expect(dropdownLayer.position).toBe('fixed');
  expect(dropdownLayer.optionsScrollbarWidth).toBe('auto');
  expect(dropdownLayer.mainScrollbarWidth).toBe('auto');
  expect(dropdownLayer.optionValues).toEqual(['basic', 'winui']);

  await page.locator('[data-target="settings"]').first().click();
  await page.locator('[data-settings-target="automation"]').click();
  const switchBefore=await page.locator('#setting-auto-login').isChecked();
  const switchBox=await page.locator('label.switch:has(#setting-auto-login)').boundingBox();
  expect(switchBox).toBeTruthy();
  const sy=switchBox!.y+switchBox!.height/2;
  const startX=switchBefore?switchBox!.x+switchBox!.width-8:switchBox!.x+8;
  const endX=switchBefore?switchBox!.x+4:switchBox!.x+switchBox!.width-4;
  await page.mouse.move(startX,sy);
  await page.mouse.down();
  await page.mouse.move((startX+endX)/2,sy,{steps:4});
  await page.mouse.move(endX,sy,{steps:4});
  await page.mouse.up();
  expect(await page.locator('#setting-auto-login').isChecked()).toBe(!switchBefore);

  await page.locator('#nav .nav-item[data-target="diagnostics"]').click();
  const scrollState=await page.evaluate(async()=>{
    const main=document.getElementById('main-content')!;
    const header=document.querySelector<HTMLElement>('#diagnostics .page-header')!;
    main.scrollTop=120;
    main.dispatchEvent(new Event('scroll'));
    await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
    const scrolled=main.classList.contains('is-scrolled');
    const blurStyle=getComputedStyle(header,'::after').backdropFilter;
    const afterVis=getComputedStyle(header,'::after').visibility;
    main.scrollTop=0;
    main.dispatchEvent(new Event('scroll'));
    await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
    return {scrolled,blurStyle,afterVis,cleared:!main.classList.contains('is-scrolled')};
  });
  expect(scrollState.scrolled).toBe(true);
  expect(scrollState.blurStyle).toContain('blur(');
  expect(scrollState.afterVis).toBe('visible');
  expect(scrollState.cleared).toBe(true);
});

test('windows caption glyphs, 2-tier desktop layout, auto-only scroll anchor, onboarding, custom controls & drag-out stability', async ({page}) => {
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());

  // Item 1: Windows native titlebar SVG glyphs & 46px button geometry
  await page.setViewportSize({width: 1280, height: 820});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=windows');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/, {timeout: 15000});
  const titlebarGeom = await page.evaluate(() => {
    const ids = ['titlebar-minimize', 'titlebar-maximize', 'titlebar-close'];
    return ids.map(id => {
      const btn = document.getElementById(id)!;
      const svg = btn.querySelector<SVGElement>('svg.titlebar-glyph:not(.titlebar-glyph-restore)')!;
      const btnRect = btn.getBoundingClientRect();
      const svgRect = svg.getBoundingClientRect();
      return {
        id,
        width: Math.round(btnRect.width),
        svgW: Math.round(svgRect.width),
        svgH: Math.round(svgRect.height),
        hasLucide: Boolean(btn.querySelector('[data-lucide]')),
      };
    });
  });
  for (const g of titlebarGeom) {
    expect(g.width).toBe(46);
    expect(g.svgW).toBe(10);
    expect(g.svgH).toBe(10);
    expect(g.hasLucide).toBe(false);
  }
  for (const theme of ['basic', 'apple27', 'winui']) {
    await page.evaluate(t => {
      document.documentElement.dataset.theme = t;
      document.documentElement.dataset.colorScheme = 'light';
    }, theme);
    for (const btnId of ['titlebar-minimize', 'titlebar-maximize']) {
      await page.locator(`#${btnId}`).hover();
      await page.waitForTimeout(160);
      const hoverStyle = await page.locator(`#${btnId}`).evaluate(el => {
        const s = getComputedStyle(el);
        return {bg: s.backgroundColor, color: s.color};
      });
      expect(hoverStyle.color, `${theme} ${btnId}`).toBe('rgb(0, 0, 0)');
      expect(hoverStyle.bg, `${theme} ${btnId}`).toMatch(/^rgba?\(0,\s*0,\s*0,\s*0\.0\d+\)$/);
    }
  }
  await page.locator('#titlebar-minimize').hover();
  await page.waitForTimeout(160);
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'basic';
    document.documentElement.dataset.colorScheme = 'light';
  });
  await page.locator('#titlebar-minimize').hover();
  await page.waitForTimeout(160);
  await page.screenshot({path: 'artifacts.local/windows-titlebar.png'});

  // Item 2: Desktop 2-tier responsive layout across Basic / Apple 27 / WinUI
  for (const theme of ['basic', 'apple27', 'winui']) {
    await page.evaluate(t => {
      document.documentElement.dataset.theme = t;
    }, theme);
    await page.setViewportSize({width: 1280, height: 800});
    await expect.poll(async () => page.evaluate(() => Math.round(document.getElementById('nav')!.getBoundingClientRect().width))).toBe(215);
    const wide = await page.evaluate(() => {
      const nav = document.getElementById('nav')!.getBoundingClientRect();
      const label = document.querySelector<HTMLElement>('#nav .nav-item span')!;
      return {navWidth: Math.round(nav.width), labelVisible: getComputedStyle(label).display !== 'none'};
    });
    expect(wide).toEqual({navWidth: 215, labelVisible: true});
    if (theme === 'basic') await page.screenshot({path: 'artifacts.local/desktop-tier2-default.png'});

    await page.setViewportSize({width: 680, height: 600});
    await expect.poll(async () => page.evaluate(() => Math.round(document.getElementById('nav')!.getBoundingClientRect().width))).toBe(64);
    const narrow = await page.evaluate(() => {
      const nav = document.getElementById('nav')!.getBoundingClientRect();
      const mainStyle = getComputedStyle(document.getElementById('main-content')!);
      const label = document.querySelector<HTMLElement>('#nav .nav-item span')!;
      const activeIcon = document.querySelector<HTMLElement>('#nav .nav-item.active svg')!;
      return {
        navWidth: Math.round(nav.width),
        labelDisplay: getComputedStyle(label).display,
        activeIconVisible: activeIcon.getBoundingClientRect().width > 12,
        mainPaddingLeft: mainStyle.paddingLeft,
      };
    });
    expect(narrow.navWidth).toBe(64);
    expect(narrow.labelDisplay).toBe('none');
    expect(narrow.activeIconVisible).toBe(true);
    expect(narrow.mainPaddingLeft).toBe('24px');
    if (theme === 'winui') await page.screenshot({path: 'artifacts.local/desktop-tier1-rail.png'});
  }

  // Restore desktop viewport + Basic theme for remaining checks
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'basic';
    document.documentElement.dataset.colorScheme = 'light';
  });
  await page.setViewportSize({width: 1080, height: 780});

  // Item 8: Revert #dashboard-override-options (#setting-more-options enabled)
  await page.locator('[data-target=settings]').first().click();
  await page.locator('[data-settings-target=appearance]').click();
  await page.locator('label.switch:has(#setting-more-options)').click();
  // Item 9: Mouse drag rightward outside window must never scroll #main-content horizontally
  const switchBox = await page.locator('label.switch:has(#setting-more-options)').boundingBox();
  const cardLeftBefore = await page.locator('[data-settings-subpage=appearance] .glass-card').first().evaluate(el => el.getBoundingClientRect().left);
  await page.mouse.move(switchBox!.x + 8, switchBox!.y + switchBox!.height / 2);
  await page.mouse.down();
  await page.mouse.move(switchBox!.x + 200, switchBox!.y + switchBox!.height / 2, {steps: 5});
  await page.mouse.move(1220, switchBox!.y + switchBox!.height / 2, {steps: 5});
  await page.mouse.up();
  const dragStability = await page.evaluate(() => ({
    mainScrollLeft: document.getElementById('main-content')!.scrollLeft,
    docScrollLeft: document.documentElement.scrollLeft,
    cardLeftAfter: document.querySelector<HTMLElement>('[data-settings-subpage=appearance] .glass-card')!.getBoundingClientRect().left,
  }));
  expect(dragStability.mainScrollLeft).toBe(0);
  expect(dragStability.docScrollLeft).toBe(0);
  expect(Math.abs(dragStability.cardLeftAfter - cardLeftBefore)).toBeLessThan(1);

  // Item 5 & Item 6: Redesigned interactive page-jumping + spotlight + arrow onboarding guide
  await page.locator('#btn-settings-back').click();
  await page.locator('[data-settings-target=advanced]').click();
  await page.locator('#btn-onboarding').click();
  const inspectOnboardingStep = async (expectedPage: string, expectedTitle: string, expectedSubpage?: string) => {
    await expect(page.locator(`#${expectedPage}.active`)).toBeVisible();
    if (expectedSubpage) {
      await expect(page.locator(`[data-settings-subpage="${expectedSubpage}"]`)).toBeVisible();
    }
    await expect(page.locator('#onboarding-dialog h4')).toHaveText(expectedTitle);
    await expect(page.locator('#onboarding-dialog')).toHaveCSS('opacity', '1');
    await page.locator('#onboarding-dialog .onboarding-dialog').first().evaluate(el =>
      Promise.all(
        el.getAnimations({subtree: true})
          .filter(a => a.effect?.getComputedTiming().iterations !== Infinity)
          .map(a => a.finished.catch(() => undefined)),
      ),
    );
    await page.waitForTimeout(280);
    const state = await page.evaluate(() => {
      const dialog = document.querySelector<HTMLElement>('#onboarding-dialog .onboarding-dialog')!;
      const footer = dialog.querySelector<HTMLElement>('.onboarding-footer')!;
      const spotlight = document.querySelector<HTMLElement>('#onboarding-dialog .onboarding-spotlight')!;
      const arrow = dialog.querySelector<HTMLElement>('.onboarding-arrow')!;
      const dRect = dialog.getBoundingClientRect();
      const fRect = footer.getBoundingClientRect();
      const sRect = spotlight.getBoundingClientRect();
      const aRect = arrow.getBoundingClientRect();
      return {
        hasSteps: dialog.querySelectorAll('.onboarding-step-pill').length === 4,
        hasCard: Boolean(dialog.querySelector('.onboarding-step-card')),
        footerFits: fRect.bottom <= dRect.bottom + 1,
        inViewport: dRect.left >= 0 && dRect.top >= 0 && dRect.right <= window.innerWidth + 1 && dRect.bottom <= window.innerHeight + 1,
        spotlightVisible: !spotlight.hidden && sRect.width > 20 && sRect.height > 20,
        arrowVisible: getComputedStyle(arrow).display !== 'none' && aRect.width > 5 && aRect.height > 5,
        placement: dialog.dataset.placement,
      };
    });
    expect(state).toMatchObject({
      hasSteps: true,
      hasCard: true,
      footerFits: true,
      inViewport: true,
      spotlightVisible: true,
      arrowVisible: true,
    });
    expect(['top', 'bottom', 'left', 'right']).toContain(state.placement);
  };

  await inspectOnboardingStep('accounts', '添加校园网账号');
  await page.screenshot({path: 'artifacts.local/onboarding-step0.png'});
  await page.locator('#onboarding-dialog').getByRole('button', {name: '下一步', exact: true}).click();
  await inspectOnboardingStep('settings', '选择连接方式', 'automation');
  await page.screenshot({path: 'artifacts.local/onboarding-step1.png'});
  await page.locator('#onboarding-dialog').getByRole('button', {name: '下一步', exact: true}).click();
  await inspectOnboardingStep('settings', '保持后台运行', 'appearance');
  await page.screenshot({path: 'artifacts.local/onboarding-step2.png'});
  await page.locator('#onboarding-dialog').getByRole('button', {name: '下一步', exact: true}).click();
  await inspectOnboardingStep('dashboard', '连接路由器（可选）');
  await page.screenshot({path: 'artifacts.local/onboarding-step3.png'});

  // Escape restores originating settings:advanced subpage when replayed from settings
  await page.keyboard.press('Escape');
  await expect(page.locator('#onboarding-dialog')).toHaveCount(0);
  await expect(page.locator('#btn-export-config')).toBeVisible();
  await page.locator('#btn-export-config').click();
  await expect(page.locator('#config-transfer-modal')).toHaveCSS('opacity', '1');
  await page.locator('#config-transfer-modal .modal-content').evaluate(el => Promise.all(el.getAnimations().map(a => a.finished.catch(() => undefined))));
  const transferStyle = await page.evaluate(() => {
    const cb = document.querySelector<HTMLInputElement>('#config-transfer-modal input[type="checkbox"]')!;
    const radioChecked = document.querySelector<HTMLInputElement>('#config-transfer-modal input[type="radio"]:checked')!;
    const radioUnchecked = document.querySelector<HTMLInputElement>('#config-transfer-modal input[type="radio"]:not(:checked)')!;
    const cbStyle = getComputedStyle(cb);
    const rStyle = getComputedStyle(radioChecked);
    return {
      cbAppearance: cbStyle.appearance,
      cbW: Math.round(cb.offsetWidth),
      radioAppearance: rStyle.appearance,
      radioRadius: rStyle.borderRadius,
      radioW: Math.round(radioChecked.offsetWidth),
      radioCheckedDiffers: rStyle.backgroundColor !== getComputedStyle(radioUnchecked).backgroundColor,
    };
  });
  expect(transferStyle.cbAppearance).toBe('none');
  expect(transferStyle.cbW).toBe(18);
  expect(transferStyle.radioAppearance).toBe('none');
  expect(transferStyle.radioRadius).toBe('50%');
  expect(transferStyle.radioW).toBe(18);
  expect(transferStyle.radioCheckedDiffers).toBe(true);
  await page.screenshot({path: 'artifacts.local/custom-controls-modal.png'});
  await page.keyboard.press('Escape');
  await expect(page.locator('#config-transfer-modal')).toHaveCount(0);

  // Switch back to Dashboard: verify Item 8, Item 7 datetime-local, and Item 4
  await page.locator('[data-target=dashboard]').first().click();
  await expect(page.locator('#dashboard-override-options')).toBeVisible();
  const overrideGeom = await page.evaluate(() => {
    const grid = document.querySelector<HTMLElement>('.dashboard-override-grid')!;
    const field = document.querySelector<HTMLElement>('.dashboard-override-field')!;
    const select = document.getElementById('override-account')!;
    const netInfo = document.getElementById('dashboard-network-info')!;
    return {
      gridDisplay: getComputedStyle(grid).display,
      netInfoDisplay: getComputedStyle(netInfo).display,
      selectFillsField: Math.abs(select.getBoundingClientRect().width - field.getBoundingClientRect().width) <= 2,
    };
  });
  expect(overrideGeom).toEqual({gridDisplay: 'flex', netInfoDisplay: 'grid', selectFillsField: true});
  await page.screenshot({path: 'artifacts.local/dashboard-override-restored.png'});

  // Enable traffic chart to test Item 7 datetime-local & Item 4 bottom-of-dashboard scheduled check
  const panel = page.locator('#device-usage-panel');
  await panel.getByRole('button', {name: '开启统计', exact: true}).click();
  await page.locator('#usage-confirm-dialog').getByRole('button', {name: '开启统计', exact: true}).click();
  await expect(panel.locator('.traffic-line')).toHaveCount(1);
  await panel.getByRole('combobox', {name: '时间范围', exact: true}).click();
  await panel.locator('.custom-select.open [data-value="custom"]').click();
  const startDateStyle = await panel.getByLabel('开始时间', {exact: true}).evaluate(el => {
    const s = getComputedStyle(el);
    return {minH: parseFloat(s.minHeight), borderColor: s.borderTopColor, radius: parseFloat(s.borderTopLeftRadius)};
  });
  expect(startDateStyle.minH).toBeGreaterThanOrEqual(32);
  expect(startDateStyle.radius).toBeGreaterThanOrEqual(4);
  await page.screenshot({path: 'artifacts.local/custom-controls-traffic.png'});

  // Item 4A: Scrolled to bottom of Dashboard -> scheduled progress bar appears without twitching bottom panel
  const autoBottomShift = await page.evaluate(async () => {
    const main = document.getElementById('main-content')!;
    const usagePanel = document.getElementById('device-usage-panel')!;
    main.scrollTop = main.scrollHeight;
    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
    const topBefore = usagePanel.getBoundingClientRect().top;
    await (window as any).__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'network-check-progress',
      payload: {id: 50, generation: 1, revision: 0, message: '定时网络巡检中', percent: 45, elapsedMs: 120, complete: false},
    });
    let maxDrift = 0;
    const frames: unknown[] = [];
    const dualBefore = document.querySelector<HTMLElement>('.dual-stack-status')!.getBoundingClientRect().top;
    const prog = document.getElementById('network-check-progress')!;
    for (let i = 0; i < 18; i++) {
      await new Promise(requestAnimationFrame);
      const uDrift = usagePanel.getBoundingClientRect().top - topBefore;
      const dDrift = document.querySelector<HTMLElement>('.dual-stack-status')!.getBoundingClientRect().top - dualBefore;
      maxDrift = Math.max(maxDrift, Math.abs(uDrift));
      frames.push({i, uDrift: +uDrift.toFixed(3), dDrift: +dDrift.toFixed(3), scrollTop: +main.scrollTop.toFixed(3), progH: +prog.getBoundingClientRect().height.toFixed(3)});
    }
    return {maxDrift, frames, visible: !document.getElementById('network-check-progress')!.hidden};
  });
  expect(autoBottomShift.visible).toBe(true);
  expect(autoBottomShift.maxDrift, JSON.stringify(autoBottomShift.frames)).toBeLessThan(1.5);

  // Item 4B: Manual click (#btn-manual-update) -> pushes below content downward instead of squeezing upward
  await page.evaluate(async () => {
    await (window as any).__TAURI_INTERNALS__.invoke('plugin:event|emit', {
      event: 'network-check-progress',
      payload: {id: 50, generation: 1, revision: 9, message: '巡检完成', percent: 100, elapsedMs: 300, complete: true},
    });
  });
  await expect(page.locator('#network-check-progress')).toBeHidden({timeout: 8000});
  const manualDownwardShift = await page.evaluate(async () => {
    const main = document.getElementById('main-content')!;
    const dual = document.querySelector<HTMLElement>('.dual-stack-status')!;
    main.scrollTop = 180;
    await new Promise(requestAnimationFrame);
    const dualTopBefore = dual.getBoundingClientRect().top;
    const scrollBefore = main.scrollTop;
    document.getElementById('btn-manual-update')!.click();
    for (let i = 0; i < 20; i++) await new Promise(requestAnimationFrame);
    return {
      dualMovedDown: dual.getBoundingClientRect().top - dualTopBefore,
      scrollChanged: Math.abs(main.scrollTop - scrollBefore),
    };
  });
  expect(manualDownwardShift.dualMovedDown).toBeGreaterThan(20);
  expect(manualDownwardShift.scrollChanged).toBeLessThan(1);

  await page.locator('[data-target=diagnostics]').first().click();
  await expect(page.locator('#router-diagnostics-panel')).toHaveCount(0);
  await expect(page.locator('#network-quality-panel')).toHaveCount(0);
  await page.screenshot({path: 'artifacts.local/diagnostics-clean.png'});

  // WinUI Fluent Design fidelity: @fluentui/tokens vars, Fluent SVG icons, active input fill, symmetric switch hover, and clean basic restore
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'winui';
    document.documentElement.dataset.colorScheme = 'light';
    document.documentElement.dataset.accent = 'blue';
  });
  await expect.poll(async () => page.evaluate(() => {
    const rootStyle = document.documentElement.style;
    const navSvg = document.querySelector<SVGSVGElement>('#nav .nav-item svg[data-lucide]');
    return {
      accentFillRest: rootStyle.getPropertyValue('--win-accent-fill-rest').trim().toLowerCase(),
      statusSuccessFg: rootStyle.getPropertyValue('--win-status-success-fg').trim(),
      shadowDialog: rootStyle.getPropertyValue('--win-shadow-dialog').trim(),
      iconEngine: navSvg?.getAttribute('data-icon-engine') ?? '',
      iconViewBox: navSvg?.getAttribute('viewBox') ?? '',
      titlebarLucideCount: document.querySelectorAll('.titlebar-glyph [data-lucide]').length,
    };
  })).toEqual({
    accentFillRest: '#005fb8',
    statusSuccessFg: expect.stringMatching(/\S/),
    shadowDialog: expect.stringMatching(/\S/),
    iconEngine: 'fluent',
    iconViewBox: '0 0 20 20',
    titlebarLucideCount: 0,
  });

  await page.locator('[data-target=dashboard]').first().click();
  const focusedWinuiInput = page.locator('#device-usage-panel').getByLabel('开始时间', {exact: true});
  await focusedWinuiInput.focus();
  await expect.poll(async () => focusedWinuiInput.evaluate(el => getComputedStyle(el).backgroundColor)).toBe('rgb(255, 255, 255)');

  await page.locator('[data-target=settings]').first().click();
  await page.locator('[data-settings-target=appearance]').click();
  const winuiSwitch = page.locator('label.switch:has(#setting-more-options)');
  await winuiSwitch.hover();
  await expect.poll(async () =>
    winuiSwitch.locator('.slider').evaluate(el => {
      const thumb = getComputedStyle(el, '::before');
      return {width: thumb.width, height: thumb.height};
    }),
  ).toEqual({width: '14px', height: '14px'});

  await page.evaluate(() => {
    document.documentElement.dataset.colorScheme = 'dark';
  });
  await expect.poll(async () =>
    page.evaluate(() => document.documentElement.style.getPropertyValue('--win-accent-text-primary').trim().toLowerCase()),
  ).toBe('#60cdff');

  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'basic';
    document.documentElement.dataset.colorScheme = 'light';
  });
  await expect.poll(async () => page.evaluate(() => ({
    accentFillRest: document.documentElement.style.getPropertyValue('--win-accent-fill-rest').trim(),
    iconEngine: document.querySelector('#nav .nav-item svg[data-lucide]')?.getAttribute('data-icon-engine') ?? '',
  }))).toEqual({
    accentFillRest: '',
    iconEngine: 'lucide',
  });
});

test('android phone onboarding jumps pages with spotlight and arrow indicator', async ({page}) => {
  await page.setViewportSize({width: 390, height: 844});
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/, {timeout: 15000});

  await page.locator('[data-target=settings]').first().click();
  await page.locator('[data-settings-target=advanced]').click();
  await page.locator('#btn-onboarding').click();

  for (const [stepIndex, expectedPage, expectedTitle, expectedSubpage] of [
    [0, 'accounts', '添加校园网账号', undefined],
    [1, 'settings', '选择连接方式', 'automation'],
    [2, 'settings', '保持后台运行', 'security'],
    [3, 'dashboard', '连接路由器（可选）', undefined],
  ] as const) {
    if (stepIndex > 0) {
      await page.locator('#onboarding-dialog').getByRole('button', {name: '下一步', exact: true}).click();
    }
    await expect(page.locator(`#${expectedPage}.active`)).toBeVisible();
    if (expectedSubpage) {
      await expect(page.locator(`[data-settings-subpage="${expectedSubpage}"]`)).toBeVisible();
    }
    await expect(page.locator('#onboarding-dialog h4')).toHaveText(expectedTitle);
    await page.waitForTimeout(380);
    const mobileState = await page.evaluate(() => {
      const dialog = document.querySelector<HTMLElement>('#onboarding-dialog .onboarding-dialog')!;
      const footer = dialog.querySelector<HTMLElement>('.onboarding-footer')!;
      const spotlight = document.querySelector<HTMLElement>('#onboarding-dialog .onboarding-spotlight')!;
      const arrow = dialog.querySelector<HTMLElement>('.onboarding-arrow')!;
      const dRect = dialog.getBoundingClientRect();
      const fRect = footer.getBoundingClientRect();
      const sRect = spotlight.getBoundingClientRect();
      return {
        footerFits: fRect.bottom <= dRect.bottom + 1,
        inViewport: dRect.left >= 0 && dRect.top >= 0 && dRect.right <= window.innerWidth + 1 && dRect.bottom <= window.innerHeight + 1,
        spotlightVisible: !spotlight.hidden && sRect.width > 20 && sRect.height > 20,
        arrowVisible: getComputedStyle(arrow).display !== 'none',
      };
    });
    expect(mobileState).toEqual({footerFits: true, inViewport: true, spotlightVisible: true, arrowVisible: true});
    if (stepIndex === 0) {
      await page.screenshot({path: 'artifacts.local/onboarding-android-step0.png'});
    } else if (stepIndex === 2) {
      await page.screenshot({path: 'artifacts.local/onboarding-android-step2.png'});
    }
  }

  await page.locator('#onboarding-dialog').getByRole('button', {name: '完成', exact: true}).click();
  await expect(page.locator('#onboarding-dialog')).toHaveCount(0);
  await expect(page.locator('#dashboard.active')).toBeVisible();
});

test('liquid glass docks refract the live backdrop and follow drag gestures', async ({page}, testInfo) => {
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());

  await page.setViewportSize({width: 390, height: 844});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/, {timeout: 15000});

  for (const theme of ['basic', 'apple27']) {
    await page.evaluate(t => {
      document.documentElement.dataset.theme = t;
      document.documentElement.dataset.colorScheme = 'light';
    }, theme);
    await page.waitForTimeout(80);
    const dockMetrics = await page.evaluate(() => {
      const nav = document.getElementById('nav')!;
      const pill = nav.querySelector<HTMLElement>('.nav-liquid-pill')!;
      const droplet = nav.querySelector<HTMLElement>('.nav-liquid-droplet')!;
      const activeItem = nav.querySelector<HTMLElement>('.nav-item.active')!;
      const ns = getComputedStyle(nav);
      const surfaceStyle = getComputedStyle(nav.querySelector('.nav-liquid-bar-surface')!);
      const pillStyle = getComputedStyle(pill);
      const nr = nav.getBoundingClientRect();
      const pr = pill.getBoundingClientRect();
      const ar = activeItem.getBoundingClientRect();
      const surfaceBackdrop = surfaceStyle.backdropFilter
        || (surfaceStyle as unknown as {webkitBackdropFilter: string}).webkitBackdropFilter
        || 'none';
      const pillBackdrop = pillStyle.backdropFilter
        || (pillStyle as unknown as {webkitBackdropFilter: string}).webkitBackdropFilter
        || '';
      return {
        radius: ns.borderTopLeftRadius,
        surfaceBackdrop,
        pillBackdrop,
        hasBarSvgFilter: Boolean(document.getElementById('dock-main-glass')),
        pillOpacity: getComputedStyle(pill).opacity,
        dropletVisibility: getComputedStyle(droplet).visibility,
        alignedX: Math.abs(pr.left - ar.left),
        alignedY: Math.abs(pr.top - ar.top),
        widthDiff: Math.abs(pr.width - ar.width),
        height: Math.round(nr.height),
      };
    });
    expect(dockMetrics.radius, theme).toBe('999px');
    expect(dockMetrics.surfaceBackdrop, theme).toContain('#dock-main-glass');
    expect(dockMetrics.surfaceBackdrop, theme).not.toContain('blur(');
    expect(dockMetrics.pillBackdrop, theme).toContain('blur(');
    expect(dockMetrics.hasBarSvgFilter, theme).toBe(true);
    expect(dockMetrics.height, theme).toBe(62);
    expect(dockMetrics.pillOpacity, theme).toBe('1');
    expect(dockMetrics.dropletVisibility, theme).toBe('hidden');
    expect(dockMetrics.alignedX, theme).toBeLessThan(2);
    expect(dockMetrics.alignedY, theme).toBeLessThan(2);
    expect(dockMetrics.widthDiff, theme).toBeLessThan(2);
  }

  // Big dock rim refracts background achromatically without blur or color fringe; its interior stays clear while the active pill blurs.
  const optics = await page.evaluate(() => {
    const nav = document.getElementById('nav')!;
    const active = nav.querySelector<HTMLElement>('.nav-item.active')!.getBoundingClientRect();
    const rect = nav.getBoundingClientRect();
    const backdrop = document.createElement('div');
    backdrop.id = 'dock-backdrop-test';
    backdrop.style.cssText = `position:fixed;inset:${rect.top - 4}px 0 0;z-index:700;pointer-events:none;`
      + 'background:repeating-linear-gradient(#101010 0 8px,#f4f4f4 8px 16px)';
    document.body.append(backdrop);
    return {
      barClip: {x: Math.round(rect.left + rect.width * 0.35), y: Math.round(rect.top), width: 1, height: 16},
      pillClip: {x: Math.round(active.left + active.width * 0.25), y: Math.round(active.top + 4), width: 1, height: 16},
      barFilter: getComputedStyle(nav.querySelector('.nav-liquid-bar-surface')!).backdropFilter,
      pillFilter: getComputedStyle(nav.querySelector('.nav-liquid-pill')!).backdropFilter,
      hasBarSvgFilter: Boolean(document.getElementById('dock-main-glass')),
    };
  });
  expect(optics.hasBarSvgFilter).toBe(true);
  expect(optics.barFilter).toContain('#dock-main-glass');
  expect(optics.barFilter).not.toContain('blur(');
  expect(optics.pillFilter).toContain('blur(');
  const barSample = await page.screenshot({clip: optics.barClip, scale: 'css'});
  const pillBlurred = await page.screenshot({clip: optics.pillClip, scale: 'css'});
  const withoutGlassFilters = await page.addStyleTag({content: '#nav .nav-liquid-bar-surface,#nav .nav-liquid-pill{backdrop-filter:none !important;-webkit-backdrop-filter:none !important}'});
  const barPlain = await page.screenshot({clip: optics.barClip, scale: 'css'});
  const pillPlain = await page.screenshot({clip: optics.pillClip, scale: 'css'});
  const opticsStats = await page.evaluate(async ([barAfterPng, barBeforePng, pillAfterPng, pillBeforePng]) => {
    const pixels = async (png: string) => {
      const img = new Image();
      img.src = `data:image/png;base64,${png}`;
      await img.decode();
      const canvas = document.createElement('canvas');
      canvas.width = img.width;
      canvas.height = img.height;
      const context = canvas.getContext('2d')!;
      context.drawImage(img, 0, 0);
      return context.getImageData(0, 0, img.width, img.height).data;
    };
    const [barAfter, barBefore, pillAfter, pillBefore] = await Promise.all([
      pixels(barAfterPng),
      pixels(barBeforePng),
      pixels(pillAfterPng),
      pixels(pillBeforePng),
    ]);
    let maxBarRimShift = 0;
    let maxBarInteriorShift = 0;
    let maxBarEdgeShift = 0;
    for (let y = 0; y < 16; y++) {
      const s = 4 * y;
      const diff = Math.max(
        Math.abs(barAfter[s] - barBefore[s]),
        Math.abs(barAfter[s + 1] - barBefore[s + 1]),
        Math.abs(barAfter[s + 2] - barBefore[s + 2]),
      );
      if (y >= 1 && y <= 8) maxBarRimShift = Math.max(maxBarRimShift, diff);
      if (y >= 11) maxBarInteriorShift = Math.max(maxBarInteriorShift, diff);
      const span = (d: Uint8ClampedArray) => Math.max(d[s], d[s + 1], d[s + 2]) - Math.min(d[s], d[s + 1], d[s + 2]);
      maxBarEdgeShift = Math.max(maxBarEdgeShift, Math.abs(span(barAfter) - span(barBefore)));
    }
    let maxPillBlurDiff = 0;
    for (let y = 0; y < 16; y++) {
      const s = 4 * y;
      maxPillBlurDiff = Math.max(maxPillBlurDiff, Math.abs(pillAfter[s] - pillBefore[s]));
    }
    return {maxBarRimShift, maxBarInteriorShift, maxBarEdgeShift, maxPillBlurDiff};
  }, [barSample.toString('base64'), barPlain.toString('base64'), pillBlurred.toString('base64'), pillPlain.toString('base64')]);
  expect(opticsStats.maxBarRimShift).toBeGreaterThan(20);
  expect(opticsStats.maxBarInteriorShift).toBeLessThanOrEqual(4);
  expect(opticsStats.maxBarEdgeShift).toBeLessThanOrEqual(3);
  expect(opticsStats.maxPillBlurDiff).toBeGreaterThan(20);
  await withoutGlassFilters.evaluate(style => style.remove());
  await page.locator('#dock-backdrop-test').evaluate(element => element.remove());

  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'basic';
    document.documentElement.dataset.colorScheme = 'light';
    document.getElementById('main-content')!.scrollTop = 140;
  });
  await page.waitForTimeout(150);
  await page.screenshot({path: testInfo.outputPath('dock-light.png')});

  await page.evaluate(() => {
    document.documentElement.dataset.colorScheme = 'dark';
  });
  await page.waitForTimeout(150);
  await page.screenshot({path: testInfo.outputPath('dock-dark.png')});

  const firstTabDark = await page.locator('#nav .nav-item[data-target="dashboard"]').boundingBox();
  await page.mouse.move(firstTabDark!.x + firstTabDark!.width / 2, firstTabDark!.y + firstTabDark!.height / 2);
  await page.mouse.down();
  await expect.poll(() => page.locator('#nav').evaluate(node => ({
    dropletOpacity: Number(getComputedStyle(node.querySelector('.nav-liquid-droplet')!).opacity),
    glowOpacity: Number(getComputedStyle(node.querySelector('.nav-liquid-bar-glow')!).opacity),
  }))).toMatchObject({dropletOpacity: expect.any(Number), glowOpacity: expect.any(Number)});
  await page.waitForTimeout(420);
  await page.screenshot({path: testInfo.outputPath('dock-dark-press.png')});
  await page.mouse.up();
  await page.waitForTimeout(380);

  await page.evaluate(() => {
    document.documentElement.dataset.colorScheme = 'light';
    document.getElementById('main-content')!.scrollTop = 140;
  });

  // Interactive press-and-drag scrubbing across bottom dock tabs (dashboard -> diagnostics)
  const tabBoxes = await page.evaluate(() => {
    const start = document.querySelector<HTMLElement>('#nav .nav-item[data-target="dashboard"]')!.getBoundingClientRect();
    const accounts = document.querySelector<HTMLElement>('#nav .nav-item[data-target="accounts"]')!.getBoundingClientRect();
    const target = document.querySelector<HTMLElement>('#nav .nav-item[data-target="diagnostics"]')!.getBoundingClientRect();
    const logs = document.querySelector<HTMLElement>('#nav .nav-item[data-target="logs"]')!.getBoundingClientRect();
    const settings = document.querySelector<HTMLElement>('#nav .nav-item[data-target="settings"]')!.getBoundingClientRect();
    return {
      sx: start.left + start.width / 2,
      sy: start.top + start.height / 2,
      ax: accounts.left + accounts.width / 2,
      tx: target.left + target.width / 2,
      ty: target.top + target.height / 2,
      lx: logs.left + logs.width / 2,
      rx: settings.left + settings.width / 2,
      tabHeight: start.height,
      straddleX: (start.right + accounts.left) / 2,
    };
  });
  await page.mouse.move(tabBoxes.sx, tabBoxes.sy);
  await page.mouse.down();
  const dock = page.locator('#nav');
  const lifted = dock.locator('.nav-liquid-droplet');
  await expect.poll(() => dock.evaluate(async nav => {
    const lens = nav.querySelector('.nav-liquid-droplet')!;
    const glowEl = nav.querySelector('.nav-liquid-bar-glow')!;
    const a = lens.getBoundingClientRect();
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    const b = lens.getBoundingClientRect();
    const lensOpacity = Number(getComputedStyle(lens).opacity);
    const glowOpacity = Number(getComputedStyle(glowEl).opacity);
    return lensOpacity >= 0.99 && glowOpacity > 0.9 && Math.abs(b.width - a.width) < 0.15 && Math.abs(b.height - a.height) < 0.15;
  })).toBe(true);
  const pressProps = await dock.evaluate(nav => {
    const lens = nav.querySelector('.nav-liquid-droplet')!;
    const surface = nav.querySelector('.nav-liquid-bar-surface')!;
    const glowEl = nav.querySelector('.nav-liquid-bar-glow')!;
    const navRect = nav.getBoundingClientRect();
    const lensRect = lens.getBoundingClientRect();
    const bar = new DOMMatrixReadOnly(getComputedStyle(surface).transform);
    const diag = nav.querySelector('.nav-item[data-target="diagnostics"]')!;
    const item = new DOMMatrixReadOnly(getComputedStyle(diag).transform);
    const dash = nav.querySelector('.nav-item[data-target="dashboard"]')!;
    const accentDash = nav.querySelectorAll('.nav-liquid-accent-item')[0]!;
    const accentMat = new DOMMatrixReadOnly(getComputedStyle(accentDash).transform);
    const recess = nav.querySelector('.nav-liquid-bar-recess')!;
    const recessSurface = nav.querySelector<HTMLElement>('.nav-liquid-bar-recess-surface')!;
    const recessMat = new DOMMatrixReadOnly(getComputedStyle(recessSurface).transform);
    const barW = (surface as HTMLElement).offsetWidth;
    const barH = (surface as HTMLElement).offsetHeight;
    const recessRadiusParts = getComputedStyle(recessSurface).borderTopLeftRadius.split(/\s+/).map(parseFloat);
    const rPreX = recessRadiusParts[0] || 0;
    const rPreY = recessRadiusParts[1] ?? rPreX;
    return {
      glow: Number(getComputedStyle(glowEl).opacity),
      mag: accentMat.a / bar.a,
      centerDy: Math.abs((lensRect.top + lensRect.height / 2) - (navRect.top + navRect.height / 2)),
      lensBevel: parseFloat(getComputedStyle(nav).getPropertyValue('--dock-lens-bevel')),
      barScaleX: bar.a,
      barScaleY: bar.d,
      barTx: bar.m41,
      barTy: bar.m42,
      itemScaleX: item.a,
      itemScaleY: item.d,
      dashOpacity: Number(getComputedStyle(dash).opacity),
      dashClip: getComputedStyle(dash).clipPath,
      barCutout: getComputedStyle(surface).clipPath,
      surfaceBackdrop: getComputedStyle(surface).backdropFilter,
      pillVisibility: getComputedStyle(nav.querySelector('.nav-liquid-pill')!).visibility,
      recessBackdrop: getComputedStyle(recessSurface).backdropFilter,
      recessVisible: getComputedStyle(recess).visibility === 'visible' && getComputedStyle(recess).clipPath.startsWith('inset('),
      recessScaleX: recessMat.a,
      recessScaleY: recessMat.d,
      recessShrinkPxX: (1 - recessMat.a / bar.a) * barW,
      recessShrinkPxY: (1 - recessMat.d / bar.d) * barH,
      recessCapDiff: Math.abs(rPreX * recessMat.a - rPreY * recessMat.d),
    };
  });
  expect(pressProps.glow).toBeGreaterThan(0.9);
  expect(pressProps.mag).toBeGreaterThan(1.08);
  expect(pressProps.centerDy).toBeLessThan(0.75);
  expect(pressProps.lensBevel).toBeGreaterThan(6);
  expect(pressProps.lensBevel).toBeLessThanOrEqual(9);
  expect(pressProps.barScaleX).toBeGreaterThan(1.02);
  expect(pressProps.barScaleY).toBeGreaterThan(1.02);
  expect(Math.abs(pressProps.itemScaleX - pressProps.barScaleX)).toBeLessThan(0.005);
  expect(Math.abs(pressProps.itemScaleY - pressProps.barScaleY)).toBeLessThan(0.005);
  expect(pressProps.barTx).toBe(0);
  expect(pressProps.barTy).toBe(0);
  expect(pressProps.dashOpacity).toBe(0);
  expect(pressProps.dashClip).toBe('none');
  expect(pressProps.barCutout).toMatch(/^path\(/);
  expect(pressProps.surfaceBackdrop).toContain('#dock-main-glass');
  expect(pressProps.surfaceBackdrop).not.toContain('blur(');
  expect(pressProps.pillVisibility).toBe('hidden');
  expect(pressProps.recessBackdrop).toBe('none');
  expect(pressProps.recessVisible).toBe(true);
  expect(pressProps.recessScaleY).toBeLessThan(pressProps.barScaleY - 0.08);
  expect(pressProps.recessScaleX).toBeLessThan(pressProps.barScaleX - 0.025);
  expect(Math.abs(pressProps.recessShrinkPxX - pressProps.recessShrinkPxY)).toBeLessThan(0.6);
  expect(pressProps.recessCapDiff).toBeLessThan(0.8);

  const heldBox = (await lifted.boundingBox())!;
  const heldWidth = heldBox.width;
  const heldHeight = heldBox.height;

  // During fast horizontal sliding inside the dock, droplet flattens slightly shorter than big glass while big glass + icons keep press swell without translating.
  const sweepWatcher = dock.evaluate((nav, spec) => new Promise<{
    frames: number;
    peakWidth: number;
    minHeight: number;
    barHeight: number;
    maxAspect: number;
    allOk: boolean;
  }>(resolve => {
    const lens = nav.querySelector('.nav-liquid-droplet')!;
    const diag = nav.querySelector('.nav-item[data-target="diagnostics"]')!;
    let frames = 0;
    let peakWidth = 0;
    let minHeight = Infinity;
    let barHeight = nav.getBoundingClientRect().height;
    let maxAspect = 0;
    let allOk = true;
    const deadline = performance.now() + 200;
    const tick = () => {
      frames++;
      const rect = lens.getBoundingClientRect();
      const lensMatrix = new DOMMatrixReadOnly(getComputedStyle(lens).transform);
      const bar = new DOMMatrixReadOnly(getComputedStyle(nav.querySelector('.nav-liquid-bar-surface')!).transform);
      barHeight = nav.getBoundingClientRect().height;
      const item = new DOMMatrixReadOnly(getComputedStyle(diag).transform);
      peakWidth = Math.max(peakWidth, rect.width);
      minHeight = Math.min(minHeight, rect.height);
      maxAspect = Math.max(maxAspect, rect.width / rect.height);
      const ok = lensMatrix.b === 0
        && lensMatrix.c === 0
        && bar.a > 1.02 && bar.d > 1.02
        && Math.abs(item.a - bar.a) < 0.005
        && bar.m41 === 0 && bar.m42 === 0
        && Math.abs(item.m41 - bar.m41) < 0.5 && Math.abs(item.m42 - bar.m42) < 0.5
        && rect.height >= spec.tabHeight - 0.5;
      if (!ok) allOk = false;
      if (performance.now() < deadline) {
        requestAnimationFrame(tick);
      } else {
        resolve({frames, peakWidth, minHeight, barHeight, maxAspect, allOk});
      }
    };
    requestAnimationFrame(tick);
  }), {tabHeight: tabBoxes.tabHeight});
  await page.mouse.move(tabBoxes.rx, tabBoxes.ty, {steps: 6});
  const sweep = await sweepWatcher;
  expect(sweep.frames).toBeGreaterThan(3);
  expect(sweep.allOk).toBe(true);
  expect(sweep.maxAspect).toBeLessThan(2.4);
  expect(sweep.peakWidth - heldWidth).toBeGreaterThan(8);
  expect(sweep.minHeight).toBeLessThan(sweep.barHeight - 1);
  await page.mouse.move(tabBoxes.tx, tabBoxes.ty, {steps: 4});

  // When stationary on the target tab, the droplet settles back to press-in-place proportions.
  await expect.poll(() => dock.evaluate(async (nav, held) => {
    const lens = nav.querySelector('.nav-liquid-droplet')!;
    const a = lens.getBoundingClientRect();
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    const b = lens.getBoundingClientRect();
    const bar = new DOMMatrixReadOnly(getComputedStyle(nav.querySelector('.nav-liquid-bar-surface')!).transform);
    return Math.abs(a.width - held.heldWidth) < 0.8
      && Math.abs(b.width - held.heldWidth) < 0.8
      && Math.abs(a.height - held.heldHeight) < 0.8
      && Math.abs(b.height - held.heldHeight) < 0.8
      && Math.abs(b.width - a.width) < 0.15
      && bar.m41 === 0
      && bar.m42 === 0;
  }, {heldWidth, heldHeight})).toBe(true);
  const dragState = await page.evaluate(() => {
    const nav = document.getElementById('nav')!;
    const droplet = nav.querySelector<HTMLElement>('.nav-liquid-droplet')!;
    const pill = nav.querySelector<HTMLElement>('.nav-liquid-pill')!;
    const accentLayer = nav.querySelector<HTMLElement>('.nav-liquid-accent-layer')!;
    const dash = nav.querySelector<HTMLElement>('.nav-item[data-target="dashboard"]')!;
    const diag = nav.querySelector<HTMLElement>('.nav-item[data-target="diagnostics"]')!;
    return {
      isDragging: nav.classList.contains('is-liquid-dragging'),
      dropletActive: nav.classList.contains('is-droplet-active'),
      dropletVisibility: getComputedStyle(droplet).visibility,
      pillOpacity: getComputedStyle(pill).opacity,
      dropletProtrudes: droplet.getBoundingClientRect().height > nav.getBoundingClientRect().height + 2,
      overlaysLabels: Number(getComputedStyle(droplet).zIndex) > Number(getComputedStyle(accentLayer).zIndex)
        && Number(getComputedStyle(accentLayer).zIndex) > Number(getComputedStyle(diag).zIndex),
      diagHovered: diag.dataset.liquidHover === 'true',
      baseNeutral: getComputedStyle(dash).color === getComputedStyle(diag).color,
      accentClipped: getComputedStyle(accentLayer).visibility === 'visible'
        && getComputedStyle(accentLayer).clipPath.startsWith('inset('),
    };
  });
  expect(dragState).toEqual({
    isDragging: true,
    dropletActive: true,
    dropletVisibility: 'visible',
    pillOpacity: '0',
    dropletProtrudes: true,
    overlaysLabels: true,
    diagHovered: true,
    baseNeutral: true,
    accentClipped: true,
  });
  await page.screenshot({path: testInfo.outputPath('dock-drag.png')});

  // Big glass and icons shift together only after cursor exits the dock beyond the dead-zone.
  for (const probe of [
    {x: tabBoxes.tx, y: tabBoxes.ty - 100, expectY: true, expectX: false},
    {x: tabBoxes.tx, y: tabBoxes.ty + 60, expectY: true, expectX: false},
    {x: 389, y: tabBoxes.ty, expectY: false, expectX: true},
  ]) {
    await page.mouse.move(probe.x, probe.y, {steps: 4});
    await expect.poll(() => dock.evaluate((nav, spec) => {
      const lens = nav.querySelector('.nav-liquid-droplet')!;
      const diag = nav.querySelector('.nav-item[data-target="diagnostics"]')!;
      const lensMatrix = new DOMMatrixReadOnly(getComputedStyle(lens).transform);
      const bar = new DOMMatrixReadOnly(getComputedStyle(nav.querySelector('.nav-liquid-bar-surface')!).transform);
      const item = new DOMMatrixReadOnly(getComputedStyle(diag).transform);
      const yOk = !spec.expectY || (Math.abs(bar.m42) > 0.5 && Math.abs(bar.m42) <= 3.5);
      const xOk = !spec.expectX || (bar.m41 > 0.5 && bar.m41 <= 6);
      return lensMatrix.b === 0
        && lensMatrix.c === 0
        && yOk
        && xOk
        && Math.abs(item.m41 - bar.m41) < 0.5
        && Math.abs(item.m42 - bar.m42) < 0.5;
    }, probe)).toBe(true);
  }

  // Straddling two tabs clips accent color cleanly over both overlapping icons/labels.
  await page.mouse.move(tabBoxes.straddleX, tabBoxes.ty, {steps: 6});
  await expect.poll(() => dock.evaluate(nav => {
    const lens = nav.querySelector('.nav-liquid-droplet')!.getBoundingClientRect();
    const dash = nav.querySelector('.nav-item[data-target="dashboard"]')!.getBoundingClientRect();
    const acc = nav.querySelector('.nav-item[data-target="accounts"]')!.getBoundingClientRect();
    const accentLayer = nav.querySelector('.nav-liquid-accent-layer')!;
    const dashClip = getComputedStyle(nav.querySelector('.nav-item[data-target="dashboard"]')!).clipPath;
    const accClip = getComputedStyle(nav.querySelector('.nav-item[data-target="accounts"]')!).clipPath;
    return getComputedStyle(accentLayer).visibility === 'visible'
      && lens.left < dash.right - 8
      && lens.right > acc.left + 8
      && (dashClip.startsWith('path(') || accClip.startsWith('path('));
  })).toBe(true);
  await page.screenshot({path: testInfo.outputPath('dock-straddle.png')});

  await page.mouse.move(tabBoxes.tx, tabBoxes.ty, {steps: 4});
  // Wait for stationary settle before release, then verify every frame keeps accent color without flashing neutral and both big glass & icons/text jelly-rebound.
  await expect.poll(() => dock.evaluate((nav, held) => {
    const rect = nav.querySelector('.nav-liquid-droplet')!.getBoundingClientRect();
    return Math.abs(rect.width - held.heldWidth) < 1 && Math.abs(rect.height - held.heldHeight) < 1;
  }, {heldWidth, heldHeight})).toBe(true);
  const releaseWatcher = page.evaluate(() => new Promise<{frames: number; flashed: boolean; minBarScaleY: number; minItemScaleY: number; details: unknown[]}>(resolve => {
    const nav = document.getElementById('nav')!;
    const diag = nav.querySelector<HTMLElement>('.nav-item[data-target="diagnostics"]')!;
    const accentLayer = nav.querySelector<HTMLElement>('.nav-liquid-accent-layer')!;
    const surface = nav.querySelector<HTMLElement>('.nav-liquid-bar-surface')!;
    const pill = nav.querySelector<HTMLElement>('.nav-liquid-pill')!;
    const droplet = nav.querySelector<HTMLElement>('.nav-liquid-droplet')!;
    const probe = document.createElement('span');
    probe.style.color = 'var(--accent-color, var(--primary-color))';
    document.body.append(probe);
    const accentColor = getComputedStyle(probe).color;
    probe.remove();
    const details: unknown[] = [];
    let flashed = false;
    let minBarScaleY = 1;
    let minItemScaleY = 1;
    let sawSettledPill = false;
    let frames = 0;
    const deadline = performance.now() + 380;
    const tick = () => {
      frames++;
      const active = nav.classList.contains('is-droplet-active');
      const tr = getComputedStyle(surface).transform;
      if (tr && tr !== 'none') {
        minBarScaleY = Math.min(minBarScaleY, new DOMMatrixReadOnly(tr).d);
      }
      const itemTr = getComputedStyle(diag).transform;
      if (itemTr && itemTr !== 'none') {
        minItemScaleY = Math.min(minItemScaleY, new DOMMatrixReadOnly(itemTr).d);
      }
      const barFilter = getComputedStyle(surface).backdropFilter;
      if (!barFilter.includes('#dock-main-glass')) {
        flashed = true;
        details.push({frames, active, barFilter});
      }
      const pillStyle = getComputedStyle(pill);
      if (pillStyle.visibility === 'visible' && !pillStyle.backdropFilter.includes('blur(')) {
        flashed = true;
        details.push({frames, active, pillVis: pillStyle.visibility, pillFilter: pillStyle.backdropFilter});
      }
      if (!active && getComputedStyle(droplet).visibility === 'hidden' && pillStyle.visibility === 'visible') {
        sawSettledPill = true;
      }
      const layerVis = getComputedStyle(accentLayer).visibility;
      const layerOpacity = parseFloat(getComputedStyle(accentLayer).opacity);
      const clip = getComputedStyle(accentLayer).clipPath;
      const diagColor = getComputedStyle(diag).color;
      if (active) {
        if (layerVis !== 'visible' || layerOpacity < 0.99 || !clip.startsWith('inset(')) {
          flashed = true;
          details.push({frames, active, layerVis, layerOpacity, clip, diagColor});
        }
      } else if (diag.classList.contains('active') && diagColor !== accentColor) {
        flashed = true;
        details.push({frames, active, diagActive: true, diagColor, accentColor});
      }
      if (performance.now() < deadline || active) {
        requestAnimationFrame(tick);
      } else {
        if (!sawSettledPill) {
          flashed = true;
          details.push({frames, active, sawSettledPill});
        }
        resolve({frames, flashed, minBarScaleY, minItemScaleY, details});
      }
    };
    requestAnimationFrame(tick);
  }));
  await page.mouse.up();
  const releaseCheck = await releaseWatcher;
  expect(releaseCheck.flashed, JSON.stringify(releaseCheck.details)).toBe(false);
  expect(releaseCheck.frames).toBeGreaterThan(2);
  expect(releaseCheck.minBarScaleY).toBeLessThan(0.999);
  expect(releaseCheck.minItemScaleY).toBeLessThan(0.999);
  await expect(page.locator('#diagnostics.active')).toBeVisible();
  await expect(lifted).toBeHidden();
  await expect.poll(() => dock.evaluate(node => getComputedStyle(node.querySelector('.nav-liquid-bar-surface')!).transform)).toBe('none');
  const settledClean = await dock.evaluate(nav => {
    const diag = nav.querySelector<HTMLElement>('.nav-item[data-target="diagnostics"]')!;
    const surface = nav.querySelector<HTMLElement>('.nav-liquid-bar-surface')!;
    const glow = nav.querySelector<HTMLElement>('.nav-liquid-bar-glow')!;
    const recess = nav.querySelector<HTMLElement>('.nav-liquid-bar-recess')!;
    return {
      barCutout: getComputedStyle(surface).clipPath,
      glowOpacity: Number(getComputedStyle(glow).opacity),
      recessVisibility: getComputedStyle(recess).visibility,
      diagClip: getComputedStyle(diag).clipPath,
      diagOpacity: Number(getComputedStyle(diag).opacity),
      diagTransform: getComputedStyle(diag).transform,
    };
  });
  expect(settledClean).toEqual({
    barCutout: 'none',
    glowOpacity: 0,
    recessVisibility: 'hidden',
    diagClip: 'none',
    diagOpacity: 1,
    diagTransform: 'none',
  });

  // Releasing while the big glass is displaced outside the dock must keep the settled active pill perfectly aligned.
  await page.mouse.move(tabBoxes.tx, tabBoxes.ty);
  await page.mouse.down();
  await page.mouse.move(tabBoxes.ax - 20, tabBoxes.ty - 110, {steps: 6});
  await expect.poll(() => dock.evaluate(nav => Math.abs(new DOMMatrixReadOnly(getComputedStyle(nav.querySelector('.nav-liquid-bar-surface')!).transform).m42))).toBeGreaterThan(1);
  await page.mouse.up();
  await expect(page.locator('#accounts.active')).toBeVisible();
  await expect.poll(() => dock.evaluate(node => getComputedStyle(node.querySelector('.nav-liquid-bar-surface')!).transform)).toBe('none');
  const postDisplacedPill = await page.evaluate(() => {
    const nav = document.getElementById('nav')!;
    const pill = nav.querySelector<HTMLElement>('.nav-liquid-pill')!.getBoundingClientRect();
    const active = nav.querySelector<HTMLElement>('.nav-item.active')!.getBoundingClientRect();
    return {dx: Math.abs(pill.left - active.left), dy: Math.abs(pill.top - active.top)};
  });
  expect(postDisplacedPill.dx).toBeLessThan(1);
  expect(postDisplacedPill.dy).toBeLessThan(1);

  // Short-dwell diagonal quick swipe still switches to the destination tab.
  await page.locator('#nav .nav-item[data-target="dashboard"]').click();
  await expect(lifted).toBeHidden();
  await page.mouse.move(tabBoxes.sx, tabBoxes.sy);
  await page.mouse.down();
  await page.mouse.move(tabBoxes.sx + 4, tabBoxes.sy - 16, {steps: 1});
  await page.mouse.move(tabBoxes.ax, tabBoxes.sy - 45, {steps: 1});
  await page.mouse.up();
  await expect(page.locator('#accounts.active')).toBeVisible();
  await page.locator('#nav .nav-item[data-target="diagnostics"]').click();
  await expect(lifted).toBeHidden();
  const settledAfterDrag = await page.evaluate(() => {
    const nav = document.getElementById('nav')!;
    const pill = nav.querySelector<HTMLElement>('.nav-liquid-pill')!;
    const droplet = nav.querySelector<HTMLElement>('.nav-liquid-droplet')!;
    const diag = nav.querySelector<HTMLElement>('.nav-item[data-target="diagnostics"]')!;
    return {
      alignedX: Math.abs(pill.getBoundingClientRect().left - diag.getBoundingClientRect().left),
      dropletVisibility: getComputedStyle(droplet).visibility,
      pillOpacity: getComputedStyle(pill).opacity,
    };
  });
  expect(settledAfterDrag.alignedX).toBeLessThan(2);
  expect(settledAfterDrag.dropletVisibility).toBe('hidden');
  expect(settledAfterDrag.pillOpacity).toBe('1');

  // Rotating or resizing during a press should not commit a stale tab.
  await page.mouse.move(tabBoxes.sx, tabBoxes.sy);
  await page.mouse.down();
  await expect(page.locator('#nav .nav-liquid-droplet')).toBeVisible();
  await page.setViewportSize({width: 900, height: 650});
  await page.mouse.up();
  await expect(page.locator('#diagnostics.active')).toBeVisible();
  await expect(page.locator('#nav')).not.toHaveClass(/is-liquid-previewing|is-droplet-active/);
  await page.setViewportSize({width: 390, height: 844});

  await page.locator('#nav .nav-item[data-target="dashboard"]').click();
  await expect(lifted).toBeHidden();

  // Rubber-band overscroll past the rightmost tab extends the droplet and stretches the dock.
  const overscrollEnd = await page.evaluate(() => {
    const start = document.querySelector<HTMLElement>('#nav .nav-item[data-target="dashboard"]')!.getBoundingClientRect();
    const settings = document.querySelector<HTMLElement>('#nav .nav-item[data-target="settings"]')!.getBoundingClientRect();
    return {
      sx: start.left + start.width / 2,
      sy: start.top + start.height / 2,
      settingsRight: settings.right,
    };
  });
  await page.mouse.move(overscrollEnd.sx, overscrollEnd.sy);
  await page.mouse.down();
  await page.mouse.move(389, overscrollEnd.sy, {steps: 14});
  await expect.poll(() => dock.evaluate((nav, settingsRight) => {
    const lens = nav.querySelector('.nav-liquid-droplet')!.getBoundingClientRect();
    const bar = new DOMMatrixReadOnly(getComputedStyle(nav.querySelector('.nav-liquid-bar-surface')!).transform);
    return lens.right - settingsRight > 4 && lens.right <= 390 && bar.m41 > 0.5;
  }, overscrollEnd.settingsRight)).toBe(true);
  await page.screenshot({path: testInfo.outputPath('dock-overscroll.png')});

  // Edge holds with small pointer tremors must not twitch droplet dimensions at right or left edge.
  for (const edgeXs of [[389, 386, 389, 387, 389], [2, 5, 2, 4, 2]]) {
    await page.mouse.move(edgeXs[0], overscrollEnd.sy, {steps: 6});
    await expect.poll(() => dock.evaluate(async (nav, held) => {
      const lens = nav.querySelector('.nav-liquid-droplet')!;
      const a = lens.getBoundingClientRect();
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
      const b = lens.getBoundingClientRect();
      return Math.abs(a.width - held.heldWidth) < 0.6
        && Math.abs(b.width - held.heldWidth) < 0.6
        && Math.abs(a.height - held.heldHeight) < 0.6
        && Math.abs(b.height - held.heldHeight) < 0.6
        && Math.abs(b.width - a.width) < 0.15
        && Math.abs(b.height - a.height) < 0.15;
    }, {heldWidth, heldHeight})).toBe(true);
    const widths: number[] = [];
    const heights: number[] = [];
    for (const ex of edgeXs) {
      await page.mouse.move(ex, overscrollEnd.sy);
      const box = await dock.evaluate(async nav => {
        await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
        const r = nav.querySelector('.nav-liquid-droplet')!.getBoundingClientRect();
        return {width: r.width, height: r.height};
      });
      widths.push(box.width);
      heights.push(box.height);
    }
    expect(Math.max(...widths) - Math.min(...widths)).toBeLessThan(1.5);
    expect(Math.max(...heights) - Math.min(...heights)).toBeLessThan(1.5);
  }

  await page.mouse.move(389, overscrollEnd.sy, {steps: 6});
  await page.mouse.up();
  await expect(page.locator('#settings.active')).toBeVisible();
  await expect(lifted).toBeHidden();
  await expect.poll(() => dock.evaluate(node => getComputedStyle(node.querySelector('.nav-liquid-bar-surface')!).transform)).toBe('none');

  await page.locator('#nav .nav-item[data-target="dashboard"]').click();
  await expect(lifted).toBeHidden();
  await page.locator('#btn-open-billing').click();
  await expect(page.locator('#billing-center')).toBeVisible();
  await page.locator('.billing-mobile-nav [data-billing-section-target="records"]').click();
  await expect(page.locator('.billing-mobile-nav .nav-liquid-droplet')).toBeHidden();
  await expect.poll(() => page.evaluate(() => {
    const dock = document.querySelector<HTMLElement>('.billing-mobile-nav')!;
    const pill = dock.querySelector<HTMLElement>('.nav-liquid-pill')!;
    const active = dock.querySelector<HTMLElement>('.billing-section-nav-button.active')!;
    return Math.abs(pill.getBoundingClientRect().left - active.getBoundingClientRect().left);
  })).toBeLessThan(2);
  const billingMetrics = await page.evaluate(() => {
    const dock = document.querySelector<HTMLElement>('.billing-mobile-nav')!;
    const pill = dock.querySelector<HTMLElement>('.nav-liquid-pill')!;
    const active = dock.querySelector<HTMLElement>('.billing-section-nav-button.active')!;
    const pr = pill.getBoundingClientRect();
    const ar = active.getBoundingClientRect();
    const surfaceStyle = getComputedStyle(dock.querySelector('.nav-liquid-bar-surface')!);
    const pillStyle = getComputedStyle(pill);
    return {
      radius: getComputedStyle(dock).borderTopLeftRadius,
      surfaceBackdrop: surfaceStyle.backdropFilter || 'none',
      pillBackdrop: pillStyle.backdropFilter || '',
      hasBarSvgFilter: Boolean(document.getElementById('dock-billing-glass')),
      alignedX: Math.abs(pr.left - ar.left),
    };
  });
  expect(billingMetrics.radius).toBe('999px');
  expect(billingMetrics.surfaceBackdrop).toContain('#dock-billing-glass');
  expect(billingMetrics.surfaceBackdrop).not.toContain('blur(');
  expect(billingMetrics.pillBackdrop).toContain('blur(');
  expect(billingMetrics.hasBarSvgFilter).toBe(true);
  expect(billingMetrics.alignedX).toBeLessThan(2);
  await page.screenshot({path: testInfo.outputPath('dock-billing.png')});

  await page.emulateMedia({reducedMotion: 'reduce'});
  await page.locator('.billing-mobile-nav [data-billing-section-target="services"]').click();
  await expect(page.locator('.billing-mobile-nav [data-billing-section-target="services"]')).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('.billing-mobile-nav .nav-liquid-droplet')).toBeHidden();
  await page.emulateMedia({reducedMotion: 'no-preference'});
});

test('winui native navigation, settings cards, breadcrumb bar & basic flclash dropdown', async ({page}) => {
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
  await page.setViewportSize({width: 1280, height: 800});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=desktop');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/, {timeout: 15000});

  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'winui';
    document.documentElement.dataset.colorScheme = 'light';
  });
  await page.waitForTimeout(80);
  const winuiDesktopNav = await page.evaluate(() => {
    const nav = document.getElementById('nav')!;
    const surface = nav.querySelector('.nav-liquid-bar-surface');
    const pill = nav.querySelector('.nav-liquid-pill');
    return {
      hasLiquidClass: nav.classList.contains('has-liquid-pill'),
      surfaceDisplay: surface ? getComputedStyle(surface).display : 'missing',
      pillDisplay: pill ? getComputedStyle(pill).display : 'missing',
    };
  });
  expect(winuiDesktopNav).toEqual({
    hasLiquidClass: false,
    surfaceDisplay: 'none',
    pillDisplay: 'none',
  });

  await page.locator('[data-target="settings"]').first().click();
  await page.locator('[data-settings-target="appearance"]').click();
  const winuiSettingsMetrics = await page.evaluate(() => {
    const headerCopy = document.querySelector<HTMLElement>('#settings .settings-header-copy')!;
    const group = document.querySelector<HTMLElement>('[data-settings-subpage="appearance"] > .settings-group:has(> .setting-item)')!;
    const cardItem = group.querySelector<HTMLElement>(':scope > .setting-item')!;
    return {
      headerDisplay: getComputedStyle(headerCopy).display,
      groupBg: getComputedStyle(group).backgroundColor,
      itemBg: getComputedStyle(cardItem).backgroundColor,
    };
  });
  expect(winuiSettingsMetrics.headerDisplay).toBe('flex');
  expect(winuiSettingsMetrics.groupBg).toBe('rgba(0, 0, 0, 0)');
  expect(winuiSettingsMetrics.itemBg).not.toBe('rgba(0, 0, 0, 0)');

  await page.setViewportSize({width: 390, height: 844});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/, {timeout: 15000});

  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'winui';
    document.documentElement.dataset.colorScheme = 'light';
  });
  await page.waitForTimeout(80);
  const winuiMobileNav = await page.evaluate(() => {
    const nav = document.getElementById('nav')!;
    const ns = getComputedStyle(nav);
    return {
      hasLiquidClass: nav.classList.contains('has-liquid-pill'),
      radius: ns.borderTopLeftRadius,
      width: Math.round(nav.getBoundingClientRect().width),
      pillDisplay: getComputedStyle(nav.querySelector('.nav-liquid-pill')!).display,
    };
  });
  expect(winuiMobileNav.hasLiquidClass).toBe(false);
  expect(winuiMobileNav.radius).toBe('0px');
  expect(winuiMobileNav.width).toBeGreaterThanOrEqual(389);
  expect(winuiMobileNav.pillDisplay).toBe('none');

  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'basic';
    document.documentElement.dataset.colorScheme = 'light';
  });
  await page.locator('[data-target="settings"]').first().click();
  await page.locator('[data-settings-target="appearance"]').click();
  await page.locator('[aria-label="主题风格"], [aria-label="界面主题"], #setting-theme').locator('..').locator('.custom-select-trigger').first().click().catch(async () => {
    await page.locator('[data-settings-subpage="appearance"] .custom-select-trigger').first().click();
  });
  await expect(page.locator('.custom-select.open:not(.is-morphing)')).toHaveCount(1, {timeout: 2000});
  const dropdownCheck = await page.evaluate(() => {
    const openWrapper = document.querySelector<HTMLElement>('.custom-select.open')!;
    const trigger = openWrapper.querySelector<HTMLElement>('.custom-select-trigger')!.getBoundingClientRect();
    const menu = openWrapper.querySelector<HTMLElement>('.custom-select-options')!;
    const menuRect = menu.getBoundingClientRect();
    const opt = menu.querySelector<HTMLElement>('.custom-option.selected')!;
    const ms = getComputedStyle(menu);
    const os = getComputedStyle(opt);
    const after = getComputedStyle(opt, '::after');
    const prevMaxH = menu.style.maxHeight;
    menu.style.maxHeight = '56px';
    menu.scrollTop = 0;
    menu.dispatchEvent(new Event('scroll'));
    const canDown = menu.classList.contains('can-scroll-down');
    menu.scrollTop = menu.scrollHeight;
    menu.dispatchEvent(new Event('scroll'));
    const canUp = menu.classList.contains('can-scroll-up');
    const maskWhileScrollable = ms.maskImage || ms.webkitMaskImage || 'none';
    menu.style.maxHeight = prevMaxH;
    menu.scrollTop = 0;
    menu.dispatchEvent(new Event('scroll'));
    return {
      menuRadius: ms.borderTopLeftRadius,
      menuPaddingTop: ms.paddingTop,
      optRadius: os.borderTopLeftRadius,
      optBg: os.backgroundColor,
      optWeight: os.fontWeight,
      afterContent: after.content,
      canDown,
      canUp,
      maskWhileScrollable,
      coversVertically: menuRect.top <= trigger.bottom && menuRect.bottom >= trigger.top,
      expandsUpAndDown: menuRect.top < trigger.top - 4 && menuRect.bottom > trigger.bottom + 4,
    };
  });
  expect(dropdownCheck.menuRadius).toBe('20px');
  expect(dropdownCheck.menuPaddingTop).toBe('8px');
  expect(dropdownCheck.optRadius).toBe('12px');
  expect(dropdownCheck.optBg).toBe('rgba(0, 0, 0, 0)');
  expect(dropdownCheck.optWeight).toBe('400');
  expect(dropdownCheck.afterContent).not.toBe('none');
  expect(dropdownCheck.canDown).toBe(true);
  expect(dropdownCheck.canUp).toBe(true);
  expect(dropdownCheck.maskWhileScrollable).toBe('none');
  expect(dropdownCheck.coversVertically).toBe(true);
  expect(dropdownCheck.expandsUpAndDown).toBe(true);

  await page.keyboard.press('Escape');
  const closingMidFrame = await page.evaluate(() => {
    const closing = document.querySelector<HTMLElement>('.custom-select.closing');
    const menu = closing?.querySelector<HTMLElement>('.custom-select-options');
    return {
      hasClosingClass: !!closing,
      visibleMidClose: !!menu && !menu.hidden && getComputedStyle(menu).display !== 'none',
    };
  });
  expect(closingMidFrame.hasClosingClass).toBe(true);
  expect(closingMidFrame.visibleMidClose).toBe(true);
  await expect(page.locator('.custom-select.closing')).toHaveCount(0, {timeout: 2000});
  await expect(page.locator('#setting-theme .custom-select-options')).toBeHidden();
});

test('overscroll rubber-band, scroll position memory & non-adjacent android slide survive synthetic click', async ({page}) => {
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
  await page.setViewportSize({width: 390, height: 844});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/, {timeout: 15000});

  const overscroll = await page.evaluate(async () => {
    const main = document.getElementById('main-content')!;
    const dash = document.getElementById('dashboard')!;
    const nav = document.getElementById('nav')!;
    const navTopBefore = nav.getBoundingClientRect().top;
    const obsY = getComputedStyle(main).overscrollBehaviorY;
    main.scrollTop = 0;
    for (let i = 0; i < 3; i++) {
      main.dispatchEvent(new WheelEvent('wheel', {deltaY: -120, cancelable: true, bubbles: true}));
    }
    await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
    const bouncedTransform = dash.style.transform;
    const navTopDuring = nav.getBoundingClientRect().top;
    const end = performance.now() + 800;
    while (dash.style.transform && performance.now() < end) {
      await new Promise(requestAnimationFrame);
    }
    return {obsY, bouncedTransform, navTopDiff: Math.abs(navTopDuring - navTopBefore), settledTransform: dash.style.transform};
  });
  expect(overscroll.obsY).toBe('none');
  expect(overscroll.bouncedTransform).toMatch(/^translate3d\(0px,\s*[1-9]\d*(\.\d+)?px,\s*0px\)$/);
  expect(overscroll.navTopDiff).toBeLessThan(0.5);
  expect(overscroll.settledTransform).toBe('');

  // Scroll memory: Settings Home <-> Subpage and Dashboard <-> Accounts
  const scrollMem = await page.evaluate(async () => {
    const main = document.getElementById('main-content')!;
    const targetDashScroll = Math.min(180, Math.max(80, main.scrollHeight - main.clientHeight));
    main.scrollTop = targetDashScroll;
    document.querySelector<HTMLElement>('#nav .nav-item[data-target="settings"]')!.click();
    await new Promise(r => setTimeout(r, 340));
    const settingsScrolledTarget = Math.min(150, Math.max(60, main.scrollHeight - main.clientHeight));
    main.scrollTop = settingsScrolledTarget;
    document.querySelector<HTMLElement>('[data-settings-target="appearance"]')!.click();
    await new Promise(requestAnimationFrame);
    const subpageScroll = main.scrollTop;
    document.getElementById('btn-settings-back')!.click();
    await new Promise(requestAnimationFrame);
    const restoredSettingsScroll = main.scrollTop;
    document.querySelector<HTMLElement>('#nav .nav-item[data-target="dashboard"]')!.click();
    await new Promise(r => setTimeout(r, 340));
    const restoredDashScroll = main.scrollTop;
    return {
      settingsScrolledTarget,
      subpageScroll,
      restoredSettingsScroll,
      targetDashScroll,
      restoredDashScroll,
    };
  });
  expect(scrollMem.subpageScroll).toBe(0);
  expect(Math.abs(scrollMem.restoredSettingsScroll - scrollMem.settingsScrolledTarget)).toBeLessThan(2);
  expect(Math.abs(scrollMem.restoredDashScroll - scrollMem.targetDashScroll)).toBeLessThan(2);

  // Non-adjacent slide survives trailing synthetic click
  await page.locator('#nav .nav-item[data-target="settings"]').click();
  await page.waitForTimeout(340);
  const nonAdjacent = await page.evaluate(async () => {
    const accountsBtn = document.querySelector<HTMLElement>('#nav .nav-item[data-target="accounts"]')!;
    const accountsPage = document.getElementById('accounts')!;
    accountsBtn.click();
    await new Promise(requestAnimationFrame);
    accountsBtn.click(); // Trailing synthetic click with pageChanged === false
    await new Promise(requestAnimationFrame);
    const keptSlideClass = accountsPage.classList.contains('page-slide-from-left');
    await new Promise(r => setTimeout(r, 360));
    return {
      keptSlideClass,
      settledHasSlideClass: accountsPage.classList.contains('page-slide-from-left'),
      active: accountsPage.classList.contains('active'),
    };
  });
  expect(nonAdjacent).toEqual({
    keptSlideClass: true,
    settledHasSlideClass: false,
    active: true,
  });
});

test('the moving lens refracts icon and label strokes in its annulus', async ({page}, testInfo) => {
  await page.setViewportSize({width: 390, height: 844});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android&no-router');
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/);
  for (const selector of ['#nav', '.billing-mobile-nav']) {
    if (selector.startsWith('.')) await page.locator('#btn-open-billing').click();
    const dock = page.locator(selector);
    const tabs = dock.locator('.nav-item:visible, .billing-section-nav-button:visible');
    const [first, second] = await Promise.all([tabs.nth(0).boundingBox(), tabs.nth(1).boundingBox()]);
    const y = first!.y + first!.height / 2;
    // The glass crosses the second tab while the finger is between destinations.
    const x = second!.x + second!.width / 2 + second!.width * 0.66;
    await page.mouse.move(first!.x + first!.width / 2, y);
    await page.mouse.down();
    await page.mouse.move(x, y, {steps: 8});
    const lens = dock.locator('.nav-liquid-droplet');
    await expect(lens).toBeVisible();
    await expect.poll(() => lens.evaluate(async el => {
      const before = el.style.transform;
      await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      return el.style.transform === before;
    })).toBe(true);
    const ink = await tabs.nth(1).evaluate(el => [el.querySelector('svg')!, el.querySelector('span')!].map(node => {
      const {x,y,width,height} = node.getBoundingClientRect();
      return {x, y, width, height};
    }));
    const rect = (await lens.boundingBox())!;
    const clip = {
      x: Math.floor(Math.min(rect.x, ink[0].x, ink[1].x)),
      y: Math.floor(rect.y),
      width: Math.ceil(rect.width + second!.width),
      height: Math.ceil(rect.height),
    };
    const stem = selector === '#nav' ? 'main' : 'billing';
    await page.screenshot({path: testInfo.outputPath(`${stem}-ink.png`), clip, scale: 'css'});
    // A uniform background isolates light split by the actual navigation ink.
    const neutral = await page.addStyleTag({content: `${selector} > .nav-liquid-bar-surface {background: #eee !important;backdrop-filter:none !important}
      ${selector} :is(.nav-item,.billing-section-nav-button) {transition:none !important;color:#222 !important}`});
    const distorted = await page.screenshot({clip, scale: 'css'});
    const noLens = await page.addStyleTag({content: `${selector} .nav-liquid-droplet {backdrop-filter:none !important}`});
    const plain = await page.screenshot({clip, scale: 'css'});
    const stats = await page.evaluate(async ({samples, clip, ink, rect}) => {
      const [a,b] = await Promise.all(samples.map(async data => {
        const image = new Image();
        image.src = 'data:image/png;base64,' + data;
        await image.decode();
        const canvas = document.createElement('canvas');
        canvas.width = image.width;
        canvas.height = image.height;
        const ctx = canvas.getContext('2d')!;
        ctx.drawImage(image,0,0);
        return ctx.getImageData(0,0,image.width,image.height).data;
      }));
      const changedInk = [0,0];
      let centerChange = 0;
      for (let y=0; y<clip.height; y++) for (let x=0; x<clip.width; x++) {
        const index = 4*(y*clip.width+x);
        const changes = [0,1,2].map(c => a[index+c]-b[index+c]);
        const px = x+clip.x+0.5, py = y+clip.y+0.5;
        const radius = Math.hypot((px-rect.x)/rect.width*2-1, (py-rect.y)/rect.height*2-1);
        if (radius < 0.55) centerChange = Math.max(centerChange, ...changes.map(Math.abs));
        if (radius < 0.6 || radius > 0.97 || Math.max(...changes)-Math.min(...changes)<24) continue;
        ink.forEach((box, i) => {
          if (px>=box.x && px<box.x+box.width && py>=box.y && py<box.y+box.height) changedInk[i]++;
        });
      }
      return {changedInk,centerChange};
    }, {samples:[distorted.toString('base64'),plain.toString('base64')], clip, ink, rect});
    expect(stats.changedInk[0], stem+' icon').toBeGreaterThan(6);
    expect(stats.changedInk[1], stem+' label').toBeGreaterThan(6);
    expect(stats.centerChange, stem+' clear center').toBeLessThan(5);
    await neutral.evaluate(el => el.remove());
    await noLens.evaluate(el => el.remove());
    // Return to the first destination before releasing.
    await page.mouse.move(first!.x+first!.width/2, y);
    await page.mouse.up();
  }
});

test('fast dock releases switch pages even when the last move is coalesced', async ({page}) => {
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
  await page.setViewportSize({width: 390, height: 844});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android&no-router');
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/);

  const releaseAcross = (from: string, to: string, deliverMove: boolean) => page.evaluate(({from, to, deliverMove}) => {
    const source = document.querySelector<HTMLElement>(from)!;
    const destination = document.querySelector<HTMLElement>(to)!;
    const pointer = {bubbles: true, pointerId: 20, pointerType: 'touch', isPrimary: true};
    const position = (element: HTMLElement) => {
      const box = element.getBoundingClientRect();
      return {clientX: box.left + box.width / 2, clientY: box.top + box.height / 2};
    };
    source.dispatchEvent(new PointerEvent('pointerdown', {...pointer, ...position(source), buttons: 1}));
    if (deliverMove) {
      source.dispatchEvent(new PointerEvent('pointermove', {...pointer, ...position(destination), buttons: 1}));
    }
    source.dispatchEvent(new PointerEvent('pointerup', {...pointer, ...position(destination)}));
  }, {from, to, deliverMove});

  // No animation frame or move listener runs between press and lift.
  await releaseAcross('#nav [data-target="dashboard"]', '#nav [data-target="logs"]', false);
  expect(await page.locator('#logs.active').count()).toBe(1);
  await releaseAcross('#nav [data-target="logs"]', '#nav [data-target="dashboard"]', true);
  expect(await page.locator('#dashboard.active').count()).toBe(1);

  // Mouse pointermove with buttons === 0 during release must commit instead of bouncing back.
  await page.evaluate(() => {
    const source = document.querySelector<HTMLElement>('#nav [data-target="dashboard"]')!;
    const destination = document.querySelector<HTMLElement>('#nav [data-target="diagnostics"]')!;
    const pos = (el: HTMLElement) => {
      const box = el.getBoundingClientRect();
      return {clientX: box.left + box.width / 2, clientY: box.top + box.height / 2};
    };
    const common = {bubbles: true, pointerId: 21, pointerType: 'mouse', isPrimary: true, button: 0};
    source.dispatchEvent(new PointerEvent('pointerdown', {...common, ...pos(source), buttons: 1}));
    source.dispatchEvent(new PointerEvent('pointermove', {...common, ...pos(destination), buttons: 0}));
  });
  await expect(page.locator('#diagnostics.active')).toBeVisible();

  // Rapid zero-pause Playwright mouse swipe switches cleanly on release.
  const swipeBoxes = await page.evaluate(() => {
    const diag = document.querySelector<HTMLElement>('#nav [data-target="diagnostics"]')!.getBoundingClientRect();
    const dash = document.querySelector<HTMLElement>('#nav [data-target="dashboard"]')!.getBoundingClientRect();
    return {
      dx: diag.left + diag.width / 2,
      dy: diag.top + diag.height / 2,
      tx: dash.left + dash.width / 2,
      ty: dash.top + dash.height / 2,
    };
  });
  await page.mouse.move(swipeBoxes.dx, swipeBoxes.dy);
  await page.mouse.down();
  await page.mouse.move(swipeBoxes.tx, swipeBoxes.ty, {steps: 2});
  await page.mouse.up();
  await expect(page.locator('#dashboard.active')).toBeVisible();

  await page.locator('#btn-open-billing').click();
  await expect(page.locator('#billing-center.active')).toBeVisible();
  const billing = '.billing-mobile-nav';
  await releaseAcross(`${billing} [data-billing-section-target="overview"]`, `${billing} [data-billing-section-target="records"]`, false);
  expect(await page.locator(`${billing} [aria-selected="true"]`).getAttribute('data-billing-section-target')).toBe('records');
});

test('touch scrubbing stays inside a narrow screen and leaves taps available after cancellation', async ({browser}) => {
  const context = await browser.newContext({
    viewport: {width: 320, height: 740},
    deviceScaleFactor: 2,
    isMobile: true,
    hasTouch: true,
  });
  try {
    const page = await context.newPage();
    await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
    await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android&no-router');
    await page.getByRole('button', {name: '隐藏验收面板', exact: true}).tap();
    await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/);

    const tab = (target: string) => page.locator(`#nav .nav-item[data-target="${target}"]`);
    const center = async (target: string) => {
      const rect = (await tab(target).boundingBox())!;
      return {x: rect.x + rect.width / 2, y: rect.y + rect.height / 2};
    };
    const client = await context.newCDPSession(page);
    const dock = page.locator('#nav');
    const lens = dock.locator('.nav-liquid-droplet');
    const start = await center('dashboard');
    const end = await center('settings');
    await client.send('Input.dispatchTouchEvent', {type: 'touchStart', touchPoints: [start]});
    await client.send('Input.dispatchTouchEvent', {type: 'touchMove', touchPoints: [end]});
    await expect(dock).toHaveClass(/is-liquid-dragging/);
    await client.send('Input.dispatchTouchEvent', {type: 'touchCancel', touchPoints: []});
    await expect(page.locator('#dashboard.active')).toBeVisible();
    await expect(lens).toBeHidden();

    await client.send('Input.dispatchTouchEvent', {type: 'touchStart', touchPoints: [start]});
    await client.send('Input.dispatchTouchEvent', {type: 'touchMove', touchPoints: [{x: 319, y: end.y}]});
    await expect(tab('settings')).toHaveAttribute('data-liquid-hover', 'true');
    await expect(lens).toBeVisible();
    await expect.poll(async () => {
      const box = (await lens.boundingBox())!;
      return box.x + box.width;
    }).toBeGreaterThan(292);
    const box = (await lens.boundingBox())!;
    expect(box.x + box.width).toBeLessThanOrEqual(320);
    await client.send('Input.dispatchTouchEvent', {type: 'touchEnd', touchPoints: []});
    await expect(page.locator('#settings.active')).toBeVisible();
    await tab('accounts').tap();
    await expect(page.locator('#accounts.active')).toBeVisible();

    await client.send('Input.dispatchTouchEvent', {type: 'touchStart', touchPoints: [await center('accounts')]});
    await client.send('Input.dispatchTouchEvent', {type: 'touchMove', touchPoints: [await center('diagnostics')]});
    await client.send('Input.dispatchTouchEvent', {type: 'touchEnd', touchPoints: []});
    await expect(page.locator('#diagnostics.active')).toBeVisible();

    await client.send('Emulation.setEmulatedMedia', {features: [{name: 'prefers-reduced-transparency', value: 'reduce'}]});
    await expect.poll(() => dock.evaluate(node => ({
      bar: getComputedStyle(node.querySelector('.nav-liquid-bar-surface')!).backdropFilter,
      pill: getComputedStyle(node.querySelector('.nav-liquid-pill')!).backdropFilter,
    }))).toEqual({bar: 'none', pill: 'none'});
  } finally {
    await context.close();
  }
});

test('desktop and tablet sidebar indicators stay aligned through layout changes', async ({page}) => {
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
  // Windows desktop and its narrow rail keep the sliding sidebar pill.
  await page.setViewportSize({width: 1280, height: 800});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=windows');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/, {timeout: 15000});

  await page.locator('#nav .nav-item[data-target="accounts"]').click();
  await page.waitForTimeout(420);
  const desktopSidebarAligned = await page.evaluate(() => {
    const pill = document.querySelector<HTMLElement>('#nav .nav-liquid-pill')!.getBoundingClientRect();
    const accounts = document.querySelector<HTMLElement>('#nav .nav-item[data-target="accounts"]')!.getBoundingClientRect();
    return {
      topDiff: Math.abs(pill.top - accounts.top),
      heightDiff: Math.abs(pill.height - accounts.height),
      pillVisible: pill.width > 100 && pill.height > 24,
    };
  });
  expect(desktopSidebarAligned.pillVisible).toBe(true);
  expect(desktopSidebarAligned.topDiff).toBeLessThan(2);
  expect(desktopSidebarAligned.heightDiff).toBeLessThan(2);
  await page.setViewportSize({width: 680, height: 600});
  await expect.poll(async () => page.evaluate(() => {
    const pill = document.querySelector<HTMLElement>('#nav .nav-liquid-pill')!.getBoundingClientRect();
    const accounts = document.querySelector<HTMLElement>('#nav .nav-item[data-target="accounts"]')!.getBoundingClientRect();
    return Math.abs(pill.top - accounts.top) < 2 && Math.abs(pill.width - accounts.width) < 2;
  })).toBe(true);

  await page.setViewportSize({width: 900, height: 650});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=android');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/);
  await page.locator('#nav .nav-item[data-target="accounts"]').click();
  await expect.poll(async () => page.evaluate(() => {
    const pill = document.querySelector<HTMLElement>('#nav .nav-liquid-pill')!.getBoundingClientRect();
    const accounts = document.querySelector<HTMLElement>('#nav .nav-item[data-target="accounts"]')!.getBoundingClientRect();
    return Math.abs(pill.top - accounts.top) < 2 && Math.abs(pill.width - accounts.width) < 2;
  })).toBe(true);
});

test('macOS desktop sidebar stays opaque without behind-window glass underlay', async ({page}) => {
  await page.route('**/*', route => new URL(route.request().url()).hostname === '127.0.0.1' ? route.continue() : route.abort());
  await page.setViewportSize({width: 1280, height: 800});
  await page.goto('http://127.0.0.1:1433/scripts/app-regression.html?platform=macos&macosNativeGlass=1');
  await expect(page.locator('#qa-run')).toBeVisible();
  await page.getByRole('button', {name: '隐藏验收面板', exact: true}).click();
  await expect(page.locator('#app-loading-mask')).not.toHaveClass(/is-visible/, {timeout: 15000});

  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.macosNativeGlass)).toBe('ns-glass-effect');
  await page.waitForTimeout(150);
  const sidebarState = await page.evaluate(() => {
    const slots = ((window as any).__MACOS_GLASS_SLOTS__ || []) as Array<{id: string; visible: boolean}>;
    const underlay = slots.find(slot => slot.id === 'sidebar-underlay');
    const navBg = getComputedStyle(document.getElementById('nav')!).backgroundColor;
    const bodyBg = getComputedStyle(document.body).backgroundColor;
    return {
      hasVisibleUnderlay: Boolean(underlay && underlay.visible),
      navOpaque: navBg !== 'rgba(0, 0, 0, 0)' && navBg !== 'transparent',
      bodyOpaque: bodyBg !== 'rgba(0, 0, 0, 0)' && bodyBg !== 'transparent',
    };
  });
  expect(sidebarState).toEqual({
    hasVisibleUnderlay: false,
    navOpaque: true,
    bodyOpaque: true,
  });
});
