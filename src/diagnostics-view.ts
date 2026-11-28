import type { DiagnosticReport, DiagnosticStep } from './models';
import { compactFamilyLabel } from './network-experience';
import { animateResize, setVisible } from './layout-motion';
import { diagnosticAdvice } from './diagnostic-advice';
import { presentationDialog } from './presentation-dialog';
import { writeTextToClipboard } from './platform';

export type ShowDetails = (text: string, title: string) => Promise<void>;
export type RenderIconsFn = (root: ParentNode) => void;

const STEP_STATUS_META: Record<
  DiagnosticStep['status'],
  { label: string; badgeClass: string; marker: string }
> = {
  success: { label: '通过', badgeClass: 'success', marker: '✓' },
  warning: { label: '需注意', badgeClass: 'warning', marker: '!' },
  error: { label: '异常', badgeClass: 'error', marker: '×' },
  skipped: { label: '已跳过', badgeClass: 'neutral', marker: '–' },
  checking: { label: '检查中', badgeClass: 'neutral', marker: '…' },
};

function createLucideIconWithFallback(
  name: 'info' | 'copy',
  renderIcons?: RenderIconsFn,
): HTMLElement | SVGSVGElement {
  const holder = document.createElement('span');
  holder.style.display = 'contents';
  const placeholder = document.createElement('i');
  placeholder.setAttribute('data-lucide', name);
  holder.append(placeholder);
  renderIcons?.(holder);
  const hydrated = holder.firstElementChild;
  if (hydrated && hydrated.tagName.toLowerCase() === 'svg') {
    return hydrated as SVGSVGElement;
  }
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('width', '16');
  svg.setAttribute('height', '16');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '2');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('data-lucide', name);
  svg.setAttribute('data-icon-name', name);
  svg.setAttribute('data-icon-engine', 'lucide');
  svg.setAttribute('class', `lucide lucide-${name}`);
  svg.innerHTML = name === 'info'
    ? '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>'
    : '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>';
  return svg;
}

function parseDetailLine(line: string): { key?: string; value: string } {
  const match = line.match(/^([^:：]{1,36})[：:]\s*(.+)$/);
  if (!match) return { value: line };
  const candidateKey = match[1].trim();
  const value = match[2].trim();
  // Avoid splitting URL schemes (http/https) or bare IPv6 segments.
  if (!candidateKey || /^https?$/i.test(candidateKey) || /^[0-9a-f]{1,4}$/i.test(candidateKey)) {
    return { value: line };
  }
  return { key: candidateKey, value };
}

function isTechnicalValue(text: string): boolean {
  return /(?:\d{1,3}\.){3}\d{1,3}|[0-9a-f]{0,4}:[0-9a-f:]{2,}|[a-z0-9-]+\.[a-z]{2,}|\b(?:en|eth|wlan|tun|utun)\d*\b/i.test(text);
}

export async function showDiagnosticStepDetailsModal(
  step: DiagnosticStep,
  renderIcons?: RenderIconsFn,
): Promise<void> {
  const { form, close } = presentationDialog('diagnostic-step-details-dialog', step.label);
  form.classList.add('diagnostic-details-modal');

  const meta = STEP_STATUS_META[step.status] ?? STEP_STATUS_META.checking;
  const statusRow = document.createElement('div');
  statusRow.className = 'diagnostic-details-status-row';

  const statusBadge = document.createElement('span');
  statusBadge.className = `health-badge ${meta.badgeClass}`;
  statusBadge.textContent = meta.label;

  const durationPill = document.createElement('span');
  durationPill.className = 'diagnostic-detail-duration';
  durationPill.textContent = `耗时 ${Math.round(step.durationMs)} ms`;
  statusRow.append(statusBadge, durationPill);
  form.append(statusRow);

  const rawMessage = (step.message || '').trim();
  if (step.summary && step.summary.trim() && step.summary.trim() !== rawMessage) {
    const summaryBox = document.createElement('p');
    summaryBox.className = 'diagnostic-details-summary';
    summaryBox.textContent = step.summary.trim();
    form.append(summaryBox);
  }

  const lines = rawMessage ? rawMessage.split('\n').map(item => item.trim()).filter(Boolean) : [];
  const detailsList = document.createElement('div');
  detailsList.className = 'diagnostic-details-list';
  if (lines.length) {
    for (const line of lines) {
      const parsed = parseDetailLine(line);
      const item = document.createElement('div');
      item.className = 'diagnostic-detail-item';
      if (parsed.key) {
        item.classList.add('is-pair');
        const keyEl = document.createElement('span');
        keyEl.className = 'diagnostic-detail-key';
        keyEl.textContent = parsed.key;
        const valEl = document.createElement('strong');
        valEl.className = `diagnostic-detail-value${isTechnicalValue(parsed.value) ? ' is-mono' : ''}`;
        valEl.textContent = parsed.value;
        item.append(keyEl, valEl);
      } else {
        const textEl = document.createElement('p');
        textEl.className = `diagnostic-detail-text${isTechnicalValue(parsed.value) ? ' is-mono' : ''}`;
        textEl.textContent = parsed.value;
        item.append(textEl);
      }
      detailsList.append(item);
    }
  } else {
    const empty = document.createElement('div');
    empty.className = 'diagnostic-empty';
    empty.textContent = '暂无更多细节';
    detailsList.append(empty);
  }
  form.append(detailsList);

  if (step.connectivity) {
    const matrixWrap = document.createElement('section');
    matrixWrap.className = 'diagnostic-details-matrix';
    const matrixTitle = document.createElement('h4');
    matrixTitle.textContent = '路径连通性明细';
    matrixWrap.append(matrixTitle, connectivityTable(step.connectivity));
    form.append(matrixWrap);
  }

  const actions = document.createElement('div');
  actions.className = 'modal-actions diagnostic-details-actions';

  const copyBtn = document.createElement('button');
  copyBtn.type = 'button';
  copyBtn.className = 'btn btn-secondary';
  const copyIcon = createLucideIconWithFallback('copy', renderIcons);
  const copyLabel = document.createElement('span');
  copyLabel.textContent = '复制详情';
  copyBtn.append(copyIcon, copyLabel);

  const copyPayload = [
    `${step.label}（${meta.label} · ${Math.round(step.durationMs)} ms）`,
    ...(step.summary ? [`摘要：${step.summary}`] : []),
    ...(lines.length ? lines : ['暂无更多细节']),
  ].join('\n');

  let copiedTimer: number | null = null;
  copyBtn.addEventListener('click', async () => {
    try {
      await writeTextToClipboard(copyPayload);
      copyLabel.textContent = '已复制';
      if (copiedTimer !== null) window.clearTimeout(copiedTimer);
      copiedTimer = window.setTimeout(() => {
        copiedTimer = null;
        if (copyLabel.isConnected) copyLabel.textContent = '复制详情';
      }, 1500);
    } catch {
      copyLabel.textContent = '复制失败';
    }
  });

  const closeBtn = document.createElement('button');
  closeBtn.type = 'button';
  closeBtn.className = 'btn btn-primary';
  closeBtn.textContent = '关闭';
  closeBtn.addEventListener('click', close);

  form.addEventListener('presentation-close', () => {
    if (copiedTimer !== null) {
      window.clearTimeout(copiedTimer);
      copiedTimer = null;
    }
  });

  actions.append(copyBtn, closeBtn);
  form.append(actions);
}

export function renderDiagnosticReportView(
  report: DiagnosticReport,
  options: {
    busy: boolean;
    showDetails?: ShowDetails;
    formatTime: (time: string) => string;
    renderIcons?: RenderIconsFn;
  },
) {
  const adapterRepairPanel = document.getElementById('diagnostic-adapter-repair')!;
  const btnRestartLgnAdapter = document.getElementById('btn-restart-lgn-adapter') as HTMLButtonElement;
  const adapterRepairMessage = document.getElementById('diagnostic-adapter-repair-message')!;
  const diagnosticSteps = document.getElementById('diagnostic-steps')!;
  const btnCopyDiagnostics = document.getElementById('btn-copy-diagnostics') as HTMLButtonElement;
  setVisible(adapterRepairPanel, !!report.adapterRestart);
  btnRestartLgnAdapter.disabled = options.busy;
  if (report.adapterRestart) {
    adapterRepairMessage.textContent = `${report.adapterRestart.interfaceName}：${report.adapterRestart.reason}。重启网卡会短暂断开当前有线连接并重新获取配置，macOS 上可能需要输入管理员密码。`;
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
    report.steps.forEach(step => renderDiagnosticStep(step, options.showDetails, false, options.renderIcons));
  });
  btnCopyDiagnostics.disabled = false;
}

function connectivityTable(matrix: NonNullable<DiagnosticStep['connectivity']>) {
  const table = document.createElement('table');
  table.className = 'diagnostic-connectivity';
  table.setAttribute('aria-label', '互联网 IPv4 与 IPv6 连通性');
  const heading = table.createTHead().insertRow();
  for (const headerTitle of ['路径', 'IPv4', 'IPv6']) {
    const cell = document.createElement('th');
    cell.scope = 'col';
    cell.textContent = headerTitle;
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
      const labelText = index === 1 && result.status === 'dns_error' ? '待确认' : compactFamilyLabel(result);
      cell.dataset.status = result.status;
      cell.textContent = result.status === 'reachable' ? `✓ ${labelText}` : labelText;
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

const diagnosticOrder = [
  'network_identity',
  'campus_route',
  'campus_environment',
  'dns',
  'internet',
  'authentication_session',
  'wired_ipv6_configuration',
  'portal',
];

export function renderDiagnosticStep(
  step: DiagnosticStep,
  showDetails?: ShowDetails,
  animate = true,
  renderIcons?: RenderIconsFn,
) {
  const diagnosticSteps = document.getElementById('diagnostic-steps')!;
  const row = document.createElement('div');
  row.className = `diagnostic-step ${step.status}${animate ? ' diagnostic-step-enter' : ''}`;
  const meta = STEP_STATUS_META[step.status] ?? STEP_STATUS_META.error;
  const marker = document.createElement('span');
  marker.className = 'diagnostic-step-marker';
  marker.textContent = meta.marker;
  const content = document.createElement('div');
  content.className = 'diagnostic-step-info';
  const header = document.createElement('div');
  header.className = 'diagnostic-step-header';
  const label = document.createElement('strong');
  label.textContent = step.label;
  const details = document.createElement('button');
  details.type = 'button';
  details.className = 'btn-icon diagnostic-details-button diagnostic-info-icon-btn';
  details.setAttribute('aria-label', `${step.label}详细信息`);
  details.title = '查看详细信息';
  details.append(createLucideIconWithFallback('info', renderIcons));
  details.addEventListener('click', () => {
    if (showDetails) {
      void showDetails(`${step.message || '暂无详细信息'}\n\n耗时 ${step.durationMs} ms`, step.label);
    } else {
      void showDiagnosticStepDetailsModal(step, renderIcons);
    }
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
