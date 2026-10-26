import {test,expect} from '@playwright/test';
import fs from 'node:fs';
const available=fs.existsSync('openwrt/tests/luci-regression.html')||fs.existsSync('openwrt-ready.local/openwrt/tests/luci-regression.html');
test('LuCI workbench refresh, billing and operations',async({page})=>{
  test.skip(!available,'Router worktree is verified in its own CI.');
  await page.route('**/*',route=>new URL(route.request().url()).hostname==='127.0.0.1'?route.continue():route.abort());
  await page.goto('http://127.0.0.1:1432/openwrt/tests/luci-regression.html');
  await page.locator('#run-router-qa').click();
  await expect(page.locator('#router-qa-results')).toContainText('全部通过',{timeout:120_000});
  await expect(page.locator('#router-qa-results')).not.toContainText(/^失败：/m);
});
