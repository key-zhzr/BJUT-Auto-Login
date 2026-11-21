import type { DiagnosticReport, DiagnosticStep, DualStackReport } from './models';
import { animateResize, setVisible } from './layout-motion';
import { diagnosticAdvice } from './diagnostic-advice';

export function renderDiagnosticReportView(report: DiagnosticReport, options: { busy: boolean; renderHealth: (health: DualStackReport) => void; formatTime: (time: string) => string }) {
  const adapterRepairPanel = document.getElementById('diagnostic-adapter-repair')!;
  const btnRestartLgnAdapter = document.getElementById('btn-restart-lgn-adapter') as HTMLButtonElement;
  const adapterRepairMessage = document.getElementById('diagnostic-adapter-repair-message')!;
  const diagnosticSteps = document.getElementById('diagnostic-steps')!;
  const btnCopyDiagnostics = document.getElementById('btn-copy-diagnostics') as HTMLButtonElement;
  if (report.dualStack && !report.stale) options.renderHealth(report.dualStack);
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
    report.steps.forEach(step => renderDiagnosticStep(step, false));
  });
  btnCopyDiagnostics.disabled = false;
}

function classifyStepValueTone(value: string, stepStatus: DiagnosticStep['status']): string {
  if (/^(?:通过|符合|已取得|同源|是)(?:$|（|\s|·)|解析成功|解析正常|校验通过|可用$/.test(value)) return 'is-ok';
  if (/^(?:未配置|未探测到|不适用|检测中)(?:$|（|\s|·)/.test(value)) return 'is-muted';
  if (/^(?:超时|失败|未通过|不符合|缺失|否|不可用)(?:$|（|\s|·)|门户已发现|无响应|错误/.test(value)) {
    return stepStatus === 'error' ? 'is-error' : 'is-warn';
  }
  return '';
}

function appendDiagnosticMessage(container: HTMLElement, message: string, stepStatus: DiagnosticStep['status']) {
  const lines = message.split('\n').map(line => line.trim()).filter(Boolean);
  if (!lines.length) return;
  let kvList: HTMLDivElement | null = null;
  lines.forEach((line, index) => {
    const colonIdx = line.indexOf('：');
    const keyCandidate = colonIdx > 0 ? line.slice(0, colonIdx).trim() : '';
    const valueCandidate = colonIdx > 0 ? line.slice(colonIdx + 1).trim() : '';
    const isSingleHeadline = lines.length === 1 && !keyCandidate.includes('·') && (stepStatus === 'success' || keyCandidate.length > 14);
    const isKvLine = Boolean(
      !isSingleHeadline
      && keyCandidate
      && valueCandidate
      && keyCandidate.length <= 26
      && !/[。！？]$/.test(keyCandidate)
      && (index > 0 || lines.length > 1),
    );
    if (isKvLine) {
      if (!kvList) {
        kvList = document.createElement('div');
        kvList.className = 'diagnostic-step-kv-list';
        container.append(kvList);
      }
      const row = document.createElement('div');
      row.className = 'diagnostic-step-kv';
      const key = document.createElement('span');
      key.className = 'diagnostic-kv-key';
      key.textContent = keyCandidate;
      const val = document.createElement('span');
      const tone = classifyStepValueTone(valueCandidate, stepStatus);
      val.className = tone ? `diagnostic-kv-val ${tone}` : 'diagnostic-kv-val';
      val.textContent = valueCandidate;
      row.append(key, val);
      kvList.append(row);
    } else {
      kvList = null;
      const lead = document.createElement('p');
      lead.className = 'diagnostic-step-lead';
      lead.textContent = line;
      container.append(lead);
    }
  });
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
export function renderDiagnosticStep(step: DiagnosticStep, animate = true) {
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
  const duration = document.createElement('span');
  duration.className = 'diagnostic-step-duration';
  duration.textContent = `${step.durationMs} ms`;
  header.append(label, duration);
  content.append(header);
  appendDiagnosticMessage(content, step.message, step.status);
  row.append(marker, content);

  row.dataset.stepId = step.id;
  const previous = Array.from(diagnosticSteps.children).find(child => (child as HTMLElement).dataset.stepId === step.id);
  const update=()=>{if (previous) previous.replaceWith(row);
  else {
    const order = diagnosticOrder.indexOf(step.id);
    const next = Array.from(diagnosticSteps.children).find(child => diagnosticOrder.indexOf((child as HTMLElement).dataset.stepId || '') > order);
    diagnosticSteps.insertBefore(row, next || null);
  }};
  if(animate)animateResize(diagnosticSteps,update);else update();
}
