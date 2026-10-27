import {test,expect} from '@playwright/test';
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
  await page.locator('#btn-onboarding').click();
  await expect(page.locator('#onboarding-dialog')).toBeVisible();
  await page.locator('#onboarding-dialog').getByRole('button',{name:'下一步',exact:true}).click();
  await expect(page.locator('#onboarding-dialog h4')).toHaveText('选择连接方式');
  await page.keyboard.press('Escape');
  await expect(page.locator('#onboarding-dialog')).toHaveCount(0);
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
