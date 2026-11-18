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
        messages.push('认证会话未确认：可先查询当前账号状态，避免连续重复提交登录。');
    if (failed('internet'))
        messages.push('请分别对照当前网络与网卡直连结果：当前网络通过、直连失败时，检查 Wi-Fi 待认证及 VPN 是否改走其他上游。');
    if (report.dualStack?.ipv6.status === 'not_configured')
        messages.push('当前网卡未取得可用 IPv6 地址，请检查上游分配；登录成功不会自动提供 IPv6 前缀。');
    if (!messages.length && report.overall !== 'healthy')
        messages.push('请对照已完成的模块检查接口、认证和路由，可用“链路检查”进一步定位。');
    return messages;
}
