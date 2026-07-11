# 发布与签名

桌面端自动更新使用 Tauri Updater 的 minisign 签名。应用内置的公钥位于
`src-tauri/tauri.conf.json`，对应私钥不会进入 Git。

首次配置当前仓库时：

1. 将本机 `src-tauri/.keys/updater.key` 备份到密码管理器或离线介质。丢失私钥后，已安装版本将无法再信任新的更新。
2. 把私钥完整内容保存为 GitHub Actions Secret `TAURI_SIGNING_PRIVATE_KEY`。
3. 当前私钥无口令，`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 可留空；若以后轮换为有口令私钥，需要同时设置该 Secret。
4. Android APK 继续使用 `ANDROID_KEYSTORE_BASE64`、`ANDROID_KEYSTORE_PASSWORD`、`ANDROID_KEY_PASSWORD` 签名。Android 系统只允许相同证书签名的 APK 覆盖安装。

发布工作流会为 Windows、macOS、Linux 生成 Tauri 更新包及 `.sig` 文件，并在 Release 中生成 `latest.json`。应用会先验证该清单与安装包签名，再执行桌面端更新。Android 端从官方 GitHub Release 下载匹配架构的 APK，并由 Android 包管理器验证 APK 签名。

不要提交以下文件：

- `src-tauri/.keys/`
- Android keystore、导出的 Base64 keystore
- 任意签名密码或 GitHub Actions Secret
