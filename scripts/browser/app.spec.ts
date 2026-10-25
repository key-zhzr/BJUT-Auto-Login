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
