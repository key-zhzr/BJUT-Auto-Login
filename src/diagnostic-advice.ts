import type { DiagnosticReport } from './models';

export function diagnosticAdvice(report: DiagnosticReport): string[] {
  if (report.stale) {
    return ['网络在诊断期间发生变化，请重新检测。'];
  }
  const messages: string[] = [];
  const failed = (id: string) =>
    report.steps.some(step => step.id === id && ['error', 'warning'].includes(step.status));
  if (report.overall === 'auth_required') {
    messages.push('当前校园网需要认证，可返回控制台完成登录。');
  }
  if (failed('dns')) {
    messages.push('DNS 解析异常：请检查当前网卡 DNS，并在 VPN 中放行校园域名。');
  }
  if (failed('campus_route')) {
    messages.push('校园网路由异常：请在 VPN 中放行校园网段，并核对当前认证网卡。');
  }
  if (failed('authentication_session')) {
    messages.push('暂未确认校园网会话：建议先刷新账号状态，避免连续重复登录。');
  }
  const connectivity = report.steps.find(step => step.id === 'internet')?.connectivity;
  if (
    connectivity?.system.some(result => result.status === 'reachable')
    && connectivity.direct.some(result => [
      'connection_error',
      'dns_error',
      'timeout',
      'tls_error',
      'response_error',
      'unknown',
      'unreachable',
    ].includes(result.status))
    && connectivity.direct.every(result => result.status !== 'reachable')
  ) {
    messages.push('系统路由与网卡直连结果不一致：若系统可通但网卡直连失败，请检查 Wi-Fi 是否未登录或 VPN 是否接管了流量。');
  }
  if (report.dualStack?.ipv6.status === 'not_configured') {
    messages.push('当前网卡未获取到 IPv6 地址；若路由器或网络端口未下发 IPv6，仅完成网页登录不会获得 IPv6。');
  }
  if (!messages.length && report.overall !== 'healthy') {
    messages.push('可点击上方各步骤右侧的信息图标查看具体原因，并排查网卡、认证与路由设置。');
  }
  return messages;
}
