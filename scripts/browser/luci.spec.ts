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

test('LuCI shows animated segment progress while loading a whole month of minute samples',async({page})=>{
  test.skip(!available,'Router worktree is verified in its own CI.');
  await page.route('**/*',route=>new URL(route.request().url()).hostname==='127.0.0.1'?route.continue():route.abort());
  await page.goto('http://127.0.0.1:1432/openwrt/tests/luci-regression.html?traffic-month');
  const chart=page.locator('.traffic-chart');
  await expect(chart.locator('.traffic-line').first()).toBeAttached();
  await chart.getByLabel('时间范围',{exact:true}).selectOption('custom');
  await chart.locator('.traffic-dates').evaluate(group=>{
    const [start,end]=group.querySelectorAll<HTMLInputElement>('input');
    start.value='2025-01-01T00:00';
    end.value='2025-01-31T23:59';
    end.dispatchEvent(new Event('change',{bubbles:true}));
  });
  const progress=chart.getByRole('progressbar',{name:'正在加载流量历史'});
  await expect(progress).toBeVisible();
  await expect.poll(()=>progress.evaluate(node=>Number(node.getAttribute('aria-valuenow')))).toBeGreaterThan(0);
  expect(await progress.locator('.traffic-loading-fill').evaluate(node=>getComputedStyle(node).transitionProperty)).toBe('transform');
  await expect(progress).toBeHidden();
  await expect(chart.getByText('所选范围新增 87.19 GB',{exact:false})).toBeVisible();
});
