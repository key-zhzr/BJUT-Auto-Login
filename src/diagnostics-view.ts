import type { DiagnosticReport, DiagnosticStep } from './models';
import { compactFamilyLabel } from './network-experience';
import { animateResize, setVisible } from './layout-motion';
import { diagnosticAdvice } from './diagnostic-advice';

type ShowDetails = (text: string, title: string) => Promise<void>;

export function renderDiagnosticReportView(report: DiagnosticReport, options: { busy: boolean; showDetails: ShowDetails; formatTime: (time: string) => string }) {
  const adapterRepairPanel = document.getElementById('diagnostic-adapter-repair')!;
  const btnRestartLgnAdapter = document.getElementById('btn-restart-lgn-adapter') as HTMLButtonElement;
  const adapterRepairMessage = document.getElementById('diagnostic-adapter-repair-message')!;
  const diagnosticSteps = document.getElementById('diagnostic-steps')!;
  const btnCopyDiagnostics = document.getElementById('btn-copy-diagnostics') as HTMLButtonElement;
  setVisible(adapterRepairPanel, !!report.adapterRestart);
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
  const advice = document.getElementById('diagnostic-advice');
  if (advice) {
    animateResize(advice, () => {
      advice.replaceChildren();
      const items = diagnosticAdvice(report);
      if (!items.length) return;
      const heading = document.createElement('strong');
      heading.className = 'diagnostic-advice-title';
      heading.textContent = '排查建议';
      const list = document.createElement('ul');
      list.className = 'diagnostic-advice-list';
      for (const message of items) {
        const li = document.createElement('li');
        li.textContent = message;
        list.append(li);
      }
      advice.append(heading, list);
    });
  }
  meta.replaceChildren();
  for (const [k, v] of [
    ['时间', options.formatTime(report.createdAt)],
    ['SSID', report.ssid || '--'],
    ['IP', report.ip || '--'],
  ]) {
    const chip = document.createElement('span');
    chip.className = 'diagnostic-meta-chip';
    const key = document.createElement('span');
    key.textContent = k;
    const val = document.createElement('strong');
    val.textContent = v;
    chip.append(key, val);
    meta.append(chip);
  }
  animateResize(diagnosticSteps, () => {
    diagnosticSteps.innerHTML = '';
    report.steps.forEach(step => renderDiagnosticStep(step, options.showDetails, false));
  });
  btnCopyDiagnostics.disabled = false;
}

function connectivityTable(matrix: NonNullable<DiagnosticStep['connectivity']>) {
  const table = document.createElement('table');
  table.className = 'diagnostic-connectivity';
  table.setAttribute('aria-label', '互联网 IPv4 与 IPv6 连通性');
  const heading = table.createTHead().insertRow();
  for (const title of ['路径', 'IPv4', 'IPv6']) {
    const cell = document.createElement('th');
    cell.scope = 'col';
    cell.textContent = title;
    heading.append(cell);
  }
  const body = table.createTBody();
  for (const [path, results] of [['系统路由', matrix.system], ['网卡直连', matrix.direct]] as const) {
    const row = body.insertRow();
    const label = document.createElement('th');
    label.scope = 'row';
    label.textContent = path;
    row.append(label);
    results.forEach((result, index) => {
      const cell = row.insertCell();
      // The system DNS card has the IPv6 target resolution and fallback details.
      const label = index === 1 && result.status === 'dns_error' ? '待确认' : compactFamilyLabel(result);
      cell.dataset.status = result.status;
      cell.textContent = result.status === 'reachable' ? `✓ ${label}` : label;
    });
  }
  return table;
}

export function formatDiagnosticReport(report: DiagnosticReport, formatTime: (time: string) => string): string {
  const maskedIp = report.ip.includes(':') ? '[本地 IPv6]' : report.ip
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
    const mask = (text: string) => report.ip ? text.replaceAll(report.ip, maskedIp) : text;
    const details = mask(step.message).split('\n').filter(Boolean);
    lines.push(`[${step.status}] ${step.label}（${step.durationMs} ms）：${step.summary ? mask(step.summary) : details.shift() || '--'}`);
    details.forEach(detail => lines.push(`  ${detail}`));
  });
  lines.push('', '报告不包含账号密码。');
  return lines.join('\n');
}

const diagnosticOrder = ['network_identity', 'campus_route', 'campus_environment', 'dns', 'internet', 'authentication_session', 'wired_ipv6_configuration', 'portal'];
export function renderDiagnosticStep(step: DiagnosticStep, showDetails: ShowDetails, animate = true) {
  const diagnosticSteps = document.getElementById('diagnostic-steps')!;
  const row = document.createElement('div');
  row.className = `diagnostic-step ${step.status}${animate ? ' diagnostic-step-enter' : ''}`;
  const marker = document.createElement('span');
  marker.className = 'diagnostic-step-marker';
  marker.textContent = step.status === 'checking' ? '…' : step.status === 'success' ? '✓' : step.status === 'warning' ? '!' : step.status === 'skipped' ? '–' : '×';
  const content = document.createElement('div');
  content.className = 'diagnostic-step-info';
  const header = document.createElement('div');
  header.className = 'diagnostic-step-header';
  const label = document.createElement('strong');
  label.textContent = step.label;
  const details = document.createElement('button');
  details.type = 'button';
  details.className = 'btn btn-secondary btn-sm diagnostic-details-button';
  details.textContent = '详细信息';
  details.setAttribute('aria-label', `${step.label}详细信息`);
  details.addEventListener('click', () => {
    void showDetails(`${step.message || '暂无详细结果'}\n\n耗时 ${step.durationMs} ms`, step.label);
  });
  header.append(label, details);
  content.append(header);
  if (step.connectivity) {
    content.append(connectivityTable(step.connectivity));
  } else {
    const summary = document.createElement('p');
    summary.className = 'diagnostic-step-lead';
    summary.textContent = step.summary || step.message.split('\n', 1)[0];
    content.append(summary);
  }
  row.append(marker, content);

  row.dataset.stepId = step.id;
  const previous = Array.from(diagnosticSteps.children).find(child => (child as HTMLElement).dataset.stepId === step.id);
  const update = () => {
    if (previous) {
      const restoreFocus = previous.contains(document.activeElement);
      previous.replaceWith(row);
      if (restoreFocus) details.focus({ preventScroll: true });
      return;
    }
    const order = diagnosticOrder.indexOf(step.id);
    const next = Array.from(diagnosticSteps.children).find(child => diagnosticOrder.indexOf((child as HTMLElement).dataset.stepId || '') > order);
    diagnosticSteps.insertBefore(row, next || null);
  };
  if (animate) animateResize(diagnosticSteps, update);
  else update();
}
