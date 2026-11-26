import type { DiagnosticReport } from './models';

export function diagnosticAdvice(report: DiagnosticReport): string[] {
    if (report.stale)
        return ['网络在诊断期间发生变化，请重新检测。'];
    const messages: string[] = [];
    const failed = (id: string) => report.steps.some(x => x.id === id && ['error', 'warning'].includes(x.status));
    if (report.overall === 'auth_required')
        messages.push('当前校园网络需要认证，可返回控制台登录。');
    if (failed('dns'))
        messages.push('DNS 解析异常：检查当前网卡 DNS，并在 VPN 中放行校园域名。');
    if (failed('campus_route'))
        messages.push('校园目标路由异常：在 VPN 中放行校园网段，并核对认证网卡。');
    if (failed('authentication_session'))
        messages.push('尚未确认校园网会话：建议先刷新账号状态，避免连续重复登录。');
    if (failed('internet'))
        messages.push('当前系统路由与网卡直连结果不一致：若系统已通但网卡直连失败，请检查 Wi-Fi 是否未登录或 VPN 接管了流量。');
    if (report.dualStack?.ipv6.status === 'not_configured')
        messages.push('当前网卡未获取到 IPv6 地址；若路由器或端口未下发 IPv6，仅完成网页登录不会获得 IPv6。');
    if (!messages.length && report.overall !== 'healthy')
        messages.push('请结合上方结果排查接口、认证与路由，或点“链路检查”进一步定位。');
    return messages;
}
