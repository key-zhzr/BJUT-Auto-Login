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

网站只向 GitHub 公共 API 查询版本信息，失败时保留有效的 Releases 下载入口。不包含分析跟踪、账号登录或校园网密码收集。
