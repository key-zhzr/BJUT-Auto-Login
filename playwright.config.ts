import {defineConfig} from '@playwright/test';
import fs from 'node:fs';
const router=fs.existsSync('openwrt/tests/luci-regression.html')?'.':'openwrt-ready.local';
const hasRouter=fs.existsSync(`${router}/openwrt/tests/luci-regression.html`);
export default defineConfig({
  testDir:'scripts/browser',timeout:180_000,expect:{timeout:15_000},fullyParallel:false,workers:1,
  retries:process.env.CI?1:0,reporter:[['list'],['html',{open:'never'}]],
  use:{headless:true,channel:process.env.BJUT_TEST_BROWSER||undefined,trace:'retain-on-failure',screenshot:'only-on-failure'},
  webServer:[{command:'npm run dev -- --host 127.0.0.1 --port 1433 --strictPort --force',url:'http://127.0.0.1:1433/scripts/app-regression.html',reuseExistingServer:!process.env.CI},
    ...(hasRouter?[{command:`python3 -m http.server 1432 --bind 127.0.0.1 --directory ${router}`,url:'http://127.0.0.1:1432/openwrt/tests/luci-regression.html',reuseExistingServer:!process.env.CI}]:[])],
  projects:[{name:'app',testMatch:'app.spec.ts'},{name:'luci',testMatch:'luci.spec.ts'}],
});
