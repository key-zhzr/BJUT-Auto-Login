# 项目网站

域名：`al.bjutdown.work`。静态页面与指南部署为独立 Cloudflare Worker `bjut-auto-login-site`，不改动 `red.bjutdown.work` 支付接力服务。

```sh
npm run site:build
npx wrangler@4.135.0 login
npx wrangler@4.135.0 deploy --config website/wrangler.jsonc
```

先确认 Cloudflare 登录账户拥有 `bjutdown.work`，再部署。`wrangler.jsonc` 使用 Custom Domain，由 Cloudflare 管理该主机名路由与证书；不应覆盖已有站点或忽略域名占用提示。

指南来自 `wiki/*.md`，编译时生成 `/guide/` 静态页面。更新文档后重新构建并部署。Wiki 独立发布：

```sh
node scripts/publish-wiki.mjs
node scripts/publish-wiki.mjs --publish
```

GitHub 必须先启用 Wiki 并创建首个页面。脚本只更新准备好的页面，不删除 Wiki 中其他内容，也不推送主仓库。

网站只向 GitHub 公共 API 查询发行版信息，失败时保留已验证的安装包下载直链。不包含分析跟踪、账号登录或校园网密码收集。

## 下载与界面展示

首页会读取浏览器 Client Hints、User-Agent、系统标识和触控信息。macOS 架构不明确时，额外尝试显卡提供的明确芯片名称；不会把 `MacIntel` 当成 Intel 处理器的可靠证明。不读取账号，不保存或上传识别结果。无法确认架构时标注兼容推荐，用户始终可按平台、架构、安装包格式自行下载。iOS、32 位 Windows 及未发布的 Linux ARM 包不会误配到其他架构。

下载来自经过验证的 GitHub Release `browser_download_url`，排除签名文件和更新专用压缩包。页面会刷新公开正式版本列表，并按版本号选择，避免 GitHub 的 Latest 标记落后于已有正式版本。API 失败或禁用 JavaScript 时，仍保留构建时的安装包直链。

```sh
npm run site:refresh-downloads
npm run site:test
npm run site:build
```

`release.json` 是可审查的公开下载清单，刷新失败不会覆盖它。发布版本后可更新此清单并重新部署。文件大小仅用于显示，下载本身直接交给 GitHub。

`preview/` 在构建时直接引用客户端 `index.html`、主题 CSS、图标和状态文案，以示例账号、余额和延迟渲染桌面与 Android 布局。不加载应用入口、Tauri 接口或真实账号存储。网站改成允许同源预览框架；内联样式属性仅用于复用客户端布局，脚本仍只允许本站来源。

Apple、Android、Linux 图标来自 [Simple Icons](https://github.com/simple-icons/simple-icons)，按该项目的 CC0 许可使用；各商标仍归对应权利人所有。Windows 图标使用四格窗形标志。
