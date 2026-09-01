# UI 文案修改位置

前端文案按生成来源分为三处：

1. `src/ui-text.ts`
   - 已集中迁移、经常需要修改的 TypeScript 动态标题、状态和说明。
   - 例如控制台网络状态、账号发现卡片、Wi-Fi 黑白名单说明。
2. `index.html`
   - 页面初始可见的静态标题、按钮、表单标签、占位文字和固定说明。
   - 搜索页面上看到的原文即可定位。
3. `src/main.ts` 与 `src/*.ts`
   - 尚未迁移或紧邻具体交互逻辑的动态提示、弹窗文字和操作结果。
   - 可直接全文搜索界面上显示的原文；新文案优先放入 `src/ui-text.ts`。
4. `src-tauri/src/`
   - Rust 返回的网络协议错误、安全提示、运行日志和计费结果。
   - 常见文件：`portal_auth.rs`（校园网认证）、`billing.rs`（计费中心）、
     `campus_services.rs`（校园卡和支付）、`lib.rs`（应用编排与平台提示）。

样式文字（例如 CSS 伪元素的 `content`）位于 `src/styles.css`，但目前只用于少量装饰。

修改后建议至少运行：

```text
npm run build
```

若修改 Rust 文案或协议逻辑，还应运行项目 CI 中的 Rust 格式、Clippy 和测试步骤。
