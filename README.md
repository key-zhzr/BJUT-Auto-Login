# BJUT-Auto-Login (BJUT-AL)

**BJUT-Auto-Login (BJUT-AL)** 是一款专为北京工业大学（BJUT）校园网（`bjut_wifi` / `bjut-sushe`/有线`lgn`）设计的现代化、跨平台自动登录客户端。基于最新的 **Tauri V2** 构建，支持 Windows、macOS、Linux 以及 Android 平台，拥有极佳的性能、轻巧的体积以及全平台的沉浸式原生体验。

---

## ✨ 核心特性

- 全平台支持：支持 Windows、macOS、Linux 桌面端， Android、iOS/iPadOS 移动端（iOS/iPadOS由快捷指令实现），以及 Openwrt 路由端。
- 无感自动登录：系统网络变化事件会立即触发检测，并以低成本局域网 IP 检测兜底；Android 界面进程退出后可由前台服务直接调用 Rust 核心继续检测，并通过开机广播与低频看门狗进行恢复。
- 智能环境感知：内置智能算法，精准识别当前是否处于校园网环境内，防止在非校园网环境下误触登录。
- 网络安全提示：结合 SSID、BSSID、本地网段与黑白名单降低在错误网络发送凭据的风险；无法取得网络身份时，手动登录会默认阻止发送。
- 多账号管理：支持保存多个校园网账号，可通过直观的拖拽交互调整登录优先级顺序，点击账号头像可切换启用/禁用。
- 网络配置档案：不同 SSID 或校园有线环境可分别绑定账号、认证协议、自动登录与前后台检测间隔。
- 账号保护与诊断：提供分阶段网络诊断、账号失败熔断、安全存储健康检查和脱敏诊断包。
- 校园网计费中心：直接查看余额、流量、在线会话和各类账单，支持完整 CSV 导出，并可在二次确认后办理停复机、套餐预约、消费保护、设备绑定、统一认证密码修改及校园卡网费充值。
- 用量提醒：余额或套餐流量低于自定义提醒线时发送系统通知，每类提醒每天最多一次。
- 桌面快捷操作：托盘显示联网状态，可立即检测、登录、切换首选账号或暂停自动登录一小时。
- 权限健康中心：集中检查通知、Wi-Fi、位置、后台运行、安全存储与安装更新权限，并提供修复入口。
- VPN 共存：提供“系统 DNS / 校园网 DNS / 固定地址 / HTTP + IP”四档策略，默认保留 HTTPS 并固定已知校园网网关地址。
- 自动更新：读取 GitHub Releases 并展示 Markdown 更新说明；桌面更新由 Tauri Updater 验证 minisign 签名，Android 更新由系统验证 APK 签名。
- 可追溯日志：保留最近 5 次启动的完整落盘日志，界面最多展示 5000 条并支持搜索、筛选和快速滚动；导出不受界面条数限制。

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
   - **VPN 共存兼容等级**：推荐“高兼容（HTTPS + 固定地址）”；仅在其他模式均不可用时选择会明文传输账密的“最高兼容（HTTP + IP）”。

---

## 🛠️ 技术栈与开发指南

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

正式发布前请按 [RELEASING.md](RELEASING.md) 配置 Tauri 更新私钥和 Android 签名 Secret。

---

## 🔐 隐私与安全说明

本应用的账号密码仅保存在设备的安全凭据存储中：macOS 使用应用私有目录内、权限限制为当前用户的 AES-GCM 加密文件，Windows 使用 Credential Manager，Linux 使用 Secret Service，Android 使用 Android Keystore；常规 `config.json` 不包含密码。导出配置时会要求你设置独立密码并采用 AES-GCM 加密。应用不会向第三方服务器上传校园网密码，认证请求仅发送至北京工业大学校园网认证网关、计费系统 `jfself.bjut.edu.cn`、统一认证 `cas.bjut.edu.cn` / `uc.bjut.edu.cn`，以及经 `itsapp.bjut.edu.cn` 进入的移动门户 `ydapp.bjut.edu.cn`。

计费中心与统一认证操作均使用只存在于内存中的临时 HTTPS 会话；`jfself` 操作完成后主动注销，其余会话在操作结束后直接销毁。密码、密码保护答案、Cookie、`openid`、安全令牌和充值订单号不会写入运行日志。校园卡充值必须先核对付款卡、目标账户和金额，再由用户二次确认；若订单或扣费结果不明确，App 不会自动重试。账单 CSV 可能包含 IP、MAC 和使用记录等个人网络信息，请妥善保管导出文件。

默认与前三档 VPN 兼容策略均使用 HTTPS；“最高兼容（HTTP + IP）”会通过明文 HTTP 向固定校园网网关发送认证信息，只应在其他模式均不可用且确认处于可信校园网络时启用。网络身份判断只能降低误发风险，无法替代 TLS 的密码学保护，请同时使用黑白名单限制未知网络。

---

## 🤝 贡献与反馈

如果您在使用过程中遇到了任何 Bug，或者有新的功能需求，欢迎提交 [Issue](https://github.com/key-zhzr/BJUT-Auto-Login/issues) 或直接发起 Pull Request。
由于本人不在校内，本项目暂时停止更新（修复Bug除外），9月份继续更新。

## 📄 许可证

本项目基于 [MIT License](LICENSE) 开源。
