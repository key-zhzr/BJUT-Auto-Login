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
    return {gridWidth:grid.getBoundingClientRect().width,healthWidth:health.getBoundingClientRect().width};
  });
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
