# OpenWrt 路由端

路由端已在 [BJUT-Auto-Login-OpenWrt](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt) 独立维护，与 App 统一使用主版本号 `0.1.6`。当前源码修订为 `0.1.6-r27`，已发布安装包以独立仓库 Releases 为准。

路由端提供多上游自动认证、IPv4/IPv6 独立接口选择、按上游分配账号、LuCI 设置与计费查询。门户会话和外网探测分开判断，兼容 mwan3 的指定出口。运行时使用 OpenWrt 自带的 shell 与轻量辅助程序，不依赖桌面图形界面。

## 下载与架构矩阵

仓库包含 `bjut-autologin` 本体和 `luci-app-bjut-autologin` 管理界面两个配套包，本体可单独使用。官方 SDK 编译矩阵固定 **OpenWrt 25.12.5 → APK**、**OpenWrt 24.10.8 → IPK**（此处的 APK 为 OpenWrt 新版包管理器格式，不是 Android 应用）。

| Target 平台 | 包架构 (`DISTRIB_ARCH`) | 适用格式 | 常见设备类型 |
| --- | --- | --- | --- |
| `x86/64` | `x86_64` | 25.12.5 APK / 24.10.8 IPK | x86 软路由 |
| `mediatek/filogic` | `aarch64_cortex-a53` | 25.12.5 APK / 24.10.8 IPK | MT7981 / MT7986 / MT7988 路由器 |
| `rockchip/armv8` | `aarch64_generic` | 25.12.5 APK / 24.10.8 IPK | Rockchip ARM64 设备 |
| `bcm27xx/bcm2711` | `aarch64_cortex-a72` | 25.12.5 APK / 24.10.8 IPK | Raspberry Pi 4 |
| `qualcommax/ipq60xx` | `aarch64_cortex-a53` | 仅 25.12.5 APK（24.10 无官方 SDK） | 连我 NN6000 v2 / IPQ6000 |
| `ipq40xx/generic` | `arm_cortex-a7_neon-vfpv4` | 25.12.5 APK / 24.10.8 IPK | IPQ40xx 路由器 |
| `mvebu/cortexa9` | `arm_cortex-a9_vfpv3-d16` | 25.12.5 APK / 24.10.8 IPK | Marvell Armada 路由器 |
| `ipq806x/generic` | `arm_cortex-a15_neon-vfpv4` | 25.12.5 APK / 24.10.8 IPK | IPQ806x 路由器 |
| `ramips/mt7621` | `mipsel_24kc` | 25.12.5 APK / 24.10.8 IPK | MT7621 路由器 |
| `ath79/generic` | `mips_24kc` | 25.12.5 APK / 24.10.8 IPK | 常见 Atheros MIPS 路由器 |

在 [OpenWrt packages 工作流](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/actions/workflows/openwrt-packages.yml)中可选择平台编译 IPK/APK，完成后下载任务附件；全部格式构建先完成 APK 再开始 IPK，版本标签构建成功后连同 `builds.json` 与 `SHA256SUMS` 一起上传至 Releases。请按路由器固件版本、平台和架构选择安装包，详见[下载与编译](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/BUILDING.md)。

安装前可在路由器上核对目标平台、包架构与文件校验和：

```sh
ubus call system board
opkg print-architecture || apk --print-arch
sha256sum -c SHA256SUMS
```

## App 与路由协作

App 自动查找当前网络实际取得的局域网网关，发现 BJUT 路由端后在控制台连接。未找到时使用“设置 → 路由协作 → 手动连接”。默认使用 HTTPS，首次连接自签证书时核对指纹；可信局域网也可勾选“允许局域网 HTTP”并填写私有 IP，HTTP 会明文传输管理密码和同步内容。

连接后，控制台显示各端口的链路、双栈、实际账号与收发流量；计费中心可选择路由账号查询，也可切回当前设备使用完整计费服务。账号管理同时显示本机和路由账号，可直接添加、编辑、排序和删除路由账号，停用只影响自动登录；诊断附加路由上游结果，日志页显示路由日志，设置页可编辑路由器服务和 IPv4/IPv6 上游，协作操作无需跳转 Web 后台。勾选“记住路由密码”后，App 一并记住当前 Wi-Fi 名称、BSSID 或有线网关及路由器标识，仅在匹配的网络中自动重连；升级前保存的密码需手动连接一次以绑定网络。未勾选则仅保留本次运行的连接信息。手动“停止协作”会暂停重连，设置中可移除保存的密码。密码不随配置备份导出。

LuCI 提供控制台、账号管理、计费系统、网络诊断、运行日志与设置。前五页采用 App 风格，设置保留 LuCI 表单。计费系统打开后自动查询各条上游，优先使用当前认证账号的免密跳转，无法使用时按上游账号分配回退。同一账号的余额、总流量和剩余流量只汇总一次，也可选择单条上游和已保存账号，直接填写图片验证码。提供概览、用量与账单、账户服务、设备与安全，不含充值。可办理报停与复通、预约或取消套餐、设置消费保护、下线设备、管理 MAC 和无感认证，以及修改计费密码与密码保护。记录查询自动拆分较长日期范围，支持分页与本页 CSV 导出。账号编号完整显示，添加、编辑、排序和删除统一放在账号管理。会话按管理员、上游、账号与出口隔离，概览读取完成后先显示。

多上游通过 `uplink` 段配置，例如 `wan14/wan16`、`wan24/wan26`；IPv6 可使用独立逻辑接口。LuCI 通常每 2 秒刷新，检测时每秒刷新；操作控件聚焦不会暂停更新，临时请求失败后会自动恢复。可以按端口登录、注销和切换账号；手动注销会暂停该上游自动登录，重新登录后恢复。默认维护绑定设备的内存出口路由和专用 mwan3 源地址规则，保留普通 LAN 转发及用户策略。没有默认网关证据时会提示，不猜测网关或反复重启健康接口。详见独立仓库的[完整配置说明](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/README.md)。

日志默认写入系统日志，可在设置中改为自定义目录的文件日志，并选择通过网页地址读取；文件按大小轮换。信息级记录检测、会话和登录结果，调试与详细级用于排查。新版界面的展开、收起及进度变化保持统一动画，移动端滚动条也不会改变内容居中位置。

## 协作功能与过期版本说明

安装时优先选择独立仓库 Releases 中与固件匹配的最新正式构建。当前源码为 **`0.1.6-r27`**，源码修订不等于已经发布安装包；历史版本说明如下：

- **早期独立编号 `0.3.0`（已过期）**：路由端已从早期 `0.3.0` 统一改为与 App 同步的 `0.1.6`。旧 `0.3.0` IPK 设备在核对平台与 SHA-256 后，可执行 `opkg --force-downgrade install` 保留原配置迁移到 `0.1.6`。
- **`0.1.6-r12` 之前版本（界面已过期）**：仅支持早期单页状态查看与基础账号同步，不包含拆分的六页 App 风格 LuCI 界面及修复后的计费共享组件。
- **`0.1.6-r10` 与 `0.1.6-r14`（已废弃的中间修订）**：`r10` 因多上游路由锁竞争未通过验证、`r14` 在实机验收时发现独立维护进程 JSON 初始化与免密回退边界问题，均未作为正式版本发布，请勿安装。
- **`0.1.6-r15` 至 `0.1.6-r18` 历史改进**：
  - `r15`：路由结果标明时效，短暂断开时保留上次数据；支持任务状态合并、多上游账号并发上限、后台计费查询与 PushPlus 用量提醒，以及匹配平台与 SHA-256 的“路由器备份与更新 / 回退”（路由备份含账号密码，保存在私有目录最多 8 份；App 本机设置和账号继续使用[加密导入/导出](Backup#导出)）。
  - `r16`：修复浅色模式登录文字对比度；LuCI 与 App 均可按接口独立调整账号顺序，流量耗尽或停机等明确拒绝会自动轮换下一账号。
  - `r18`：计费中心合并为单次认证、逐模块渐进返回结果，复用同账号同物理出口的已验证会话；补齐当前套餐与周期标识，禁止重复预约当前激活套餐，并修复 Argon 等主题下套餐卡片高度受限问题。

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

账户服务和设备管理的共享界面会在上游合并时重新生成并验证，相关修改仍需审查。桌面 Rust 与路由 shell 不是同一种实现，不能保证任意新功能都自动移植。工作流会保留冲突并明确指出相关文件，需要人工处理时不会覆盖路由端改动。

详见[安装与配置](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/README.md)、[测试说明](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/TESTING.md)和[上游同步说明](https://github.com/key-zhzr/BJUT-Auto-Login-OpenWrt/blob/main/openwrt/UPSTREAM.md)。路由端提供 LuCI 计费查询，充值等完整校园服务继续使用 App；SDK 编译通过不代表已经完成各设备的安装和校园网实测。


## 流量统计与维护

流量统计默认关闭，需手动开启；已手动开启的设备升级后保持原设置。每分钟从 jfself 读取在线设备的下载计费量，按上游接口或全部在线设备记录真实采样区间。图表支持分钟至月的尺度、滚轮缩放、自选起止时间、新增量、范围累计、当月累计、接口多曲线和 CSV 导出。控制台的“查看范围”只过滤显示，实际统计范围在设置中修改，启停需确认。

分钟明细默认保留 48 小时，小时汇总 90 天，每日汇总 366 天；保留时间和存储上限均可在设置中调整，并查看预计占用。路由器上游用量包含经该出口转发的流量，无法据此拆分 NAT 后的各台局域网设备。历史每 15 分钟保存一次，正常停止服务时立即保存，突然断电可能丢失尚未保存的区间。

各账号并发准备计费会话，到统一时点读取流量。换号后立即查询新账号并标记位置；普通设置修改保留有效基线，查询出口失败时尝试其他可用上游。首次、长时间中断或计数回退不补算。

r22 修复了通过 LuCI 或 App 保存统计开关、范围等路由设置时出现“设置项无效”的问题。已手动开启并保存范围的设备保持开启。

可选余量优先只使用三分钟内成功取得的数据；上游单独指定的账号顺序仍优先。IPv6 单栈 lgn 仍需校园地址、指定接口和正常 TLS/门户验证，前缀委派只展示 netifd 实际收到的结果，认证不会保证获得 PD。

PushPlus 测试状态区分排队、请求失败及平台已受理，并显示 HTTP/业务码。平台受理不等于微信送达；界面不会将 code=200 当作最终送达证明。测试消息和故障恢复仍遵循去重与冷却规则。
