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
      return {
        triggerLeft: Math.round(trigger.left), triggerRight: Math.round(trigger.right),
        menuLeft: Math.round(bounds.left), menuRight: Math.round(bounds.right),
        inside: bounds.left >= 0 && bounds.right <= innerWidth + 1 && bounds.top >= 0 && bounds.bottom <= innerHeight + 1,
        gap: Math.min(Math.abs(bounds.top - trigger.bottom), Math.abs(trigger.top - bounds.bottom)),
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
    };
  });
  expect(dropdownLayer.popoverOpen).toBe(true);
  expect(dropdownLayer.position).toBe('fixed');
  expect(dropdownLayer.optionsScrollbarWidth).toBe('auto');
  expect(dropdownLayer.mainScrollbarWidth).toBe('auto');

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
