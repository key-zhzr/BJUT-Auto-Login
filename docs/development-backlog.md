# 增量开发待办清单（简单 PRD）

> 基线：`main` @ `a012353`（App 0.1.6）
> 状态：待评审。本文档只定义待办与验收标准，**不含任何源代码改动**。
> 约束：保持现有功能与兼容性，最小变更，不做无关重构；每项独立验收，验证通过后才进入下一项。

## 0. 执行状态（2026-10-29）

| 项 | 状态 | 提交 / 说明 |
| --- | --- | --- |
| P0-01 锁中毒收敛 | 已完成 | `65d7e63`，本地测试与基线对比通过 |
| P0-02 超时预算分层 | 已完成 | `57f8671`，`portal_auth` 41 项通过；实机日志待补 |
| P0-03 歧义认证测试 | 已完成 | `ea43f00`，纯测试，2 个用例 |
| P0-04 错误分支测试 | 已完成 | `3a4e376`，纯测试，6 个用例 |
| P1-01 端点单一来源 | 已完成 | `80b1109`，新增 `endpoints.rs` 与 2 个 golden 测试 |
| P1-04 测试入口守卫 | 已完成 | `943197f`，6 个前端脚本全注册 |
| P1-05 首帧文案守卫 | 已完成 | `b0e6ba2`，34 项前端测试通过 |
| P1-06 平台 CI 子集 | 已完成（待 CI 验证） | `23f5774`；GitHub Actions 未启用，尚无法实际跑 |
| P2-02 `md5` 升级 | 已完成 | `652a2d9`，0.7.0 → 0.8.1，调用点 5 处不变 |
| P1-02 端点覆盖配置 | 未开始 | 需先定稿覆盖数据模型与配置传入链路 |
| P1-03 证书指纹替换 | 未开始 | 需先定稿 TLS 校验链与安全边界 |
| P1-06 之后的 P2-01/P2-03 | 未开始 | 跨平台 TLS 后端改动，建议 Actions 启用后做 |
| IPv6 免流 | 未开始 | 仅完成调研；行为改动需真实计费数据 Gate |

> 第一批验证记录见 `docs/verification-2026-10-28.md`。当前 fork 的 GitHub Actions 为 0 次运行，本地 Rust stable 1.99 + VS BuildTools 2026 是实际门禁；全量测试 262 passed / 2 failed（均为既有环境失败），clippy 10 个既有错误、零新增。

## 1. 项目信息

| 项 | 内容 |
| --- | --- |
| Language | 中文 |
| 技术栈 | Tauri 2 + Rust（`src-tauri/`）+ TypeScript（`src/`、`index.html`）+ Android（`src-tauri/gen/android`） |
| Project Name | `bjut_auto_login` |
| 现有规模 | Rust 库测试 261 项；前端 5 个 `scripts/test-*.mjs`；Playwright `app` / `luci` 两个 project；CI 4 个 job |
| 原始需求复述 | 在现有校园网自动登录助手上做增量改进，方向为：①P0 登录核心健壮性；②P1 端点/证书可配置化；③P1 测试与 CI 增强；④P2 依赖与构建健康；⑤新功能「IPv6 免流」（出口优先、状态展示、保活增强、临时单栈、方案调研）。前提是学校 IPv6 不计费；全部默认关闭、显式开启、可回滚；不涉及绕过计费或认证安全控制。 |

## 2. 产品目标与范围

### 2.1 三个正交目标

1. **稳定性**：登录链路无静默失败、无后台崩溃。自动登录在异常后能自我恢复，失败原因可定位。
2. **可维护性**：学校侧网关地址、证书变化时，无需等待新版本发布即可适配；默认行为严格不变。
3. **可选增强**：在「学校 IPv6 不计费」这一前提被证实后，提供 IPv6 免流能力，默认关闭、随时一键回滚。

### 2.2 本轮做

- 登录核心的崩溃面收敛、超时预算修正、凭据重复提交防护、错误分支测试补齐。
- 网关端点与宿舍证书 pin 的集中化与可配置化（默认值 = 当前硬编码值，行为不变）。
- 测试入口完整性守卫、UI 文案回退一致性测试、Windows/macOS CI 覆盖核心测试子集。
- 依赖健康：reqwest TLS 后端统一、`md5` 升级、重复依赖收敛记录。
- IPv6 免流：先出前提确认结论，再依次做配置骨架 → 只读状态展示 → 出口优先 → 保活增强 → 单栈提示。

### 2.3 本轮不做

见第 6 节「非目标」。

## 3. 用户故事

| 角色 | 故事 |
| --- | --- |
| 在校学生（宿舍 / 教学楼双栈网络） | 作为每天在 Wi-Fi 与有线间切换的用户，我希望切换后自动登录不会静默失败，失败时能看到"是超时、证书问题还是密码错"，以便我自己判断要不要重试。 |
| 进阶用户（自建 OpenWrt、跨校区） | 作为会自己抓包的用户，我希望学校换网关地址或换证书时，我能改配置而不是等下一个版本，同时默认值不要被悄悄改掉。 |
| 省流量用户 | 作为流量吃紧的用户，我希望在 IPv6 不计费时让流量优先走 IPv6，并能清楚看到"当前出口是哪个协议族、是否处于免流状态"，随时能一键关掉回退双栈。 |
| Android 用户 | 作为手机后台常驻用户，我希望 IPv6 相关增强不要额外耗电、不要破坏现有保活与省电策略。 |
| 贡献者 / 维护者 | 作为提交 PR 的人，我希望 CI 能在 Windows/macOS 上跑到我改动的核心测试，也希望新增的测试脚本不会被漏注册。 |

## 4. 待办清单

优先级：P0 必须做、P1 应该做、P2 可做（本轮排期但不阻塞前两批）。
验收命令统一使用仓库既有入口：`cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、`cargo test --manifest-path src-tauri/Cargo.toml`、`npm run typecheck`、`npm run build`、`npm run test:frontend`、`npm run test:browser -- --project=app`、`npm run check:versions`。

### 4.1 P0 登录核心健壮性

| ID | 优先级 | 任务 | 目标 | 范围（文件 / 模块） | 验收标准（可执行） | 依赖 | 风险 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P0-01 | P0 | 后台关键路径消除锁中毒 panic | 任一线程 panic 导致互斥锁中毒后，后台轮询与自动登录不再二次 panic 而静默停止 | `src-tauri/src/connectivity.rs`（非测试段 7 处 `.lock().unwrap()` / `.read().unwrap()`，行 80/91/123/142/200/217/305）；`src-tauri/src/lib.rs` 后台任务函数内同类写法 | 1) `connectivity.rs` 的 `#[cfg(test)]` 之前不再出现 `.unwrap()`/`.expect(`（`awk '/#\[cfg\(test\)\]/{exit} 1'` 后 grep 计数为 0）；2) 新增单测 `poisoned_lock_does_not_panic`（构造中毒锁后调用恢复式获取）通过；3) `cargo clippy --all-targets -- -D warnings` 与 `cargo test` 全绿 | 无 | 全仓现有 188 处同类写法，**只改后台关键路径**，避免大范围改动引入回归 |
| P0-02 | P0 | 登录阶段超时预算分层 | 内层单请求超时必须短于外层阶段预算，超时后仍能拿到逐地址失败原因，不再退化为一句"IPv6 地址发现超时" | `src-tauri/src/portal_auth.rs`：`discover_lgn_ipv6` 外层 `tokio::time::timeout`（行 1095/1647）与内层 client `timeout`（行 1496/1589 附近）当前同为 5s | 1) 超时数值集中为常量，新增单测断言 `内层 < 外层`；2) `cargo test --manifest-path src-tauri/Cargo.toml portal_auth` 全绿；3) 实机日志中 IPv6 发现超时时可看到逐地址错误明细（贴日志片段入 `docs/`） | 无 | 数值调整可能改变极端慢网络下的判定时机，需保留总时长不劣于现状 |
| P0-03 | P0 | 歧义认证结果不重复提交凭据（三协议一致性） | 凭据已提交但响应不可解析时，绝不重试或切换到第二套协议，避免触发账号锁定 | `src-tauri/src/portal_auth.rs`：`AMBIGUOUS_LOGIN_RESULT`（行 24）、`login_result_is_ambiguous`、`login_lgn_once`（行 1082 起）；Type1 dorm / Type2 wifi 对应登录函数；`src-tauri/src/portal_auth/fixtures/` | 1) 新增测试覆盖 Type1/Type2/Type3 在响应不可解析时**仅提交一次**（提交计数器断言 = 1）；2) `cargo test --manifest-path src-tauri/Cargo.toml ambiguous` 通过；3) 若发现某协议存在重试，修复后同一测试由失败转通过（测试先行） | 无 | 现有 Type3 已有注释级保护，Type1/Type2 是否同样受保护需实测确认 |
| P0-04 | P0 | 登录错误分支测试补齐 | 三类协议的主要失败形态有回归保护，后续改动不会静默劣化错误语义 | `src-tauri/src/portal_auth.rs` 测试模块；`src-tauri/src/portal_auth/fixtures/` | 1) 新增 ≥6 个用例：连接超时、连接被拒、TLS 证书错误、JSONP 解析失败、空响应、HTTP 非 200；2) `cargo test --manifest-path src-tauri/Cargo.toml portal_auth` 全绿；3) `git diff --stat` 仅涉及测试代码与 fixtures，非测试代码零改动 | 无 | 需构造稳定 fixture，避免依赖真实网络 |

### 4.2 P1 端点 / 证书可配置化

| ID | 优先级 | 任务 | 目标 | 范围（文件 / 模块） | 验收标准（可执行） | 依赖 | 风险 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P1-01 | P1 | 网关端点收敛为单一来源 | 所有生产用网关地址/域名常量集中一处，后续可配置化只改一个模块 | 新增 `src-tauri/src/portal_auth/endpoints.rs`；`src-tauri/src/portal_auth.rs`（常量区 60–88）、`src-tauri/src/portal_auth/dorm_tls.rs`（`TLS_HOST_CANDIDATES`）、`src-tauri/src/lib.rs`（2138–2140 `WLGN_HOST`/`LGN_HOST`/`LGN6_HOST`） | 1) 生产代码中 `10.21.221.98`、`10.21.251.3`、`172.30.201.2`、`lgn6.bjut.edu.cn` 字面量仅出现在 `endpoints.rs`（用 `grep -rn` 列出后逐条确认，测试代码与注释不计）；2) 新增 golden 单测断言三协议登录/登出/探测 URL 与改动前逐字一致；3) `cargo test` 全绿，现有 URL 相关测试断言不改 | 无 | 常量搬家若漏改一处即造成静默错地址，必须用 golden 测试兜底 |
| P1-02 | P1 | 端点覆盖配置（默认空 = 内置值） | 学校变更地址后，用户可自行覆盖而不必等发版；默认行为完全不变 | `src-tauri/src/config_model.rs`（新增 `#[serde(default)]` 字段，默认空）、`endpoints.rs`（覆盖解析与校验）、`src/ui-copy/settings.ts` + `index.html`（高级设置入口） | 1) 单测 4 条：缺省走内置值、非法 IP/域名被拒绝保存、合法覆盖生效、清空后恢复内置；2) 旧配置文件（无该字段）加载后行为与当前一致；3) `npm run typecheck`、`npm run build`、`npm run test:browser -- --project=app` 通过 | P1-01 | 覆盖值可把流量导向错误主机，必须仅允许合法 IP/域名且不携带 scheme/凭据 |
| P1-03 | P1 | 宿舍网关证书 pin 可维护化 | 学校换证后无需发版即可适配；默认仍严格校验，pin 例外仅对白名单 SNI + 精确指纹生效 | `src-tauri/src/portal_auth/dorm_tls.rs`（`EXPIRED_DORM_CERT_SHA256`、`PINNED_VALIDATION_TIME` 当前硬编码，证书有效期 2024-08-08～2025-09-09 已过期）、`config_model.rs`、`index.html` 高级设置说明 | 1) 单测 4 条：未知指纹仍拒绝、关闭 pin 例外后过期证书拒绝、配置新指纹后通过、指纹格式非法拒绝；2) 默认配置下与当前行为逐项一致（现有证书/主机名/过期测试断言不改）；3) `cargo test --manifest-path src-tauri/Cargo.toml dorm` 通过 | P1-01 | 安全敏感项。任何放宽都必须显式开启、限定 SNI 白名单、且 UI 明确风险提示 |

### 4.3 P1 测试与 CI 增强

| ID | 优先级 | 任务 | 目标 | 范围（文件 / 模块） | 验收标准（可执行） | 依赖 | 风险 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P1-04 | P1 | 前端测试入口完整性守卫 | 新增的 `scripts/test-*.mjs` 不会被漏出 `npm run test:frontend` | 新增 `scripts/check-test-coverage.mjs`；`package.json`；`.github/workflows/ci.yml` | 1) `node scripts/check-test-coverage.mjs` 退出码 0；2) 临时新建 `scripts/test-dummy.mjs` 未注册时退出码非 0（验证后删除该文件）；3) CI `verify` job 增加该步骤并全绿 | 无 | 脚本需容忍 `test-update-utils.mjs` 与 `--test` 两类执行方式 |
| P1-05 | P1 | UI 文案回退一致性测试 | 保证「JS 未加载时首帧文案完整」这一既有约定不被破坏 | 新增 `scripts/test-ui-copy-fallback.mjs`；`src/ui-copy/settings.ts`、`src/ui-copy/billing.ts`、`index.html`、`package.json`、CI | 1) 脚本对 `SETTINGS_COPY.items` 每个 key 断言 `index.html` 存在 `id="<key>"` 且标题字符串出现；2) 当前代码下退出码 0；3) 删除 `index.html` 中任一 fallback 标题后脚本失败（手动验证一次并恢复） | 无 | 文案属中文自然语言，匹配需允许空白差异，避免误报 |
| P1-06 | P1 | Windows / macOS CI 增加核心 Rust 测试子集 | 平台相关回归（IPv6 源地址选择、双栈探测）在目标平台真实执行 | `.github/workflows/ci.yml`（`desktop-platform-check` job） | 1) 新增步骤运行 `cargo test --manifest-path src-tauri/Cargo.toml --locked portal_auth::tests` 与 `dual_stack::tests`；2) 两平台 CI 全绿；3) 单 job 总时长仍低于现有 30 分钟上限 | P0-04、P1-01 完成后再加，避免测试名频繁变动 | 平台测试可能暴露既有平台差异，需预留修复时间 |

### 4.4 P2 依赖与构建健康

| ID | 优先级 | 任务 | 目标 | 范围（文件 / 模块） | 验收标准（可执行） | 依赖 | 风险 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P2-01 | P2 | reqwest TLS 后端统一 | 去掉 vendored OpenSSL，缩短构建时间与产物体积；Windows 保留系统 TLS（Schannel） | `src-tauri/Cargo.toml`（行 28 当前同时启用 `native-tls-vendored` 与 `rustls-tls`）；`src-tauri/src/campus_services.rs`（行 992 `use_native_tls()`）；`src-tauri/src/portal_auth.rs`（行 348–356 native_tls 回退分支） | 1) `Cargo.toml` 不再出现 `native-tls-vendored`，Windows 改走 `native-tls`（Schannel）；2) `cargo tree -i openssl-sys` 无输出；3) CI 四平台（ubuntu / macos / windows / android）全绿；4) Android APK 体积变化写入 `docs/dependency-health.md` | 无 | 现有 Windows native_tls 回退是为规避 Schannel 证书链卡顿，去掉 vendored 后需实机复测 lgn IPv6 发现 |
| P2-02 | P2 | `md5` 0.7 → 0.8 | 消除过旧加密哈希依赖版本 | `src-tauri/Cargo.toml`（行 40）；调用点 `campus_services.rs` 2036/2630/2654/2700、`lib.rs` 7374 | 1) 升级后 `cargo test --manifest-path src-tauri/Cargo.toml campus_services` 与 `lib` 相关测试全绿；2) 现有 digest 断言（`192.0.2.8`、`2001:db8::8`）数值不变；3) `grep -rn "md5::compute" src-tauri/src \| wc -l` 仍为 5（无新增调用点） | 无 | 低。若 API 有变，改动限于调用点，不新增封装 |
| P2-03 | P2 | 重复依赖收敛记录 | 避免同一 crate 多版本共存导致体积与行为分叉 | `src-tauri/Cargo.toml`、新增 `docs/dependency-health.md` | 1) `cargo tree -d --manifest-path src-tauri/Cargo.toml` 中 `rustls`、`reqwest`、`sha2`、`tokio` 无多版本共存，或例外逐条记录原因；2) 记录 TLS 后端矩阵（rustls / native-tls 各用在哪些模块）；3) `npm run check:versions` 通过 | P2-01 | 纯记录类，不得借机做大版本升级 |
| P2-04 | P2 | 恢复 `clippy -D warnings` 基线 | 当前 Rust 1.99 下 HEAD 已有 10 个 clippy 错误，导致 CI lint 门禁无法作为有效判据 | `src-tauri/src/lib.rs`（464/684-689/1310）、`src-tauri/src/probe_transport.rs`（452/454）、`src-tauri/src/dual_stack.rs`（72）；必要时加 `rust-toolchain.toml` 固定版本 | 1) `cargo clippy --all-targets -- -D warnings` 退出码 0；2) 修复不改变行为，现有测试全绿；3) 若选择固定工具链，记录版本与升级策略 | 无 | 10 个错误均为既有问题；逐条修复需避免为消 lint 而改变语义 |

### 4.5 P2 新功能：IPv6 免流

依据 `docs/ipv6-free-flow-research.md`。全部能力**默认关闭**、显式开启、可回滚；不涉及伪造计费报文、篡改计量字段或绕过认证校验。

| ID | 优先级 | 任务 | 目标 | 范围（文件 / 模块） | 验收标准（可执行） | 依赖 | 风险 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| IPV6-01 | P2 | 前提确认与方案定稿（Gate） | 用真实数据判定「学校 IPv6 不计费」是否成立，成立才继续；不成立则关闭整个方向 | 新增 `docs/ipv6-prerequisite.md`；增补 `docs/ipv6-free-flow-research.md` 结论节 | 1) 文档记录确认方式：同一账号、同一时段，对比"仅 IPv4 出口"与"IPv6 优先"各 ≥24h 的计费系统流量增量，写明日期、数值、结论；2) 明确平台范围（哪些系统做行为改动，哪些只做提示）；3) 结论为"不计费"才放行 IPV6-02 起任务，否则本方向整体关闭并记录原因 | 无 | 前提不成立时所有后续工作无收益，必须前置 |
| IPV6-02 | P2 | 配置骨架（4 个开关，默认关闭） | 先把开关与兼容性打好，不改任何网络行为 | `src-tauri/src/config_model.rs`（`prefer_ipv6_egress`、`show_ipv6_freeflow_status`、`ipv6_keepalive_enhanced`、`temporary_ipv6_only`，全部 `#[serde(default)]`）；`src/ui-copy/settings.ts`、`index.html`、`src/appearance.ts` | 1) 单测：缺省全 false；旧配置（无这些字段）加载后仍全 false；`temporary_ipv6_only=true` 但未设恢复时限时保存被拒；2) `cargo test`、`npm run typecheck`、`npm run test:frontend` 通过；3) 网络行为零改动（`git diff` 不涉及 `portal_auth.rs` / `dual_stack.rs` 逻辑） | IPV6-01 | 配置字段一旦发布即成为兼容契约，命名与别名需一次定好 |
| IPV6-03 | P2 | 免流状态展示（只读） | 用户能看到 IPv6 连通性、当前出口族、是否处于免流假设状态；不发起新请求 | `src-tauri/src/dual_stack.rs`（`DualStackReport` 新增 `egress_family`、`ipv6_unmetered_assumed` 等字段，`#[serde(default)]`）；`src/network-experience.ts`、`src/diagnostics-view.ts`、新增 `src/ipv6-freeflow-view.ts`、`src/ui-text.ts`、`scripts/browser/app.spec.ts` | 1) 序列化兼容测试：旧 `DualStackReport` JSON 反序列化成功；2) Playwright 用例：开关关闭时卡片显示"未启用"，开启且双栈可用时显示出口族；3) 代码审查确认未新增 reqwest client 构造（`grep -c "Client::builder()"` 数量不变）；4) `npm run test:browser -- --project=app` 通过 | IPV6-02 | 若计费接口不支持按协议族拆分流量，**只展示出口族与假设，不臆造流量占比数字** |
| IPV6-04 | P2 | IPv6 出口优先 | 双栈可用时优先经 IPv6 建立会话与出口；不可用时自动回退双栈 | `src-tauri/src/portal_auth.rs`（`lgn_login_url`、`discover_lgn_ipv6`、`portal_client`）；`src-tauri/src/portal_auth/lgn_ipv6_windows.rs`；`src-tauri/src/dual_stack.rs` | 1) 单测：开启后登录报文 `wlan_user_ipv6` 为真实地址（新增用例，与现有 `lgn_ipv4_fallback_keeps_an_empty_ipv6_field` 互为反向）；2) 单测：IPv6 不可达时自动回退且原因入日志；3) 关闭时现有全部测试断言不改且全绿；4) 实机双栈环境验证：开启后出口族变化、关闭后恢复（记录到 `docs/`） | IPV6-03 | 改变会话建立路径，是风险最高的一档；必须"探测失败即回退"，且回退路径与当前完全一致 |
| IPV6-05 | P2 | 增强 IPv6 登录保活 | IPv6 会话静默失效时可控地重连或告警，不悄悄退回计费 IPv4 | `src-tauri/src/connectivity.rs`、`src-tauri/src/lib.rs`（后台轮询，尊重 `check_interval` / `check_interval_bg`）、`src-tauri/src/login_progress.rs`、`src-tauri/src/network_progress.rs` | 1) 单测：IPv6 会话失效时按配置重连或仅告警（两条分支各一例）；2) 不改变 `check_interval_bg` 语义（既有测试不改且通过）；3) 关闭开关时零额外探测（探测计数断言 = 0）；4) Android 后台 30 分钟耗电对比记录，相对基线增幅在阈值内（阈值见待确认问题） | IPV6-04 | 移动端耗电与厂商 ROM 省电策略冲突，可能被系统杀进程 |
| IPV6-06 | P2 | 临时 IPv6 单栈（提示型先行） | 需要"仅 IPv6"时给用户可逆的操作路径，不由 App 直接改系统网络配置 | `src-tauri/src/network_platform.rs`、`src-tauri/src/native_network.rs`（仅生成提示与系统操作指引，不执行改配置）、`index.html`、`src/ui-copy/settings.ts` | 1) 启用需二次确认，且必须设置恢复时限，否则拒绝启用（单测）；2) 到点提示恢复并记录；3) Windows / macOS / Linux 展示对应系统操作指引；Android 仅展示提示、不做切换；4) `npm run test:browser -- --project=app` 通过 | IPV6-05 | 单栈下 IPv4-only 资源不可达；一旦误留单栈，用户会以为"断网"。必须限时 + 一键恢复 + UI 明确警示 |

## 5. 实施顺序与分批

每批内**按 ID 顺序逐项提交、逐项验收**，一项的验收命令全绿后再开始下一项。

| 批次 | 内容 | 说明 |
| --- | --- | --- |
| 第一批（P0 + 低风险 P1） | P0-01 → P0-02 → P0-03 → P0-04 → P1-01 → P1-04 → P1-05 | 全部不改网络行为语义（P0-02 只调预算分层）。P1-01 的 golden 测试为后续 P1-02/P1-03 铺底。 |
| 第二批 | P1-02 → P1-03 → P1-06 → P2-02 → P2-03 → P2-01 | 先补测试与 CI 覆盖，再做 TLS 后端替换（唯一需要实机复测 Windows 的项）。 |
| 第三批（IPv6 只读） | IPV6-01 → IPV6-02 → IPV6-03 | 只读与配置层，零网络行为改动；IPV6-01 不通过则本批及第四批整体取消。 |
| 第四批（IPv6 行为） | IPV6-04 → IPV6-05 → IPV6-06 | 网络行为改动靠后；每项都必须带"关闭即回到当前行为"的回滚验证。 |

补充要求：

- 每批完成后在 `docs/` 下新增一份 `verification-<日期>.md` 风格的验证记录（沿用 `docs/verification-2026-10-26.md` 的表格格式：改动范围 / 实际验证 / 尚需实机验收）。
- 涉及用户可见开关的项（P1-02、P1-03、IPV6-02、IPV6-06）必须同时更新 `src/ui-copy/settings.ts` 与 `index.html` 回退文案，否则 P1-05 的守卫会失败。
- 版本号与发布动作不在本清单内，除非另行确认。

## 6. 待确认问题（仅列影响实施的）

1. **分支与提交策略**：是直接在 `main` 逐项提交，还是每项一个分支 + PR？本地无 Rust 工具链，只有走 PR 才能让 GitHub CI 承担 Rust 验证。
2. **验证门槛**：是否以「GitHub CI 四个 job 全绿」作为唯一验收依据？是否需要在本机安装 Rust 工具链以便提交前自查（`cargo fmt`/`clippy` 反馈更快）？
3. **IPv6 单栈平台范围**：是否接受「Windows / macOS / Linux 仅提示 + 指引，Android 仅提示」这一保守范围（即 App 不直接改系统 IP 栈）？
4. **IPv6 前提确认的责任与时间**：谁在什么时间窗口用真实账号完成「开/关 IPv6 出口的计费增量对比」？在结论出来前，IPV6-02 起的任务是否冻结？
5. **端点覆盖配置的入口形态**：只做高级设置里的表单，还是仅允许配置文件手改？影响 P1-02 的前端工作量。
6. **宿舍证书 pin 例外的可配置边界**：是否允许用户在 UI 中关闭 pin 例外或替换指纹？若不允许，P1-03 只做代码内可维护化（不发版不可改），需确认是否仍有价值。
7. **IPv6 保活的耗电阈值**：Android 后台 30 分钟增幅的验收阈值定为多少（例如 ≤5%）？

## 7. 非目标（Out of scope）

- 不实现任何绕过计费、篡改计量字段、伪造认证报文的能力；不修改 `PortalRouteContext` 对 VPN/TUN Fake-IP 的拒绝逻辑。
- 不做全仓 `.unwrap()` 清理（当前 `lock()`/`read()`/`write()` 共 188 处）——只处理后台关键路径，其余不动。
- 不做无关重构：不拆模块、不改目录结构、不重命名既有命令、不调整前端视觉与交互风格。
- 不做依赖大版本升级（Tauri 2 → 3、reqwest 0.12 → 0.13、Vite 6 → 7 等），不做覆盖率平台接入、性能基准、遥测上报。
- 不新增运行时依赖（含 HTTP 客户端、日志框架、配置框架）；不引入新的抽象层或通用插件机制。
- 不做 OpenWrt 路由端改动（独立仓库 `BJUT-Auto-Login-OpenWrt`），不改 `release.yml` 发布矩阵与签名流程。
- 不做 IPv6 单栈的自动系统网络配置（含 Android rooted 场景的自动切换）。
- 不改动认证协议实现本身（`drcom` / `ePortal` 报文结构与加密算法），除 P0 明确列出的健壮性修正。
- 不承诺在无真实校园网环境的机器上完成 IPv6 行为类验收——此类项必须标注"尚需实机验收"。
