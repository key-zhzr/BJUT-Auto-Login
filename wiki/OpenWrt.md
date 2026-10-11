# OpenWrt 路由端

路由端已在 [BJUT-Auto-Login-OpenWrt](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt) 独立维护，与 App 统一使用源码版本 0.1.6。

路由端提供多上游自动认证、IPv4/IPv6 独立接口选择、按上游分配账号、LuCI 设置与计费查询。门户会话和外网探测分开判断，兼容 mwan3 的指定出口。运行时使用 OpenWrt 自带的 shell 与轻量辅助程序，不依赖桌面图形界面。

仓库包含 `bjut-autologin` 本体和 `luci-app-bjut-autologin` 管理界面两个包，本体可单独使用。在 [OpenWrt packages 工作流](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/actions/workflows/openwrt-packages.yml)中可选择平台编译 IPK/APK，完成后下载任务附件；全部格式构建先完成 APK 再开始 IPK，版本标签构建成功后上传至 Releases。请按路由器固件版本、平台和架构选择安装包，详见[下载与编译](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/BUILDING.md)。

## App 与路由协作

两端升级到 0.1.6 后，在 App 的“设置 → 路由协作”填写路由器 HTTPS 地址与管理账号。路由器需先启用 HTTPS 管理及 `/ubus`；地址可包含实际 HTTPS 端口。首次遇到自签证书时，App 会读取并显示指纹，确认这是你的路由器后继续，也可手动粘贴 PEM 证书。证书变化时需要重新核对。

连接后可以查看各上游的认证与 IPv4/IPv6 状态，选择上游和账号手动登录，并勾选 App 账号同步到路由端。同步仅更新选定账号，保留其他账号和接口分配；停用账号仍可手动使用。管理密码与登录令牌只留在 App 内存，两端保持独立运行。

LuCI 入口为“网络 → BJUT 自动登录 → 计费系统”，打开后自动查询各条上游，优先使用当前认证账号的免密跳转，无法使用时按上游账号分配回退。同一账号余额只汇总一次，也可选择单条上游和已保存账号，直接填写图片验证码。会话按管理员、上游、账号与出口隔离，概览读取完成后先显示。

多上游通过 `uplink` 段配置，例如 `wan14/wan16`、`wan24/wan26`；IPv6 可使用独立逻辑接口。LuCI 每 2 秒读取最新状态，同时显示检测步骤并保留最近完成的结果。默认维护绑定设备的内存出口路由和专用 mwan3 源地址规则，保留普通 LAN 转发及用户策略。没有默认网关证据时会提示，不猜测网关或反复重启健康接口。详见独立仓库的[完整配置说明](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/README.md)。

## 尚未开启 HTTPS

管理地址为 `192.168.1.1`、且 `uci -q get uhttpd.main.listen_https` 没有输出时，可在路由器上运行：

```sh
uci add_list uhttpd.main.listen_https='192.168.1.1:443'
uci commit uhttpd
/etc/init.d/uhttpd restart
```

保留现有证书与 `/ubus` 设置；TLS 组件和 `uhttpd-mod-ubus` 需由对应固件的软件源提供。此配置只监听所填 LAN 地址。

若已配置监听却仍被拒绝连接，检查证书和生成工具：

```sh
ls -l /etc/uhttpd.crt /etc/uhttpd.key /lib/libustream-ssl.so /usr/sbin/px5g /usr/bin/openssl
```

如果 TLS 库存在，但证书、私钥和两个生成工具均不存在，安装 `openssl-util` 后重启，uhttpd 会使用现有配置自动生成证书：

```sh
if command -v apk >/dev/null 2>&1; then
  apk update && apk add openssl-util
else
  opkg update && opkg install openssl-util
fi && /etc/init.d/uhttpd restart

ls -l /etc/uhttpd.crt /etc/uhttpd.key
```

只使用对应固件的软件源。重启时的 `4+0 records` 不代表证书已生成，`logread -e uhttpd` 也可能没有输出。已有证书和私钥时不要删除重建；恢复 HTTPS 后在 App 中核对证书指纹并连接。详细说明见[路由端连接指南](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/README.md#路由器尚未开启-https)。

## 与主项目同步

独立仓库保留主项目的 Git 历史，通过每日工作流读取主项目的 `main`，也可手动运行。无冲突且检查通过的普通改动自动合并；认证协议、安全策略等改动保留为待审查分支。任务摘要提供差异链接，确认后手动合并。

桌面 Rust 与路由 shell 不是同一种实现，不能保证任意新功能都自动移植。工作流会保留冲突并明确指出相关文件，需要人工处理时不会覆盖路由端改动。

详见[安装与配置](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/README.md)、[测试说明](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/TESTING.md)和[上游同步说明](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/UPSTREAM.md)。路由端提供 LuCI 计费查询，充值等完整校园服务继续使用 App；SDK 编译通过不代表已经完成各设备的安装和校园网实测。
