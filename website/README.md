# 项目网站

域名：`al.bjutdown.work`。静态页面与指南部署为独立 Cloudflare Worker `bjut-auto-login-site`，不改动 `red.bjutdown.work` 支付接力服务。

```sh
npm run site:build
npx wrangler@4.135.0 login
npx wrangler@4.135.0 deploy --config website/wrangler.jsonc
```

先确认 Cloudflare 登录账户拥有 `bjutdown.work`，再部署。`wrangler.jsonc` 使用 Custom Domain，由 Cloudflare 管理该主机名路由与证书；不应覆盖已有站点或忽略域名占用提示。

指南来自 `wiki/*.md`，编译时生成 `/guide/` 静态页面，自动生成标题 `id`、本页目录（TOC）、跨页面/本页锚点重写、全站章节搜索索引（`guide-index.json`）以及代码块一键复制，并在构建阶段对全部内部路径与跨页/本页锚点执行链接检查。更新文档后重新构建并部署。Wiki 独立发布：

```sh
node scripts/publish-wiki.mjs
node scripts/publish-wiki.mjs --publish
```

GitHub 必须先启用 Wiki 并创建首个页面。脚本只更新准备好的页面，不删除 Wiki 中其他内容，也不推送主仓库。

网站只向 GitHub 公共 API 查询发行版信息，失败时保留已验证的安装包下载直链与路由端编译矩阵回退。不包含分析跟踪、账号登录或校园网密码收集。

## 下载与界面展示

首页读取浏览器 Client Hints、User-Agent、系统标识和触控信息。macOS 架构不明确时，额外尝试显卡提供的明确芯片名称；不会把 `MacIntel` 当成 Intel 处理器的可靠证明。在 Android Via 浏览器、System WebView 或开启桌面模式/UA 缩减的环境中，若浏览器仅暴露 `Linux armv8l` 等 ABI 而不提供精确操作系统位数，官网推荐普通手机使用 `ARM64`（x86 环境推荐 `x86_64`），同时明确提示“浏览器未提供精确位数”，并提供 `ARM64` / `x86_64` 手动切换下拉框，绝不将浏览器无法提供的信息伪称为准确硬件识别。不读取账号，不保存或上传识别结果。iOS、32 位 Windows 及未发布的 Linux ARM 包不会误配到其他架构。

下载分为 **正式版（Latest Release）** 与 **预发布版（Pre-release）** 两个独立频道：稳定版跟随 GitHub `releases/latest` 标记，预发布版单独列出，排除签名文件和更新专用压缩包。API 失败、受限或禁用 JavaScript 时，仍保留构建时已验证的安装包直链与 Windows `Offline` 离线安装包。

官网同时提供 **OpenWrt 路由端 APK / IPK 与架构下载矩阵**（基于路由仓只读的 `build-targets.json` 与已验证的 `.build.json` / `builds.json` / `SHA256SUMS` 元数据），覆盖 OpenWrt `25.12.5`（APK）与 `24.10.8`（IPK）共 10 类平台、19 项 SDK 构建（其中 `qualcommax/ipq60xx` 仅提供 25.12.5 APK）。仅展示已验证的包链接、大小与真实 SHA-256 校验值，不编造未发布包或校验值。

```sh
npm run site:refresh-downloads
npm run site:test
npm run site:build
```

`release.json` 与 `openwrt-targets.json` 是可审查的公开清单，刷新失败不会覆盖它们。发布版本后可更新清单并重新部署。文件大小仅用于显示，下载本身直接交给 GitHub。

`preview/` 在构建时直接引用客户端 `index.html`、主题 CSS、图标和状态文案，以示例账号、余额和延迟渲染桌面（`1080×740`）与 Android（`390×844`）布局，并随系统明暗模式（`prefers-color-scheme: dark`）自动切换外观。不加载应用入口、Tauri 接口或真实账号存储。页面包含完整的暗色模式适配、键盘焦点与 ARIA 可访问性标注，以及 Open Graph / Twitter 分享卡片元数据。

Apple、Android、Linux 图标来自 [Simple Icons](https://github.com/simple-icons/simple-icons)，按该项目的 CC0 许可使用；各商标仍归对应权利人所有。Windows 图标使用四格窗形标志。
