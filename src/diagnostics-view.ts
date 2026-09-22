import type { DiagnosticReport, DiagnosticStep, DualStackReport } from './models';

export function renderDiagnosticReportView(report: DiagnosticReport, options: { busy: boolean; renderHealth: (health: DualStackReport) => void; formatTime: (time: string) => string }) {
  const adapterRepairPanel = document.getElementById('diagnostic-adapter-repair')!;
  const btnRestartLgnAdapter = document.getElementById('btn-restart-lgn-adapter') as HTMLButtonElement;
  const adapterRepairMessage = document.getElementById('diagnostic-adapter-repair-message')!;
  const diagnosticSteps = document.getElementById('diagnostic-steps')!;
  const btnCopyDiagnostics = document.getElementById('btn-copy-diagnostics') as HTMLButtonElement;
  if (report.dualStack && !report.stale) options.renderHealth(report.dualStack);
  adapterRepairPanel.hidden = !report.adapterRestart;
  btnRestartLgnAdapter.disabled = options.busy;
  if (report.adapterRestart) {
    adapterRepairMessage.textContent = `${report.adapterRestart.interfaceName}：${report.adapterRestart.reason}。重启会短暂中断此有线连接并重新获取网络配置，macOS 可能要求管理员授权。`;
  }
  const summary = document.getElementById('diagnostic-summary')!;
  const badge = document.getElementById('diagnostic-summary-badge')!;
  const title = document.getElementById('diagnostic-summary-title')!;
  const meta = document.getElementById('diagnostic-summary-meta')!;
  const overallClass = report.overall === 'healthy' ? 'success'
    : report.overall === 'auth_required' || report.overall === 'partial' || report.overall === 'changed' ? 'warning' : 'error';
  const overallLabel = report.overall === 'changed' ? '需要重新检测' : report.overall === 'healthy' ? '网络正常'
    : report.overall === 'partial' ? '部分可用' : report.overall === 'auth_required' ? '需要认证'
      : report.overall === 'no_network' ? '无网络接口' : '无法联网';
  summary.className = `diagnostic-summary glass-card diagnostic-${overallClass}`;
  badge.className = `health-badge ${overallClass}`;
  badge.textContent = overallLabel;
  title.textContent = report.summary;
  meta.textContent = `${options.formatTime(report.createdAt)} · SSID ${report.ssid || '--'} · IP ${report.ip || '--'}`;
  diagnosticSteps.innerHTML = '';
  report.steps.forEach(step => renderDiagnosticStep(step));
  btnCopyDiagnostics.disabled = false;
}

export function formatDiagnosticReport(report: DiagnosticReport, formatTime: (time: string) => string): string {
  const maskedIp = report.ip
    ? report.ip.split('.').map((part, index) => index < 2 ? part : '*').join('.')
    : '--';
  const lines = [
    'BJUT-AL 网络诊断报告',
    `时间：${formatTime(report.createdAt)}`,
    `结论：${report.summary}`,
    `SSID：${report.ssid || '--'}`,
    `IP：${maskedIp}`,
    '',
  ];
  report.steps.forEach(step => {
    const details = step.message.split('\n').filter(Boolean);
    lines.push(`[${step.status}] ${step.label}（${step.durationMs} ms）：${details.shift() || '--'}`);
    details.forEach(detail => lines.push(`  ${detail}`));
  });
  lines.push('', '报告不包含账号密码。');
  return lines.join('\n');
}

const diagnosticOrder = ['network_identity', 'campus_route', 'campus_environment', 'dns', 'internet', 'authentication_session', 'wired_ipv6_configuration', 'portal'];
export function renderDiagnosticStep(step: DiagnosticStep) {
  const diagnosticSteps = document.getElementById('diagnostic-steps')!;
  const row = document.createElement('div');
  row.className = `diagnostic-step ${step.status}`;
  const marker = document.createElement('span');
  marker.className = 'diagnostic-step-marker';
  marker.textContent = step.status === 'checking' ? '…' : step.status === 'success' ? '✓' : step.status === 'warning' ? '!' : step.status === 'skipped' ? '–' : '×';
  const content = document.createElement('div');
  content.className = 'diagnostic-step-info';
  const label = document.createElement('strong');
  label.textContent = step.label;
  const detail = document.createElement('small');
  detail.textContent = step.message;
  content.append(label, detail);
  const duration = document.createElement('span');
  duration.className = 'diagnostic-step-duration';
  duration.textContent = `${step.durationMs} ms`;
  row.append(marker, content, duration);

  row.dataset.stepId = step.id;
  const previous = Array.from(diagnosticSteps.children).find(child => (child as HTMLElement).dataset.stepId === step.id);
  if (previous) previous.replaceWith(row);
  else {
    const order = diagnosticOrder.indexOf(step.id);
    const next = Array.from(diagnosticSteps.children).find(child => diagnosticOrder.indexOf((child as HTMLElement).dataset.stepId || '') > order);
    diagnosticSteps.insertBefore(row, next || null);
  }
}
