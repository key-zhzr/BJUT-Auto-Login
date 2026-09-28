# 一卡通会话适配（2.8.8）

依据用户提供的日新工大 Android 2.8.8 APK 的调用关系实现，未执行 APK，未复制抓包中的凭据。引用会话中的字符串推测只用于定位线索。

## 已核实的流程

1. `BjutNetRepository.checkToken`：向 itsapp 的 `/bjutapp/wap/app-login/check` **POST** `ticket`。
2. CAS 入口是 `/a_bjut/api/sso/index?redirect=/bjutapp/wap/app-login/local-login&from=wap`。CAS 的 service 和回跳参数均严格核验。
3. `local-login` 是 WebView 拦截的回调，不能继续请求这个页面。官方 `toCasLogin` 在此同步 Cookie，然后调用 native `login`。
4. `login` 向 `/bjutapp/wap/app-login/login` POST `imei`、`sid`、`mobile_type`；前两个字段都是应用生成的 UUID。成功响应为 `e:0`，票据位于 `d.login_ticket`。
5. `beforeInitialWebLoad` 等待 `checkToken`，随后才在共享 Cookie 会话中访问 ydapp。

原生接口与网页导航使用不同请求格式。`HeaderInterceptor` 在 API 请求中加入 `from-eai:1` 与 `authorization-str`，后者为包含秒级字符串时间戳及 ticket 的 JSON。`rsaPost` 将 JSON 编码为表单 `content`；请求和响应使用 APK 自带的 RSA/PKCS#1 v1.5 分块格式（117 字节请求明文块、128 字节密文块）。响应也可能是明文 `e/m/d` 错误对象。网页只使用 WebView UA 和 Cookie，不携带 API 的授权头。

代码中的响应解码常量是该客户端 APK 随包分发的共享协议材料，并非用户私钥或服务器签名密钥。这套旧格式仅用于互操作；TLS 证书校验始终启用，本机密码与备份的加密方式不变。不要把这种协议编码用于保护新的本地数据。

`&token=` 来自同一 APK 的 **SDU** 网页模块，不能套用到 BJUT。未添加猜测的 token 查询参数。

## 实现约束

- 首次匿名会话检查可取得 `eai-sess` / `UUkey`，无需提交安装统计或读取硬件 IMEI。
- 只有已明确核实的 `10013`（用户信息已失效）触发重新 CAS 登录；网络、解码错误不会触发密码重试。
- Cookie 遵守域名与路径范围，CAS Cookie 不会复制给 itsapp 或 ydapp。
- 票据按账号加密保存，不交给前端，不写日志、不进入配置备份；账号密码变化后沿用现有会话失效机制。
- 只改登录与核对阶段，订单提交、二次确认、结果不明确时不自动重试等付款约束保持有效。

## 验证记录与待复测

2026-09-28，以空票据、不含账号密码的只读请求访问真实 `/check`，服务返回 HTTP 200；上述封装成功解码为 `e:10013`、`m:用户信息已失效`，并签发访客 Cookie。未进行真实账号登录或支付。

本地 HTTPS 模拟覆盖访客 Cookie、CAS 表单、SSO 导航挑战、拦截 local-login、取得及校验票据、不同域名 Cookie 隔离、账号隔离和安全存储序列化。另有多块 UTF-8 解码、损坏响应、未知回跳地址及旧配置兼容测试。

仍需真实账号核验服务端登录后的 ydapp 入口与充值信息返回，以及实际需要图片验证时的 jfself/WebVPN 页面。模拟通过不等于实际充值成功。
