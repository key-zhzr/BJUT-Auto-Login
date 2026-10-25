# App 与 LuCI UI 开发、调试与 Agent 交接

更新于 2026-10-26。本轮两端保持 0.1.6，路由包源码修订 r19（尚未发布）。接手时先查看两仓工作区与适用的 `AGENTS.md`，保留其他人的改动。

## 1. 仓库与修改范围

| 项目 | 本机位置 | 远端 |
| --- | --- | --- |
| App | `/Users/keyzhzr/Documents/BJUT-AL` | [BJUT-Auto-Login](https://github.com/key-zhzr/BJUT-Auto-Login) |
| OpenWrt / LuCI | `/Users/keyzhzr/Documents/BJUT-AL/openwrt-ready.local` | [BJUT-Auto-Login-OpenWrt](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt) |

这是两个独立 Git 仓库，分别检查、提交。`openwrt-ready.local` 是本机目录名，不是 App 发布文件的一部分。路由仓库内的 `openwrt/` 才是路由运行时代码；不要因为该仓库保留上游历史，就在其中修改桌面 App 的副本。

App 当前使用原生 DOM、TypeScript、CSS 和 Tauri 2，不是 React/Vue 项目。优先在现有组件和主题体系内修改。独立的项目网站位于 `website/`，不等于 App UI。

## 2. 最快启动：无需真实账号或路由器

在 App 仓库执行，首次准备依赖时使用 `npm ci`：

```sh
cd /Users/keyzhzr/Documents/BJUT-AL
npm ci
npm run dev -- --host 127.0.0.1 --port 1430
```

若已有同一项目的开发服务器，直接复用；不要结束其他人的进程。1430 被占用且不是本项目时，换一个端口，并同步调整下表网址。

| 入口 | 用途 | 运行与结果定位 |
| --- | --- | --- |
| `http://127.0.0.1:1430/scripts/app-regression.html` | 默认 Windows 风格的平台分支，完整 App 模拟界面 | 点击“运行本轮界面回归”；`#qa-run`、`#qa-status`、`#qa-results` |
| `http://127.0.0.1:1430/scripts/app-regression.html?platform=macos` | macOS 平台分支及 `.is-macos` 布局 | 同上；应在 Safari/WebKit 再看一次 |
| `http://127.0.0.1:1430/scripts/app-regression.html?platform=android` | Android 分支及底部导航布局 | 同上；还要把视口实际调整到移动尺寸 |
| `http://127.0.0.1:1430/scripts/dropdown-regression.html` | 三主题下拉菜单压力、键盘和弹窗焦点检查 | `#run`、`#result` |
| `http://127.0.0.1:1430/scripts/network-adapter-regression.html` | 认证网卡长名称、长地址与窄屏溢出检查 | `#run`、`#result` |

完整 App 页的通过信号是 `#qa-status` 显示“全部通过”，详细过程在 `#qa-results`。隐藏验收面板后再截图。修改源码可能触发页面重载并清空结果；应在停止编辑后完成最后一轮检查，再保存结果。

直接打开 `/` 加载的是正常 App 入口；普通浏览器没有真实 Tauri 后端，不能把它当成已连接后端的调试环境。`?platform=android/macos` 只选择模拟平台分支，不会自动换浏览器内核、运行原生插件或模拟 Android 键盘。

### 模拟数据在哪里

[`scripts/app-regression.ts`](../scripts/app-regression.ts) 读取当前 `index.html`、五份样式和真实 `src/main.ts`，再用 `mockIPC`、`mockWindows` 和内存偏好替换原生接口。它不读取真实 App 账号，不连接真实路由器、校园认证或付款服务。

- `cfg`：账号列表、主题、自动化和检测设置。
- `routerSnapshot`：上游、端口、账号、协作能力；`192.0.2.1` 是模拟地址。
- `center()`：App 计费概览、记录、服务与安全数据。
- `mockIPC` 各分支：延迟、错误、断线重连、验证码、查询与操作返回值。
- 模拟路由连接使用测试密码即可；其中的证书确认与短信验证也是本地模拟流程，不要填写真实凭据。

要增加加载中、失败、空数据或长列表场景，修改这些 fixture 或对应 mock 分支。它们没有作为稳定的 `window.*` 调试 API 导出，不要依赖不存在的全局对象。

## 3. App 调试接口与文件定位

App 没有单独对外开放的 UI 调试 REST 服务。1430/1420 是前端资源服务，真实业务通过 Tauri IPC 调用 Rust。

| 修改内容 | 主要入口 |
| --- | --- |
| 页面结构、元素 ID、首帧文字 | [`index.html`](../index.html) |
| 初始化、导航、表单和业务事件绑定 | [`src/main.ts`](../src/main.ts) |
| 基础布局与控件 | [`src/styles.css`](../src/styles.css) |
| 主题变量与公共外观 | [`src/themes.css`](../src/themes.css)、[`src/appearance.ts`](../src/appearance.ts) |
| 样式加载前应用保存的主题 | [`public/theme-bootstrap.js`](../public/theme-bootstrap.js)；颜色与别名需同 `appearance.ts` 保持一致 |
| Apple OS 27 / WinUI 专属外观 | [`src/theme-apple.css`](../src/theme-apple.css)、[`src/theme-winui.css`](../src/theme-winui.css) |
| 固定顶栏、滚动区域、布局修正 | [`src/page-layout.css`](../src/page-layout.css)、[`src/page-headers.ts`](../src/page-headers.ts) |
| 应用内下拉菜单、键盘焦点 | [`src/custom-select.ts`](../src/custom-select.ts)、[`src/keyboard-navigation.ts`](../src/keyboard-navigation.ts) |
| 显隐、展开收起与内容高度动画 | [`src/animated-visibility.ts`](../src/animated-visibility.ts)、[`src/layout-motion.ts`](../src/layout-motion.ts) |
| Android 键盘避让 | [`src/android-keyboard.ts`](../src/android-keyboard.ts) |
| 认证网卡、双栈状态、诊断模块 | [`src/network-experience.ts`](../src/network-experience.ts)、[`src/diagnostics-view.ts`](../src/diagnostics-view.ts) |
| 路由发现、连接、融合展示 | [`src/router-control.ts`](../src/router-control.ts)、[`src/router-workbench.ts`](../src/router-workbench.ts) |
| 路由设置、账号编辑 | [`src/router-admin.ts`](../src/router-admin.ts) |
| 路由数据类型与流量/余额汇总 | [`src/router-model.ts`](../src/router-model.ts) |
| 路由计费数据适配与操作桥接 | [`src/router-billing-center.ts`](../src/router-billing-center.ts)、[`src/router-billing-bridge.ts`](../src/router-billing-bridge.ts) |
| LuCI 共享计费控件源 | [`src/router-billing-tools.ts`](../src/router-billing-tools.ts) |
| 文案来源 | [UI 文案修改位置](ui-text-guide.md) |

CSS 加载顺序是 `styles.css → themes.css → theme-apple.css → theme-winui.css → page-layout.css`。不要只修改早期规则而忽略后面的主题或布局覆盖。不要随意改动元素 ID：很多控制器直接通过 ID 绑定。

App 计费的“路由器 / 当前设备”复用同一工作台。路由器数据由 `RouterBillingCenter` 适配，再经 `router-billing-bridge` 提交给现有渲染函数；不要另建一套来源专属计费面板。切换来源、账号或断开连接时，应取消旧查询及验证码弹窗，保留 ticket 的账号核对。路由账单固定每页 100 条，分页保留服务端总数；完整导出上限为 50,000 条，分页缺失或数据变化时应失败，避免导出不完整文件。对应控制器回归包含在 `npm run test:frontend` 中。

### 现有 IPC 与事件

路由相关原生命令包括 `discover_routers`、`connect_router`、`get_router_status`、`router_login`、`router_request`、`disconnect_router`、`saved_router_connection`、`reconnect_router` 和账号同步。注册清单在 [`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs)，实现及参数校验在 [`src-tauri/src/router_control.rs`](../src-tauri/src/router_control.rs)。

业务控件优先复用已有控制器。例如路由工作台转发：

```ts
import { invoke } from '@tauri-apps/api/core';

const result = await invoke('router_request', {
  operation: 'billing_start',
  args: { uplink: 'dorm_a', account: 'current' },
});
```

这个调用在完整模拟页由 `mockIPC` 接收；在真实 App 中会访问已连接的路由器。不要把示例调用放进页面初始化，造成反复发起查询。操作名和字段必须符合后端白名单；计费 `owner` 由原生层加入，不让 UI 随意指定。

| 事件 | 对应展示 |
| --- | --- |
| `network-state-change` | 网络状态及登录入口 |
| `link-health`、`link-health-reset` | IPv4 / IPv6 状态 |
| `network-check-progress` | 控制台手动检测全过程 |
| `login-progress` | 登录阶段与耗时 |
| `network-diagnostic-step`、`network-diagnostic-progress` | 诊断逐项结果与进度 |
| `billing-center-progress` | 计费查询进度 |
| `log-event` | 运行日志 |

在 mock 的 `run_network_diagnostics` 分支中，可通过 `emit` 注入过程事件：

```ts
await emit('network-diagnostic-progress', {
  runId: payload.runId,
  percent: 50,
  label: '正在检查网络',
});
```

沿用请求的 `runId`，保留现有 `generation`、`revision`、ticket 和过期结果隔离逻辑；否则旧请求可能覆盖新页面。

## 4. LuCI 的离线预览与真实接口

路由仓库有独立入口，不需要 Node 或 OpenWrt 实机：

```sh
cd /Users/keyzhzr/Documents/BJUT-AL/openwrt-ready.local
python3 -m http.server 1431 --bind 127.0.0.1 --directory openwrt
```

打开 `http://127.0.0.1:1431/tests/luci-regression.html`，点击“检查刷新与功能”。按钮是 `#run-router-qa`，结果是 `#router-qa-results`；最终应包含“全部通过”且没有“失败”。

它直接加载实际 LuCI JS 模块，注入假的 `E`、`rpc`、`request`、`L`、`baseclass`、`form` 与 `uci`；目前覆盖**控制台、账号管理、计费系统、网络诊断、运行日志、设置六页**。账号和设置的表单由 mock 驱动，不包含 Argon 等真实主题壳，也不能替代真实 LuCI 的保存与权限检查。完整 App mock 中的路由设置页同样不能替代实机验收。

只要两仓库仍按本机目录摆放，也可用 App 的 Vite 打开 `http://127.0.0.1:1430/openwrt-ready.local/openwrt/tests/luci-regression.html`。独立 HTTP 入口更适合只负责 LuCI 的 Agent。

路由源码、模块导出规范、ubus 方法和实机调试流程见路由仓库的 `openwrt/UI-DEVELOPMENT.md`。它的所有页面代码位于：

```text
openwrt/luci-app-bjut-autologin/htdocs/luci-static/resources/
  view/bjut-autologin/{overview,accounts,billing,diagnostics,logs,config}.js
  bjut-autologin/{workbench,client,records,billing-tools}.js
```

## 5. 两端共享计费 UI：只能改源文件后生成

[`src/router-billing-tools.ts`](../src/router-billing-tools.ts) 现在是 LuCI 账户服务、设备与账号安全控件的生成源。App 的路由器计费来源不再渲染这些独立卡片，而是由 `router-billing-center.ts` 适配数据、`router-billing-bridge.ts` 转发查询与操作，并复用当前设备的计费工作台、面板和记录控件。不要把路由器来源重新拆成独立概览或标签页 DOM。

```sh
cd /Users/keyzhzr/Documents/BJUT-AL
node scripts/build-router-billing-ui.mjs /Users/keyzhzr/Documents/BJUT-AL/openwrt-ready.local

cd /Users/keyzhzr/Documents/BJUT-AL/openwrt-ready.local
node openwrt/tests/test_luci_module_contract.cjs
```

生成目标为路由包中的 `bjut-autologin/billing-tools.js`，应连同路由修改一并交付。不要直接修改生成文件，也不要改用路由仓库中未同步的旧生成器。当前生成器把 `animateResize` 接到 LuCI 的 `bjut.resize`，并返回 `baseclass.extend({ create(...) { ... } })`；LuCI 通过 `BillingTools.create(...)` 使用它。

任意 ES class 直接 `return`，或对导入的模块实例执行 `new BillingTools(...)`，会导致真实 LuCI 页面加载失败。这个问题不能只靠 JavaScript 语法检查发现，必须保留模块契约检查。

上游自动同步不是跨语言自动移植。共享计费源文件和生成器的更新会进入待审查分支；不要擅自扩展 Actions 权限或覆盖已有待审查分支。纯 UI 调整不需要顺带修改协议、发布标签或认证策略。

## 6. 必须保留的交互与视觉约束

- 三种主题都支持，分别检查浅色/深色。文案面向用户，协议显示 `bjut-sushe`、`bjut_wifi`、`lgn`，不显示内部 Type 编号。
- 下拉菜单复用 `CustomSelect`；Android 保持应用内菜单。通过真实 trigger 打开，不直接强加 `.open`，否则会绕过定位和关闭清理。打开前选定向上/向下位置，退出动画中也不能换方向。
- 只有实际使用键盘方向键时才显示菜单指示框。触摸/鼠标选择保持各主题反馈，菜单选项不能统一按按钮方式缩小。
- 涉及页面上下移动时复用 `setVisible`、`animateResize`、`setExpanded` / `toggleExpanded`，避免瞬间 `display:none`。快速反向操作、退出后再打开和减少动态效果都要可用；结束后恢复 overflow，避免持续裁切阴影。
- 保留 `#main-content` 主滚动区与固定顶栏。正文、菜单和弹窗层级分明；展开设置卡片不能盖住标题，标题也不能压住弹窗。
- 不要在 `html` 或 `.custom-select-options` 上重新加对称滚动条留白：前者曾露出 macOS 两条黑边，后者曾让选中项出现左右白边。正文仍要避免滚动条造成居中偏移，移动端使用窄滚动条，日志保留可拖动宽度。
- 刷新只更新变化数据，保留正在编辑的输入、焦点、选项和滚动位置。切换应用不能清空未提交表单，断开连接后不能接受旧请求结果。
- 当前网络（含 VPN）与网卡直连是不同探测路径；“未探测”“无法探测”“失败”不能改成同一语义。App 路由器来源按所选账号显示，切换账号复用计费账号选择器；LuCI 仍可按上游汇总并去重。
- App 内路由协作管理继续在 App 内完成；路由器来源与当前设备共用计费界面，但路由器计费不包含充值，充值页只显示切换说明。账号停用只影响自动登录，仍可手动使用。

## 7. 验证顺序与边界

App 最低检查：

```sh
npm run typecheck
npm run build
npm run test:frontend
```

这些命令**不会自动点击或运行浏览器验收页**。`tsconfig.json` 当前只包含 `src`；改动 `scripts/*-regression.ts` 后尤其要在浏览器实际执行。

至少检查桌面尺寸及 390 × 844 移动视口；有长标题、账号或网卡列表时补充 320px 窄屏。完整 App 回归已经调用 `scripts/motion-regression.ts`，它没有独立 HTML 页面。下拉压力页没有完整 App 结构和最后一层页面布局样式，不能替代顶栏与窗口边界验收。

改动 LuCI / 共享组件后，在路由仓库执行：

```sh
node openwrt/tests/test_luci_module_contract.cjs
python3 openwrt/tests/run_all.py
python3 openwrt/tests/test_sync_upstream.py
```

完整路由检查需要 Python 3、Node、Dash、OpenSSL、C 编译器及 ASan/UBSan，可能运行数分钟；不要反复启动同一组检查。UI-only 改动通常无需重跑全部 Rust 测试；若改变 IPC、原生窗口或后端行为，应按 CI 补上对应检查。

需要真实 App 时运行 `npm run tauri dev`，默认使用 1420，并启动真实后端；不要误以为它仍是 mock。macOS 若出现 Swift 链接库查找错误，先核对本机 Xcode/Command Line Tools；本机此前使用 `LIBRARY_PATH=/Library/Developer/CommandLineTools/usr/lib/swift/macosx npm run tauri dev` 解决环境查找，不要把本机绝对路径写进产品配置。

真实设备才可验证 Windows 单实例/窗口恢复、WebView2 表现、macOS 原生窗口边缘、Android 原生崩溃/键盘、VPN 与真实校园计费。Safari 可以补充 WebKit 的布局检查，但仍不是打包后的 Tauri App。交付时分别写清“模拟界面”“真实浏览器内核”“原生设备”哪些实际做过，保存最后状态截图及测试结果。

## 8. 可直接转给 UI Agent 的任务说明

```text
先阅读 docs/ui-development.md；如果修改 LuCI，再阅读路由仓库
openwrt/UI-DEVELOPMENT.md。按本次任务指定范围改 UI，保留其他人的工作。

先使用已有 mock 页面复现，不在真实路由器上运行模拟回归里的批量业务操作。
沿用现有主题、CustomSelect、动画控制器、IPC 和 ubus 数据契约。
共享计费控件仅在修改 LuCI 时先改 App 的 src/router-billing-tools.ts，再生成路由文件。
App 路由器计费的数据映射和操作分别改 src/router-billing-center.ts、src/router-billing-bridge.ts。
不要直接改生成物，不要把 LuCI 模块导出成任意 ES class。

完成后执行相关构建/回归，检查三主题、窄屏、加载/空数据/失败状态、
连续操作及离页清理；提供截图、改动文件和实际验证范围。
提交、推送、部署及真实业务操作按用户在当前任务中的授权执行。
```

### 路由协作 r14

- `src/router-model.ts` 的 `routerStatus` / `routerAdvice` 统一处理过期状态与诊断建议；暂时断线保留快照，显式停止协作才移除。
- `src/router-maintenance.ts` 管理备份和升级任务，使用 `maintenance_info` / `maintenance_start` / `maintenance_poll`；`lastJob` 可恢复任务进度。UI 不接收任意下载地址或文件路径。
- `RouterBillingCenter` 先发布概览，再逐步加入可选模块；`billing_cached` 仅预览上次结果。导出与账号切换仍核对请求代次和真实账号，旧回复不能覆盖新选择。
- 路由能力按 `capabilities` 显示，新增 `operations`、`account_limits`、`billing_background`、`maintenance` 和 `pushplus`。旧版路由继续可用，不强行显示不支持的设置。
- `scripts/test-router-experience.mjs` 验证过期状态和建议，`scripts/test-router-billing.mjs` 验证渐进发布、单位和明细隔离；界面入口仍是 `scripts/app-regression.html`。


## 新增模块与自动验收

- 托盘：`src-tauri/src/desktop_tray.rs`、`tray_settings.rs` 与 `src/tray-preferences.ts`。macOS 不把菜单直接绑定到图标，左键显示窗口、右键显式弹出菜单；Windows/Linux 保留原生右键菜单。
- 引导与新增弹窗：`src/onboarding.ts`、`src/presentation-dialog.ts`。后者处理动画、Esc、焦点恢复和 Tab 范围；动态高度复用 `layout-motion.ts`。
- 统计：`src-tauri/src/device_usage.rs`、`usage_history.rs`，前端 `device-usage-model.ts`、`device-usage-view.ts`。只累计 jfself 在线设备计数的连续增量；路由端 `traffic.sh` 与 `traffic-history.sh` 使用相同单位。共享生成器：`node scripts/build-router-usage-ui.mjs <路由仓目录>`。
- 后端编排：计费命令在 `billing_commands.rs`，校园服务命令在 `campus_commands.rs`。修改 Tauri 命令后检查 `lib.rs` 注册路径。
- 网络：`native_network.rs` 补充桌面系统事件；`network-quality*.ts` 保存已完成检查的质量历史；`diagnostic_tools.rs` 只接受预设链路目标。
- 快捷入口：`bjut-al://dashboard`、`bjut-al://check`、`bjut-al://accounts`、`bjut-al://billing`、`bjut-al://login`。不接受参数或账号密码；login 只打开确认窗口。Android Tasker 可使用 VIEW Intent。Android 另有快速设置磁贴和桌面组件。

```sh
npm run check:versions -- --router openwrt-ready.local --require-router
npm run test:frontend
npm run test:browser -- --project=app
npm run site:test
npm run site:build
```

浏览器验收用独立端口 1433，不接触真实账号、路由器或校园服务。本机若使用已安装 Edge：`BJUT_TEST_BROWSER=msedge npm run test:browser -- --project=app`；CI 安装 Chromium。不要同时运行两个 Playwright 进程写同一个结果目录。

路由端六页浏览器入口和独立配置位于 `openwrt/tests/browser/`；按该目录 package.json 执行。`python3 openwrt/tests/run_all.py` 为宿主离线验证。源码包的 r19 不等于已发布安装包，发布仍需按现有工作流显式执行。

本机 CLT 与 SDK 版本不同时，可用 SDKROOT 指向已安装的兼容 SDK；Swift 兼容静态库还可通过 LIBRARY_PATH 指定。不修改系统 xcode-select，也不提交本机生成路径。
