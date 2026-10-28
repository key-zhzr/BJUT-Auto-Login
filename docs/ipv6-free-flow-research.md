# IPv6 免流功能 · 技术方案调研

> 状态：调研稿（未实现功能代码）
> 适用范围：仅当「学校 IPv6 出口不计费 / 双栈中 IPv6 不计费」这一前提成立时才有意义。
> 若学校对 IPv6 同样计量，下列任何方案都不会产生免流效果，仅改变出口协议。
> 全部能力默认**关闭**，由用户在设置中显式开启；不涉及绕过计费安全控制或认证校验。

## 1. 前提与边界

- 北京工业大学校园网当前提供双栈（`bjut_wifi` / `bjut-sushe` / `lgn` 有线均可能拿到 IPv6）。
- “免流”在本方案中定义为：**让符合条件的流量优先或仅走 IPv6 出口**，从而在“IPv6 不计费”的政策下减少 IPv4 计量。
- 不做：伪造计费报文、篡改账户计量字段、绕过登录/认证校验。这些都属于违规且本项目 `portal_auth` 已有防护（如 `PortalRouteContext` 拒绝 VPN/TUN Fake-IP）。
- 风险前提：单栈 IPv6 下，仅提供 IPv4 的服务（部分校内旧系统、部分 IPv4-only CDN）将不可达；需提供“回退到双栈”的一键开关与自动探测。

## 2. 现有 IPv6 支持盘点（代码证据）

| 能力 | 位置 | 说明 |
|---|---|---|
| 双栈连通性探测 | `src-tauri/src/dual_stack.rs`（`DualStackReport` / `FamilyConnectivity`，行 6–51） | 已分别探测 IPv4 / IPv6 可达性、地址、时延，并支持 `redact_addresses` 与 `online()` |
| Windows IPv6 源地址选择 | `src-tauri/src/portal_auth/lgn_ipv6_windows.rs`（`source_ipv6`，行 15） | 在没有接口绑定的 Windows reqwest 上，从拥有已验证物理 IPv4 的网卡挑选 IPv6 源地址 |
| LGN IPv6 网关与根 | `src-tauri/src/portal_auth.rs`（`LGN6_GATEWAY_IPV6` 行 72–75，`LGN6_ROOT`/`LGN6_HOST` 经 `lib.rs` 引入） | 已硬编码 lgn6 双栈网关地址 |
| 登录报文字段 | `src-tauri/src/portal_auth.rs`（`wlan_user_ipv6`、`login_ip_type` 等，见测试 `lgn_ipv4_fallback_keeps_an_empty_ipv6_field` 行 1902） | 登录 URL 已携带 IPv6 字段，支持“空 IPv6 字段”回退 |
| 网络探测超时 | `src-tauri/src/network_probe.rs`（`NETWORK_PROBE_TIMEOUT`） | 双栈探测统一超时基准 |

结论：项目已具备“能探测 IPv6、能在 Windows 选 IPv6 源、能在登录报文带 IPv6”的基础，但**没有**“按策略切换出口 / 展示免流状态 / IPv6 单栈模式 / 免流相关保活增强”的用户层能力。

## 3. 四种实现形态与架构

### 3.1 IPv6 出口优先（用户勾选）
- 目标：在双栈可用时，优先经 IPv6 建立认证/计费会话与后续流量出口。
- 改动点：
  - `portal_auth.rs`：在构造登录/探测请求时，依据配置 `prefer_ipv6_egress` 优先使用 `LGN6_*` / `wlgn` 的 IPv6 路径，并把 `wlan_user_ipv6` 填为真实地址而非空。
  - `lgn_ipv6_windows.rs`：将“选 IPv6 源”从仅 Windows 扩展到受控策略；非 Windows 走 `reqwest` 接口绑定。
  - `config_model.rs`：新增 `prefer_ipv6_egress: bool`（默认 false，`#[serde(default)]`，带别名兼容）。
  - `src/appearance.ts` / 设置 UI：新增开关与说明文案（引用 `src/ui-text.ts` 文案机制）。
- 风险：部分运营商/IPv6 路径到某些资源更慢或不可达；需保留“探测失败自动回退双栈”。

### 3.2 免流状态展示（用户勾选）
- 目标：在诊断/网络页展示 IPv6 连通性、当前出口族、是否处于“免流（IPv6 不计费）”状态、IPv4/IPv6 流量占比。
- 改动点：
  - `dual_stack.rs`：扩展 `DualStackReport`（新增 `egress_family`、`ipv6_unmetered_assumed` 等字段，保持向后兼容）。
  - 前端：`src/network-experience.ts`、`src/diagnostics-view.ts`、`src/network-progress-view.ts` 增加免流状态卡片；新增 `src/ipv6-freeflow-view.ts`（仿 `device-usage-view.ts` 结构）。
  - 文案：`src/ui-text.ts` 增加中英文案，遵循现有 `applyConfiguredUiText`。
- 风险：流量占比需计费接口支持按族拆分；若不支持则只展示“出口族”与“假设”，不臆造数字。

### 3.3 增强 IPv6 登录保活（用户勾选）
- 目标：避免 IPv6 会话静默失效后回退到计费 IPv4；在 IPv6 路径异常时可控地重连或告警。
- 改动点：
  - `portal_auth.rs` / `connectivity.rs`：在后台轮询（`check_interval` / `check_interval_bg`）中加入“IPv6 会话存活”检查；失效时按配置选择“重连 IPv6”或“告警并维持双栈”。
  - `login_progress.rs` / `network_progress.rs`：增加 IPv6 保活阶段进度。
- 风险：频繁探测增加耗电（移动端敏感）；需尊重现有后台间隔与 Android 省电策略。

### 3.4 临时 IPv6 单栈模式（用户勾选）
- 目标：在需要“仅 IPv6”的场景临时禁用 IPv4 计量路径（例如宿舍/实验室特定时段），用完一键恢复双栈。
- 实现约束（平台相关，需谨慎）：
  - **Windows / macOS / Linux**：本项目不直接管理 IP 栈；应通过“优先 IPv6 + 在系统层提示用户关闭 IPv4 获取（DHCPv4）”或调用平台 API（Windows `netsh`/`ipconfig`、macOS `networksetup`、Linux `ip`）实现，且必须有明确回滚。
  - **Android**：需 `CHANGE_NETWORK_STATE` 等权限，且受厂商 ROM 限制；保活已在 `src-tauri/gen/android/.../KeepAliveService.kt` 等处实现，单栈切换风险高，建议仅在 rooted/受控环境提供或仅做“提示手动操作”。
- 风险：单栈下 IPv4-only 资源不可达；必须由用户显式触发、限时、可一键恢复，并在 UI 明确警示。

## 4. 统一配置与默认值

建议在 `AppConfig`（`config_model.rs`）新增一组默认值全 false 的开关，集中在“高级/网络”设置页：

```rust
#[serde(default)]
pub(crate) prefer_ipv6_egress: bool,
#[serde(default)]
pub(crate) show_ipv6_freeflow_status: bool,
#[serde(default)]
pub(crate) ipv6_keepalive_enhanced: bool,
#[serde(default)]
pub(crate) temporary_ipv6_only: bool,
```

所有开关默认关闭；`temporary_ipv6_only` 在启用时强制要求“自动恢复定时器 + 手动恢复入口”。

## 5. 验证方案

- 单元/集成（Rust，`cargo test`）：
  - `dual_stack.rs` 增加“强制 IPv6 源 / 单栈回退”用例（复用现有 `Family::matches` 测试设施）。
  - `portal_auth.rs` 增加“开启 `prefer_ipv6_egress` 时登录报文携带真实 `wlan_user_ipv6`”用例（沿用 `lgn_ipv4_fallback_keeps_an_empty_ipv6_field` 风格）。
  - `lgn_ipv6_windows.rs::source_ipv6` 既有 Windows 测试保持；非 Windows 绑定逻辑加 mock 测试。
- 前端（Node + Playwright）：
  - 新增 `scripts/test-ipv6-freeflow.mjs` 校验配置默认关闭与开关互斥；
  - `playwright` 增加免流状态卡片的回归用例（仿 `scripts/browser/app.spec.ts`）。
- 手动：在真实校园网双栈环境验证“开启后出口族变化、关闭后回退、单栈模式可恢复”。

## 6. 实施建议顺序

1. 先合入配置与默认值（低风险，纯新增，不触动现有逻辑）。
2. 再做状态展示（只读展示，不改变网络行为，风险最低）。
3. 再做出口优先（改变会话建立路径，需充分回退）。
4. 最后做单栈模式（平台风险最高，建议先做 Windows/macOS 提示型，Android 仅提示或受控环境）。

> 是否进入功能代码实现，以及从哪一形态开始，待你确认。在此之前本项目不会修改任何网络行为相关代码。
