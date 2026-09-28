# 一卡通网页登录与 datajson（2026-09-28）

依据用户提供的 `ydapp.bjut.edu.cn-yda-ua.har` 及其中引用的学校公开脚本核验。原始 HAR/PCAP、账号、Cookie、验证码、票据和个人数据不得提交仓库。

## 登录路径

实际 HAR 保留了原有链条：

`ydapp/openV8HomePage → itsapp/uc/api/oauth/index → itsapp/uc/wap/login → itsapp/a_bjut/api/sso/index → CAS → itsapp → ydapp/openV8HomePage → 首页 fragment 中的 openid`

不需要先调用日新工大 App 的 native 登录接口。本实现直接使用这条网页登录路径；不提交安装统计，不读取硬件标记，不保留 App 原生登录票据或协议密钥。

HAR 的浏览器标识同时含微信与 `ZhilinBjutApp` 后缀，CAS 密码提交后出现 `formToken`，再由 `sendToken` / `submitToken` 完成短信验证。用户实测纯微信 UA 不触发这一分支。因此充值会话从首次访问 ydapp 到 CAS 表单提交、返回页面、后续接口都保持同一个纯微信 UA。统一认证账户设置另用原有普通浏览器会话，不受此更改影响。学校仍要求短信时明确报错，不反复提交密码，也不假装验证成功。

PCAP 中 BJUT 登录连接采用 TLS，未提供解密密钥；只能确认连接，不能据此推断明文登录参数。网页 HAR 是本轮协议适配的主要依据。

## datajson 格式

学校 `index.621bca5e.js` 的通用请求函数与 `$myRequest` 都先将参数包为 `{ "datajson": "..." }`，并对返回的同名字段解码：

1. 生成 16 个 ASCII 字母或数字作为 AES-128 密钥。
2. 业务 JSON 的 UTF-8 字节使用 AES-ECB、PKCS#7 填充，密文使用标准 Base64。
3. 密钥左移 10 位再反转，作为密文之前的 16 字符前缀。
4. 解码前缀时先反转，再左移 6 位，得到原密钥。

这个前缀携带解码材料，所以它只是服务端要求的传输封装，不能作为独立的安全加密。实现始终验证 HTTPS 证书；本机安全存储和备份仍使用原有加密。

所有 ydapp JSON POST、支付状态 GET 参数、支付宝 HTML 返回均经过该封装。GET 将参数包放在单个 `datajson` 查询参数中。兼容没有封装的旧 JSON/HTML 响应；出现封装但解码失败时中止，不把密文当作业务错误，也不再次提交订单。业务字段和网费通道仍沿用此前真实充值抓包核实的行为。

## 验证

- 本地私有 HAR 测试实际解码了捕获的请求和响应，响应 `success:true`，可读取业务对象；不输出个人字段值。
- 固定 AES 向量由独立 OpenSSL 工具生成；测试包含中文、多块内容、随机新密钥、旧 JSON、HTML、截断与无效填充。
- CI 在 Windows 上运行封装与验证码测试，Linux 运行完整 Rust 测试。
- 未重放 HAR 中的 Cookie、短信验证码或请求，自动测试未提交真实订单、未扣费。用户随后实测确认充值已恢复正常。

本地抓包回归通过 `BJUT_YDAPP_TEST_HAR` 指定私有文件后，仅运行忽略测试 `validates_local_private_har_without_printing_payloads`。测试文件不会被复制或写入仓库。
