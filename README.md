# BJUT Auto Login

北京工业大学校园网连接助手，支持 **bjut_wifi、bjut-sushe 和有线 lgn**。提供 Windows、macOS、Linux 与 Android 客户端。

[项目网站](https://al.bjutdown.work) · [下载安装](https://github.com/key-zhzr/BJUT-Auto-Login/releases) · [使用指南 / Wiki](https://github.com/key-zhzr/BJUT-Auto-Login/wiki) · [反馈问题](https://github.com/key-zhzr/BJUT-Auto-Login/issues)

## 能做什么

- 自动识别校园网、登录与重连，支持多账号排序和独立网络档案。
- 选择认证网卡，查看 IPv4 / IPv6 状态，诊断网络与 VPN 兼容问题。
- 查询校园网余额、流量、账单和在线设备，办理计费服务与网费充值。
- 账号在本机安全保存，支持加密备份与跨设备迁移。
- 三种主题、托盘操作、后台检测、用量提醒与应用更新。

## 开始使用

1. 从 [Releases](https://github.com/key-zhzr/BJUT-Auto-Login/releases) 安装对应平台的客户端。
2. 连接校园网，在“账号管理”添加账号。
3. 手动登录确认可用后，根据需要开启自动登录。

详细安装、VPN 设置、Android 后台运行、计费和备份说明已迁至 [Wiki](https://github.com/key-zhzr/BJUT-Auto-Login/wiki)。网站也提供[使用指南](https://al.bjutdown.work/guide/Getting-Started/)。功能以当前发行版本为准。

## 开发与贡献

基于 Tauri 2、Rust 和 TypeScript。参见 [开发指南](https://github.com/key-zhzr/BJUT-Auto-Login/wiki/Development) 和 [发布说明](RELEASING.md)。欢迎提交 Issue 或 Pull Request。OpenWrt 路由端正在整理为独立项目，详见 [路由端说明](https://github.com/key-zhzr/BJUT-Auto-Login/wiki/OpenWrt)。

本项目是**开源第三方工具，非学校官方应用**。使用明文 HTTP 兼容模式前，请确认网络可信；详见[隐私与安全](https://github.com/key-zhzr/BJUT-Auto-Login/wiki/Privacy)。

[MIT License](LICENSE)
