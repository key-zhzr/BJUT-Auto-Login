# 开发与发布

本项目采用 **Tauri V2** 框架，使用 Vanilla HTML + CSS + TypeScript 构建，兼顾轻量与美观。

### 开发环境要求
- [Node.js](https://nodejs.org/) (建议最新 LTS)
- [Rust](https://www.rust-lang.org/) (用于 Tauri 后端)
- [Android Studio](https://developer.android.com/studio) (如果需要编译或调试 Android 端)

### 本地运行与编译

1. **克隆项目**
   ```bash
   git clone https://github.com/key-zhzr/BJUT-Auto-Login.git
   cd BJUT-Auto-Login
   ```

2. **安装依赖**
   ```bash
   npm install
   ```

3. **桌面端本地调试**
   ```bash
   npm run tauri dev
   ```

4. **Android 移动端本地调试**
   ```bash
   npm run tauri android dev
   ```

5. **编译发布版本**
   ```bash
   npm run tauri build           # 编译桌面端
   npm run tauri android build   # 编译安卓端
   ```

正式发布前请按 [发布指南](https://github.com/key-zhzr/BJUT-Auto-Login/blob/main/RELEASING.md) 配置 Tauri 更新私钥和 Android 签名 Secret。

## 检查改动

```sh
npm run build
npm run test:frontend
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

界面回归入口位于 `scripts/app-regression.html` 与 `scripts/dropdown-regression.html`，仅使用模拟账号和网络结果。Android 的键盘、相机、VPN 路由和后台保活仍需在真实设备验证。

## 网站与文档

网站源码在 `website/`，运行 `npm run site:build` 生成静态页面。域名为 `al.bjutdown.work`，由独立的 Cloudflare Worker 托管，不影响支付接力服务。Wiki 源文件在 `wiki/`，可独立发布；主仓库提交不会自动改变线上网站。
