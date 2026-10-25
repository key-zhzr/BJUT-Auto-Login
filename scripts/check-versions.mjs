#!/usr/bin/env node
// 版本一致性检查（任务 32）。
//
// 只读以下发布事实源，绝不把生成缓存当作发布真相（不读 website/release.json、
// artifacts.local、openwrt-ready.local、UPSTREAM.lock.json 等任何构建/同步产物）：
//   - package.json                       （npm，作为唯一基准版本）
//   - src-tauri/tauri.conf.json          （Tauri 版本 + Android versionCode）
//   - src-tauri/Cargo.toml               （Cargo 版本）
//   - 路由仓库 openwrt/*/Makefile        （两包 PKG_VERSION / PKG_RELEASE）
//
// Android versionCode 约定来自 git 历史（0.1.3→1004 … 0.1.6→1007）：
// 0.1.x 系列取 1000 + 补丁号 + 1；其他系列无既有约定，只报告不判定。
//
// 用法：
//   node scripts/check-versions.mjs [--expect x.y.z] [--router <目录>] [--require-router]
// 退出码：0 一致；1 存在不一致或事实源缺失。

import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import process from 'node:process';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, '..');

function parseArgs(argv) {
  const options = { expect: null, router: null, requireRouter: false };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === '--expect') options.expect = argv[++index] ?? null;
    else if (flag === '--router') options.router = argv[++index] ?? null;
    else if (flag === '--require-router') options.requireRouter = true;
    else {
      console.error(`未知参数：${flag}`);
      process.exit(2);
    }
  }
  return options;
}

function readText(path, label) {
  if (!existsSync(path)) {
    console.error(`缺少事实源：${label}（${path}）`);
    process.exit(1);
  }
  return readFileSync(path, 'utf8');
}

// 在 JSON 树中递归查找第一个指定键的值。
function findKey(value, key) {
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findKey(item, key);
      if (found !== undefined) return found;
    }
    return undefined;
  }
  if (value && typeof value === 'object') {
    if (Object.prototype.hasOwnProperty.call(value, key)) return value[key];
    for (const child of Object.values(value)) {
      const found = findKey(child, key);
      if (found !== undefined) return found;
    }
  }
  return undefined;
}

function parseMakefileField(text, field) {
  const match = text.match(new RegExp(`^${field}\\s*:?=\\s*(\\S+)`, 'm'));
  return match ? match[1] : null;
}

const options = parseArgs(process.argv.slice(2));
const failures = [];
const notes = [];

const npmVersion = JSON.parse(readText(join(repoRoot, 'package.json'), 'package.json')).version;
const expected = options.expect ?? npmVersion;
if (options.expect && options.expect !== npmVersion) {
  failures.push(`--expect ${options.expect} 与 package.json 中的 ${npmVersion} 不一致（package.json 是基准）`);
}

const tauriConfig = JSON.parse(readText(join(repoRoot, 'src-tauri/tauri.conf.json'), 'tauri.conf.json'));
const tauriVersion = tauriConfig.version;
const cargoText = readText(join(repoRoot, 'src-tauri/Cargo.toml'), 'Cargo.toml');
const cargoMatch = cargoText.match(/^version\s*=\s*"([^"]+)"/m);
const cargoVersion = cargoMatch ? cargoMatch[1] : null;

if (tauriVersion !== expected) failures.push(`tauri.conf.json version=${tauriVersion}，期望 ${expected}`);
if (!cargoVersion) failures.push('Cargo.toml [package] version 未找到');
else if (cargoVersion !== expected) failures.push(`Cargo.toml version=${cargoVersion}，期望 ${expected}`);

const versionCode = findKey(tauriConfig, 'versionCode');
let versionCodeOk = false;
if (typeof versionCode !== 'number' || !Number.isInteger(versionCode)) {
  failures.push(`tauri.conf.json versionCode 缺失或不是整数（当前：${JSON.stringify(versionCode)}）`);
} else {
  const [, minorText, patchText] = expected.split('.');
  const minor = Number(minorText);
  const patch = Number(patchText);
  if (minor === 1) {
    const expectedCode = 1000 + patch + 1;
    versionCodeOk = versionCode === expectedCode;
    if (!versionCodeOk) {
      failures.push(`Android versionCode=${versionCode}，按 0.1.x 约定应为 ${expectedCode}（1000+补丁号+1）`);
    }
  } else {
    versionCodeOk = true;
    notes.push(`版本 ${expected} 不属于 0.1.x 系列，versionCode=${versionCode} 仅报告（无既有约定）`);
  }
}

// —— 路由仓库两包（可选：未找到则跳过，--require-router 时视为失败） ——
const routerCandidates = [
  options.router,
  process.env.BJUT_ROUTER_REPO,
  join(repoRoot, '..', 'BJUT-Auto-Login-OpenWrt'),
  join(repoRoot, 'BJUT-Auto-Login-OpenWrt'),
].filter(Boolean);
const routerRoot = routerCandidates.find(candidate => {
  const candidateRoot = resolve(candidate);
  return existsSync(join(candidateRoot, 'openwrt/bjut-autologin/Makefile'))
    && existsSync(join(candidateRoot, 'openwrt/luci-app-bjut-autologin/Makefile'));
});

let routerRows = [];
if (!routerRoot) {
  const message = '未找到路由仓库（openwrt/bjut-autologin 与 openwrt/luci-app-bjut-autologin 的 Makefile）';
  if (options.requireRouter) failures.push(message);
  else notes.push(`${message}；已跳过（可用 --router <目录> 指定）`);
} else {
  for (const name of ['bjut-autologin', 'luci-app-bjut-autologin']) {
    const makefile = readText(join(routerRoot, `openwrt/${name}/Makefile`), `路由包 ${name} Makefile`);
    const pkgVersion = parseMakefileField(makefile, 'PKG_VERSION');
    const pkgRelease = parseMakefileField(makefile, 'PKG_RELEASE');
    routerRows.push({ name, pkgVersion, pkgRelease });
    const versionPart = pkgVersion ? pkgVersion.split('-')[0] : null;
    if (versionPart !== expected) {
      failures.push(`路由包 ${name} PKG_VERSION=${pkgVersion}，期望基于 ${expected}`);
    } else if (!pkgRelease || !/^\d+$/.test(pkgRelease)) {
      failures.push(`路由包 ${name} PKG_RELEASE 缺失或不是整数（当前：${pkgRelease}）`);
    }
  }
}

if (new Set(routerRows.map(row=>row.pkgRelease)).size>1) failures.push('路由本体与 LuCI 的 PKG_RELEASE 不一致');

// —— 汇总 ——
const check = (label, value) => `${value === expected ? '√' : '×'} ${label.padEnd(28)} ${value}`;
console.log(`基准版本（package.json）：${expected}`);
console.log(check('tauri.conf.json version', tauriVersion));
console.log(check('Cargo.toml version', cargoVersion));
console.log(`${versionCodeOk ? '√' : '×'} Android versionCode${' '.repeat(13)}${typeof versionCode === 'number' ? versionCode : '<缺失>'}`);
for (const row of routerRows) {
  console.log(check(`路由包 ${row.name} PKG_VERSION`, row.pkgVersion));
  console.log(`  · PKG_RELEASE=${row.pkgRelease}`);
}
for (const note of notes) console.log(`说明：${note}`);
if (failures.length > 0) {
  console.error('\n版本不一致：');
  for (const failure of failures) console.error(`  × ${failure}`);
  process.exit(1);
}
console.log('\n版本一致性检查通过（仅基于事实源，未读取任何生成缓存）。');
