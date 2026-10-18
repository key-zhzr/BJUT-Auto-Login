// Keep the router and App billing controls on the same view implementation.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import ts from 'typescript';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const router=path.resolve(process.argv[2] || path.join(root,'openwrt-ready.local'));
const destination=path.join(router,'openwrt/luci-app-bjut-autologin/htdocs/luci-static/resources/bjut-autologin/billing-tools.js');
if(!fs.existsSync(path.dirname(destination)))throw new Error('请提供路由端仓库路径');
const source=fs.readFileSync(path.join(root,'src/router-billing-tools.ts'),'utf8')
 .replace("import { animateResize } from './layout-motion';",'const animateResize = bjut.resize;')
 .replace('export class RouterBillingTools','class RouterBillingTools')
 .replace('export interface BillingTarget','interface BillingTarget');
const {outputText}=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2020,module:ts.ModuleKind.None}});
// LuCI requires a baseclass subclass and imports its instance, not a bare ES class.
fs.writeFileSync(destination,"'use strict';\n'require baseclass';\n'require bjut-autologin.workbench as bjut';\n// Generated from the App's router-billing-tools.ts. Do not edit the shared view here.\n"+outputText+'\nreturn baseclass.extend({\n create: function(root, options) { return new RouterBillingTools(root, options); }\n});\n');
console.log('路由端计费控件已同步');
