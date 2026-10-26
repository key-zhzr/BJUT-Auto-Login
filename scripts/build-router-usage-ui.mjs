// One set of units, forecasts and CSV escaping for App and LuCI.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import ts from 'typescript';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const router=path.resolve(process.argv[2] || path.join(root,'openwrt-ready.local'));
const target=path.join(router,'openwrt/luci-app-bjut-autologin/htdocs/luci-static/resources/bjut-autologin/usage-model.js');
const source=(fs.readFileSync(path.join(root,'src/device-usage-model.ts'),'utf8')+'\n'+fs.readFileSync(path.join(root,'src/usage-chart.ts'),'utf8').replace(/^import .*;$/gm,'')).replace(/^export /gm,'');
const {outputText}=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2020,module:ts.ModuleKind.None}});
fs.writeFileSync(target,"'use strict';\n'require baseclass';\n// Generated from src/device-usage-model.ts.\n"+outputText+'\nreturn baseclass.extend({flowLabel,flowMb,forecastUsage,usageCsv,buildUsageChart,normalizeUsageReport,usageChartCsv,UsageChart});\n');

fs.copyFileSync(path.join(root,'src/usage-chart.css'),path.join(path.dirname(target),'usage-chart.css'));
