# lgn IPv4+IPv6 认证

协议依据为 `lgn.bjut.edu.cn-v4+v6-login+logout.har` 中成功的登录、在线记录和注销请求。

1. 从校园网物理接口访问 `https://lgn6.bjut.edu.cn/drcom/getipv6`，读取 JSONP 的 `ip` 字段。这是只读请求，不需要账号密码。
2. 向 `https://lgn.bjut.edu.cn:802/eportal/portal/login` 发送一次 GET 请求，同时携带物理接口 IPv4 和上一步返回的 IPv6。账号前缀为 `,0,`，`login_ip_type=0`，`jsVersion=4.2.2`。按门户脚本的 XOR `0x16` 规则编码字段，并附加 `encrypt=1`；最后的 `v` 和 `lang=zh` 不编码。
3. 注销通过同一接口族的 `/eportal/portal/logout`，同时提交两种地址，使用门户规定的固定占位字段 `user_account=drcom`、`user_password=123`，并采用相同编码。

旧版 `/V6` 预认证和表单 POST 不属于这份抓包的认证流程，已移除。登录后以 JSONP `result` 判断结果；提交后无法确认响应时，不自动切换协议或再次提交。若登录前无法发现 IPv6，保留单 IPv4 认证，并在结果中明确说明。

IPv6 发现使用独立客户端：reqwest 的 IPv4 源地址绑定会过滤 IPv6 目标，不能直接复用 ePortal 的 IPv4 客户端。macOS/Linux 通过接口绑定保留校园网路由，Windows 从同一 IPv4 所在网卡选择有效的 BJUT IPv6，Android 保留已选择的 Network 绑定。IPv6 发现保留原生 TLS、证书校验、主机名和固定网关解析，禁止代理与重定向。

诊断实际获取 IPv6 地址；单纯打开登录页不算地址发现成功。JSONP 不可用时，可只读解析 lgn6 登录页的客户端地址字段。完整地址发现过程有超时限制。

回归样例位于 `src-tauri/src/portal_auth/fixtures/lgn-dual-stack.json`，仅保留协议字段和成功响应，账号、密码及客户端地址均替换为测试值。测试核对全部请求参数（包括重复的 `lang`），并在 macOS/Linux 的 IPv6 回环接口上验证真实连接。

运行：`cargo test --manifest-path src-tauri/Cargo.toml portal_auth::tests`。

较低兼容模式按物理源 IPv4 选择校园 DNS：lgn 有线的 `172.26/16` 使用 `172.21.0.21`、`172.21.201.22`，其他已知接口保留 `10.21.200.28`。没有接口信息时，lgn/lgn6 域名采用有线 DNS。两台有线 DNS 并发查询，任一有效应答即可继续；DNS 故障不会阻止独立的 IPv6 门户探测。

DNS、互联网连通性、认证网关诊断同时执行，统一采用 3 秒超时；报告保留各项实际耗时，等待期间持续更新进度。协议识别也并发执行，但按物理接口对应的优先级处理结果：首选协议确认后立即返回，不再等待不可达的次选网关。手动登录的安全预检查和提交前网络身份复核仍然保留。
