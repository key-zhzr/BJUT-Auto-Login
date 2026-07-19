# 统一认证、修改密码与充值入口离线采集

这组脚本用于在无法同时连接校园服务和 Codex 的环境中，采集以下只读协议资料：

- `cas.bjut.edu.cn`：统一认证登录页、一次登录请求及受控重定向；
- `uc.bjut.edu.cn`：修改密码前端、密码规则、用户状态与公共配置；
- `itsapp.bjut.edu.cn`：移动门户进入 `ydapp` 时使用的可信 OAuth 中转；
- `ydapp.bjut.edu.cn`：`openV8HomePage` 跳转、`openid` 路由形态，以及网费充值
  页面依赖的前端资源。

脚本使用项目指定的微信浏览器 UA。每次运行只发送一次携带账号密码的 CAS
登录 POST；此后的请求均为只读 GET。它不会提交修改密码、充值、付款、用户资料
修改或任何其他状态变更请求。

## 使用

在仓库根目录运行：

```bash
./scripts/capture-cas-services.sh
```

按提示输入统一认证账号和密码。密码输入不会显示，不会进入命令行参数、Cookie
文件、原始响应或分享包。脚本不会自动重试登录，避免错误次数累积。

如果脚本还没有执行权限，可先运行一次：

```bash
chmod +x scripts/capture-cas-services.sh
```

可以更改输出目录：

```bash
./scripts/capture-cas-services.sh --output /path/to/private-output
```

只有在输出明确显示 TLS 证书校验失败，并且已经确认网络与目标域名可信时，才可
临时使用 `--insecure`。正常情况下不应使用该选项。

## 请求边界

脚本会执行：

1. 读取 CAS 登录页并校验表单、动态 `execution` 和验证码状态；
2. 保存 CAS 同源 JS/CSS；
3. 向经过校验的 CAS 表单提交一次登录；
4. 逐跳校验 HTTPS 重定向：UC 登录只允许 CAS 与 UC；移动门户流程只允许
   CAS、`itsapp.bjut.edu.cn` 与 `ydapp.bjut.edu.cn`；
   若 `itsapp` 返回已知的 `window.location + md5(服务端给定 IPv4)` 导航挑战，
   脚本只解析这一种受限表达式并以 GET 继续，不执行任意 JavaScript；
5. 读取 UC 的以下只读接口：
   - `/api/register/rules`
   - `/api/uc/status`
   - `/api/reset/rules`
   - `/api/uc/userinfo`
   - `/api/uc/commonConfig`
6. 复用同一临时 CAS 会话访问 `ydapp.bjut.edu.cn/openV8HomePage`；
7. 保存 UC、移动门户页面直接引用以及最多四层字面引用的同源 JS/CSS，供离线
   查找改密与充值接口、参数和响应结构。

脚本不会执行：

- UC 修改密码提交；
- 校园卡或网费充值；
- 付款、验证码发送、用户资料修改；
- 从前端资源中猜测并调用新发现的接口；
- 自动重复登录。

若 CAS 显示人机验证，脚本会在发送账号密码前停止。若出现意外域名、307/308
凭据重放或超过 12 次跳转，也会立即停止对应流程。

## 输出与隐私

输出位于 Git 已忽略的 `billing-capture.local/`：

- `cas-services-*.private/`：原始响应，可能包含个人信息、CAS 票据、Cookie 响应
  头和 `openid`；不要发送、同步或提交；
- `cas-services-*.share/`：自动脱敏后的可检查目录；
- `cas-services-*.share.zip`（或 `.tar.gz`）：检查后发送给 Codex 的文件。

分享包会移除输入的账号和密码、Cookie/会话标识、CAS `ST`/`TGT`、动态
`execution`、`openid`、常见令牌、姓名、学号/卡号、手机号、身份证号、IP 地址
和邮箱。移动门户公开的固定 `appid` 会保留，以便离线分析协议。
`report.json` 只保留响应形状、端点候选、`openid` 是否出现及其长度，不保留原值。
尽管如此，发送前仍应人工打开 `report.json` 与 `sanitized/` 检查一次。

脚本结束时会删除临时 Cookie。原始私有目录不会自动删除，以便需要时重新脱敏；
确认分析完成后可以手动删除它。
