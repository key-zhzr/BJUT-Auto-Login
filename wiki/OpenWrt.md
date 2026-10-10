# OpenWrt 路由端

路由端已在 [BJUT-Auto-Login-OpenWrt](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt) 独立维护，当前源码版本为 0.3.0。

路由端提供自动认证、多个账号顺序尝试、LuCI 设置、网络变化触发和接口绑定。运行时使用 OpenWrt 自带的 shell 与轻量辅助程序，不依赖桌面图形界面。

仓库包含 `bjut-autologin` 本体和 `luci-app-bjut-autologin` 管理界面两个包，本体可单独使用。请按路由器固件版本和架构选择 SDK 编译、安装。

## 与主项目同步

独立仓库保留主项目的 Git 历史，通过每日工作流读取主项目的 `main`，也可手动运行。无冲突且检查通过的普通改动自动合并；认证协议、安全策略等改动保留为待审查分支。仓库允许 Actions 创建 PR 时自动创建，否则从任务摘要打开差异链接继续审查。

桌面 Rust 与路由 shell 不是同一种实现，不能保证任意新功能都自动移植。工作流会保留冲突并明确指出相关文件，需要人工处理时不会覆盖路由端改动。

详见[安装与配置](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/README.md)、[测试说明](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/TESTING.md)和[上游同步说明](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/UPSTREAM.md)。路由端不包含桌面计费和充值界面；离线检查通过不代表已经完成特定固件的 SDK 编译和校园网实测。
