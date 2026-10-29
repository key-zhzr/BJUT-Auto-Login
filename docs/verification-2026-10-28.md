# 第一批改进与验证记录

> 分支：`feat/robustness-ipv6`
> 基线：`main` @ `a012353`
> 第一批 HEAD：`b0e6ba2`
> 范围：P0-01、P0-02、P0-03、P0-04、P1-01、P1-04、P1-05
> 原则：保持现有功能与兼容性；每项独立提交、独立回滚；不新增运行时依赖。

## 改动范围

| 项 | 提交 | 改动 |
| --- | --- | --- |
| P0-01 锁中毒收敛 | `65d7e63` | `connectivity.rs` 7 处、`lib.rs` 8 行锁获取改为 `unwrap_or_else(PoisonError::into_inner)`；新增 `poisoned_lock_does_not_panic` |
| P0-02 超时预算分层 | `57f8671` | `portal_auth.rs` 新增 `LGN_IPV6_DISCOVERY_BUDGET` / `LGN_IPV6_ATTEMPT_RESERVE` / `attempt_timeout_for`；内层探测 5s→4s，外层仍 5s；新增预算断言 |
| P0-03 歧义认证测试 | `ea43f00` | 纯测试：`gateway_stub` + 2 个用例，证明不可解析响应只提交一次、不切换协议 |
| P0-04 错误分支测试 | `3a4e376` | 纯测试：超时、拒连、TLS 握手、不可解析 JSONP、空响应、HTTP 503 共 6 个用例 |
| P1-01 端点单一来源 | `80b1109` | 新增 `portal_auth/endpoints.rs`；`portal_auth.rs`、`dorm_tls.rs`、`lib.rs`、`macos_network.rs` 接线；新增 2 个 golden 测试 |
| P1-04 测试入口守卫 | `943197f` | 新增 `scripts/check-test-coverage.mjs`、`npm run check:test-coverage`、CI 步骤 |
| P1-05 首帧文案守卫 | `b0e6ba2` | 新增 `scripts/test-ui-copy-fallback.mjs` 并加入 `test:frontend` |

## 实际验证

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过（退出码 0） |
| `cargo test poisoned_lock_does_not_panic` | 1 passed |
| `cargo test portal_auth` | 41 passed / 0 failed |
| `cargo test endpoint_constants_are_the_captured_gateway_values` | 1 passed |
| `cargo test url_builders_start_from_the_shared_endpoint_constants` | 1 passed |
| `cargo test`（全量） | 262 passed / 2 failed / 4 ignored |
| `cargo clippy --all-targets -- -D warnings` | 10 个既有错误，零新增（错误集合与基线逐条一致） |
| `npm run test:frontend` | 34 passed / 0 failed（6 个脚本） |
| `npm run check:test-coverage` | 通过：6 个 `scripts/test-*.mjs` 全部注册 |
| `npm run typecheck` | 通过 |
| `npm run build` | 通过（仅既有 chunk 体积警告） |
| 生产段端点字面量 | `portal_auth.rs` 测试段之前无 `10.21.*` / `172.30.*` / `wlgn.*` / `lgn.*` URL 字面量 |
| 既有断言改动 | `git diff main..HEAD -- src-tauri/src/portal_auth.rs \| grep "^-.*assert"` = 0 |

## 已知基线问题（非本批引入）

- `tests::request_errors_never_include_credential_urls`：本机回环 502 响应导致断言失败，改动前后一致。
- `router_control::tests::explicitly_pinned_router_certificate_still_requires_tls_key_proof`：本机证书生成/校验环境导致失败，改动前后一致。
- `clippy -D warnings` 的 10 个既有错误：`lib.rs` 464/684-689/1310、`probe_transport.rs` 452/454、`dual_stack.rs` 72（dead_code）。当前 Rust 1.99 下基线即红，本批未新增。

## CI 状态

该 fork 的 GitHub Actions 当前显示 **0 次运行**，工作流尚未启用。因此本批以本地 Rust stable 1.99 + VS BuildTools 2026 的 `fmt`/`clippy`/`test` 与前端测试作为门禁；启用 Actions 后应再跑一次四平台 CI 复核。

## 尚需实机验收

- P0-02：在双栈校园网中触发一次 lgn IPv6 不可达，确认日志保留逐地址失败明细，且登录总耗时不劣于改动前。
- P1-01：在真实网关下确认三种协议登录/注销/探测 URL 与改动前逐字一致（golden 测试已覆盖常量层）。
- IPv6 免流行为改动不在本批范围，尚未实现。

## 回滚

每项均可独立回滚：

```bash
git revert 65d7e63  # P0-01
git revert 57f8671  # P0-02
git revert ea43f00  # P0-03
git revert 3a4e376  # P0-04
git revert 80b1109  # P1-01
git revert 943197f  # P1-04
git revert b0e6ba2  # P1-05
```

## 补充：P1-06 / P2-02（2026-10-29）

| 项 | 提交 | 验证 |
| --- | --- | --- |
| P1-06 平台 CI 子集 | `23f5774` | 在 `desktop-platform-check` 增加 `portal_auth` 与 `dual_stack` 测试；因 Actions 未启用，尚未实际执行 |
| P2-02 `md5` 升级 | `652a2d9` | 0.7.0 → 0.8.1；`cargo test campus_services` 16 passed / 1 ignored；全量仍 262 passed / 2 failed / 4 ignored；clippy 仍为同一组 10 个既有错误；`md5::compute` 调用点仍为 5 |

P2-02 的 `cargo update` 通过当前代理完成，锁文件仅变更 `md5` 版本与校验和。
