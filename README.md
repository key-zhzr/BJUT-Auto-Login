# BJUT-Auto-Login (BJUT-AL)

**BJUT-Auto-Login (BJUT-AL)** 是一款专为北京工业大学（BJUT）校园网（`bjut_wifi` / `bjut-sushe`/有线`lgn`）设计的现代化、跨平台自动登录客户端。基于最新的 **Tauri V2** 构建，支持 Windows、macOS、Linux 以及 Android 平台，拥有极佳的性能、轻巧的体积以及全平台的沉浸式原生体验。

---

## ✨ 核心特性

- 全平台支持：支持 Windows、macOS、Linux 桌面端， Android、iOS/iPadOS 移动端（iOS/iPadOS由快捷指令实现），以及Openwrt 路由端。
- 无感自动登录：后台智能轮询检测网络状态，断网秒级恢复，开机自启/后台驻留，一次配置，永久无感上网。
- 智能环境感知：内置智能算法，精准识别当前是否处于校园网环境内，防止在非校园网环境下误触登录。
- 网络安全提示：结合 SSID、BSSID、本地网段与黑白名单降低在错误网络发送凭据的风险；无法取得网络身份时，手动登录会默认阻止发送。
- 多账号管理：支持保存多个校园网账号，可通过直观的拖拽交互调整登录优先级顺序，点击账号头像可切换启用/禁用。
- 自动更新：直接读取 GitHub Releases，在应用内展示 Markdown 更新说明，并自动下载当前系统与架构对应的安装包后交由系统安装。
- 可追溯日志：保留最近 5 次启动的运行日志，支持一键复制与快速滚动到底部。

---

## 🚀 快速使用

### 📥 下载安装

前往本仓库的 [Releases 页面](https://github.com/key-zhzr/BJUT-Auto-Login/releases) 下载适合您对应平台的安装包：

- **Windows**: `.exe`（NSIS）
- **macOS**: `.dmg` (支持 Apple Silicon 与 Intel)
- **Linux**: `.deb` / `.AppImage`
- **Android**: `.apk` (提供 `arm64-v8a`、`x86_64` 架构)

### ⚙️ 核心配置

1. **添加账号**：在主界面点击“账号管理”添加你的校园网学号与密码，添加多账号时支持拖拽排序。
2. **连接检测**：点击界面上的登录按钮，或由应用在后台自动进行登录。
3. **设置项**：
   - **开机自启动**：建议开启，开机后自动潜伏后台保活网络。
   - **重试间隔/超时**：支持自定义检测频率与超时时间，兼顾省电与断网恢复速度。

---

## 🛠️ 技术栈与开发指南

本项目采用 **Tauri V2** 框架，使用最纯粹的 Vanilla HTML + CSS + TypeScript 构建，兼顾轻量与美观。

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

---

## 🔐 隐私与安全说明

本应用的账号密码仅保存在设备的系统安全存储中：桌面端使用系统钥匙串，Android 使用 Android Keystore 加密存储；常规 `config.json` 不包含密码。导出配置时会要求你设置独立密码，采用 AES-GCM 加密。应用不会向第三方服务器上传校园网密码，认证请求仅发送至北京工业大学校园网认证网关。

部分校内认证协议（Type1 / Type2）由网关限定为明文 HTTP，网络身份判断只能降低误发风险，无法提供密码学意义上的防伪保证。请仅在可信的校园网络环境中启用自动登录，并通过黑白名单限制未知网络。

---

## 🤝 贡献与反馈

如果您在使用过程中遇到了任何 Bug，或者有新的功能需求（例如适配新的校园网登录系统或网段更新），欢迎提交 [Issue](https://github.com/key-zhzr/BJUT-Auto-Login/issues) 或直接发起 Pull Request。

## 📄 许可证

本项目基于 [MIT License](LICENSE) 开源。
