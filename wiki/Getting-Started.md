# 快速开始

## 安装

从 [项目官网下载区](https://al.bjutdown.work/#download) 或 [GitHub Releases](https://github.com/key-zhzr/BJUT-Auto-Login/releases) 选择适合设备的安装包。官网稳定版跟随 GitHub Latest Release，预发布版（Pre-release）在独立频道列出；网络受限时仍保留已验证的安装包直链与离线回退。

| 平台 | 安装包 |
| --- | --- |
| Windows | EXE 安装程序（提供标准包与内置 WebView2 的 Offline 离线安装包） |
| macOS | DMG；按 Apple Silicon (ARM64) 或 Intel (x64) 选择 |
| Linux | DEB 或 AppImage (x64) |
| Android | APK；普通手机选 ARM64 (`arm64-v8a`)，模拟器或 Intel 设备选 `x86_64`（详见 [Android 架构说明](Android#安装包与架构选择)） |
| OpenWrt | 路由端 `25.12.5 APK` / `24.10.8 IPK`（详见 [OpenWrt 路由端](OpenWrt#下载与架构矩阵)） |

不要从不明来源下载修改版安装包。应用内可检查更新，也可重新下载当前版本完整包。

Windows 上重复打开应用会唤起已运行的窗口；最小化或关闭到托盘后也可用此方式找回窗口。

## 第一次连接

1. 连接 `bjut_wifi`、`bjut-sushe` 或校园有线网络。
2. 在“账号管理”添加校园网账号和密码。
3. 在控制台点击“登录”，等待认证反馈。
4. 确认可用后，在设置中开启自动登录。

多账号可以拖动排序。禁用账号只会将它移出自动登录顺序，手动登录和计费系统仍可选择。

## 常用设置

- **认证网卡**：自动模式优先校园有线；也可在设置中直接点击指定网卡。指定网卡断开后会等待恢复。
- **VPN 兼容性**：建议先使用默认的高兼容。需要调整时参见[网络与 VPN](Network#vpn-兼容策略)。
- **检测间隔**：设置前后台基础间隔；自适应检测会在网络稳定时降低频率。
- **外观**：Basic、Apple OS 27、WinUI 主题均支持明暗模式和强调色。

遇到问题时，先运行“网络诊断”，再根据结果处理。请不要公开账号密码或未经脱敏的个人网络记录。
