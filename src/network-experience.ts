import type { AdapterInventory, DualStackReport, FamilyConnectivity, LoginProgress, NetworkSchedule, NetworkStatePayload } from './models';

const element = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
export const compactFamilyLabel = (family: FamilyConnectivity) => family.status === 'reachable'
  ? `${Math.round(family.durationMs)} ms`
  : ({ timeout: '超时', checking: '检测中', unreachable: '未通过', not_configured: '无地址', unknown: '未确认' } as Record<string, string>)[family.status] || '未确认';

export class NetworkExperience {
  private inventory: AdapterInventory | null = null;
  private busy = false;
  private pending = false;
  private identity = '';
  private health: DualStackReport | null = null;
  private readonly list = element('network-adapter-list');
  private onSelect: (id: string) => Promise<void>;
  private onRefresh: () => Promise<void>;

  constructor(onSelect: (id: string) => Promise<void>, onRefresh: () => Promise<void>) {
    this.onSelect = onSelect; this.onRefresh = onRefresh;
    this.list.addEventListener('click', event => {
      const button = (event.target as HTMLElement).closest<HTMLButtonElement>('button[data-adapter-id]');
      if (button && !button.disabled) void this.choose(button.dataset.adapterId || '');
    });
    this.list.addEventListener('keydown', event => {
      if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return;
      const options = Array.from(this.list.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
      const index = options.indexOf(document.activeElement as HTMLButtonElement);
      if (!options.length || index < 0) return;
      event.preventDefault();
      const direction = event.key === 'ArrowUp' || event.key === 'ArrowLeft' ? -1 : 1;
      const next = options[(index + direction + options.length) % options.length];
      next.focus(); next.click();
    });
    element('btn-refresh-adapters').addEventListener('click', async () => {
      this.pending = true; this.updateBusy();
      try { await onRefresh(); } catch (error) { this.showMessage(String(error)); }
      finally { this.pending = false; this.updateBusy(); }
    });
  }

  private showMessage(message: string) {
    element('network-adapter-message').textContent = message;
    element('network-adapter-message').hidden = !message;
  }

  private async choose(id: string) {
    if (this.busy || this.pending || this.inventory?.selectionSupported === false) return;
    this.pending = true; this.updateBusy(); this.showMessage('正在保存选择…');
    try { await this.onSelect(id); await this.onRefresh(); this.showMessage(''); }
    catch (error) { this.showMessage(String(error)); }
    finally {
      this.pending = false; this.updateBusy();
      if (document.activeElement === document.body || this.list.contains(document.activeElement)) {
        Array.from(this.list.querySelectorAll<HTMLButtonElement>('button')).find(button => button.dataset.adapterId === id && !button.disabled)?.focus({ preventScroll: true });
      }
    }
  }

  setBusy(busy: boolean) { this.busy = busy; this.updateBusy(); }

  private updateBusy() {
    this.list.querySelectorAll<HTMLButtonElement>('button').forEach(button => {
      button.disabled = this.busy || this.pending || button.dataset.unavailable === 'true' || this.inventory?.selectionSupported === false;
    });
    element<HTMLButtonElement>('btn-refresh-adapters').disabled = this.busy || this.pending;
    // A saved but disconnected choice must not remove the group from tab order.
    if (!this.list.querySelector('button[tabindex="0"]:not(:disabled)')) {
      const firstAvailable = this.list.querySelector<HTMLButtonElement>('button:not(:disabled)');
      if (firstAvailable) firstAvailable.tabIndex = 0;
    }
  }

  renderInventory(inventory: AdapterInventory) {
    this.inventory = inventory;
    const focused = (document.activeElement as HTMLElement)?.dataset.adapterId;
    this.list.replaceChildren();
    const addRow = (id: string, title: string, detail: string, chosen: boolean, available: boolean, active = false) => {
      const row = document.createElement('button'); row.type = 'button';
      row.className = `network-adapter-row${chosen ? ' chosen' : ''}${active ? ' in-use' : ''}`;
      row.dataset.adapterId = id; row.dataset.unavailable = String(!available);
      row.setAttribute('role', 'radio'); row.setAttribute('aria-checked', String(chosen)); row.tabIndex = chosen ? 0 : -1;
      const marker = document.createElement('span'); marker.className = 'adapter-radio-marker'; marker.setAttribute('aria-hidden', 'true');
      const text = document.createElement('span'); const label = document.createElement('strong'); label.textContent = title;
      const description = document.createElement('small'); description.textContent = detail;
      text.append(label, description); row.append(marker, text); this.list.append(row);
    };
    if (inventory.selectionSupported) addRow('', '自动选择', '优先校园有线，随当前连接自动识别', !inventory.preferredInterface, true);
    const adapters = [...inventory.adapters].sort((a, b) => Number(b.selected) - Number(a.selected) || Number(b.connected) - Number(a.connected));
    for (const adapter of adapters) {
      const kind = ({ wifi: 'Wi-Fi', ethernet: '有线', vpn: '虚拟 / VPN', cellular: '移动数据' } as Record<string, string>)[adapter.transport] || adapter.transport;
      const title = adapter.name === kind ? adapter.name : `${adapter.name} · ${kind}`;
      addRow(adapter.id, `${title}${adapter.selected ? ' · 用于认证' : ''}`,
        `${adapter.interfaceName} · ${adapter.connected ? '已启用' : '已断开'} · IPv4 ${adapter.ipv4.join(' / ') || '--'} · IPv6 ${adapter.ipv6.length ? '已取得地址' : '--'}`,
        inventory.preferredInterface === adapter.id, adapter.selectable && adapter.connected && adapter.ipv4.length > 0, adapter.selected);
    }
    if (inventory.preferredInterface && !inventory.adapters.some(adapter => adapter.id === inventory.preferredInterface)) {
      addRow(inventory.preferredInterface, '指定网卡当前不可用', '等待网卡恢复，或选择其他网卡', true, false);
    }
    const selected = inventory.adapters.find(adapter => adapter.selected);
    element('network-selection-caption').textContent = selected ? `当前认证网卡：${selected.name}（${selected.interfaceName}）`
      : inventory.preferredInterface ? '指定网卡不可用，认证已暂停' : '尚未取得可用于认证的网卡';
    if (!inventory.selectionSupported) this.showMessage('Android 由系统选择校园 Wi-Fi；下方同时展示其他网络。');
    this.updateBusy();
    if (focused !== undefined) Array.from(this.list.querySelectorAll<HTMLButtonElement>('button')).find(button => button.dataset.adapterId === focused && !button.disabled)?.focus({preventScroll:true});
  }

  renderState(state: NetworkStatePayload) {
    const identity = `${state.interfaceName || ''}|${state.ip || ''}`;
    if (this.identity && identity !== this.identity && this.health?.scope !== 'system') this.clearHealth();
    this.identity = identity;
  }

  clearHealth() {
    this.health = null;
    for (const id of ['ipv4-health', 'ipv6-health']) {
      const node = element(id); node.className = 'family-status'; node.title = '等待重新检测';
      node.querySelector('strong')!.textContent = '检测中'; node.querySelector('.family-marker')!.textContent = '…';
    }
  }

  renderHealth(health: DualStackReport) {
    if (this.health && Date.parse(health.checkedAt) < Date.parse(this.health.checkedAt)) return;
    this.health = health;
    for (const [id, family] of [['ipv4-health', health.ipv4], ['ipv6-health', health.ipv6]] as const) {
      const node = element(id); node.className = `family-status ${family.status}`;
      node.querySelector('strong')!.textContent = compactFamilyLabel(family);
      node.querySelector('.family-marker')!.textContent = family.status === 'reachable' ? '✓' : family.status === 'checking' ? '…' : family.status === 'not_configured' ? '–' : '×';
      node.title = `${new Date(health.checkedAt).toLocaleTimeString()} · ${family.detail}`;
    }
  }

  renderSchedule(plan: NetworkSchedule) {
    element('network-schedule-summary').textContent = `${plan.reason}。当前每 ${plan.intervalSeconds} 秒检测连通性，每 ${plan.interfacePollSeconds} 秒核对接口；网络变化事件仍会立即响应。`;
  }
}

const phases: Record<string, string> = { safety: '安全检查', checking: '核对网卡', gateway: '确认网关', queued: '等待操作', logout: '注销旧账号', preparing: '准备认证', authenticating: '提交认证', verifying: '验证联网', complete: '完成', stopped: '已停止' };

export class LoginProgressView {
  private id = '';
  private started = 0;
  private timer: number | null = null;
  private cancelled = false;
  private canCancel = false;
  private safetyMs: number | null = null;
  private hideTimer: number | null = null;
  private finished = false;
  private onFinished: (summary: string) => void;
  private onCancel: (id: string) => Promise<void>;

  constructor(onCancel: (id: string) => Promise<void>, onFinished: (summary: string) => void = () => {}) {
    this.onCancel = onCancel; this.onFinished = onFinished;
    element('btn-cancel-login').addEventListener('click', async () => {
      if (!this.canCancel) return;
      element<HTMLButtonElement>('btn-cancel-login').disabled = true;
      try {
        await this.onCancel(this.id); this.cancelled = true;
        element('login-progress-message').textContent = '已请求取消，正在停止提交前的准备工作…';
      } catch (error) { element('login-progress-message').textContent = String(error); }
    });
  }

  begin() {
    this.id = crypto.randomUUID(); this.started = performance.now(); this.cancelled = false; this.safetyMs = null; this.finished = false;
    if (this.hideTimer !== null) { window.clearTimeout(this.hideTimer); this.hideTimer = null; }
    element('btn-cancel-login').hidden = false;
    if (this.timer !== null) window.clearInterval(this.timer);
    this.timer = window.setInterval(() => { element('login-progress-time').textContent = `已用时 ${((performance.now() - this.started) / 1000).toFixed(1)} 秒`; }, 100);
    element('login-progress-panel').hidden = false;
    element('login-progress-phases').replaceChildren();
    this.local('安全检查中，可在提交认证前取消');
    return this.id;
  }

  isCancelled() { return this.cancelled; }
  local(message: string) {
    this.canCancel = true;
    element('login-progress-message').textContent = message;
    element<HTMLButtonElement>('btn-cancel-login').disabled = false;
  }

  render(progress: LoginProgress) {
    if (progress.operationId !== this.id) return;
    this.safetyMs ??= Math.max(0, performance.now() - this.started - progress.elapsedMs);
    this.canCancel = progress.canCancel && !this.cancelled;
    element<HTMLButtonElement>('btn-cancel-login').disabled = !this.canCancel;
    element('login-progress-message').textContent = progress.message;
    element('login-progress-phases').textContent = [`安全检查 ${(this.safetyMs / 1000).toFixed(1)} 秒`, ...progress.timings.map(timing => `${phases[timing.phase] || timing.phase} ${(timing.durationMs / 1000).toFixed(1)} 秒`)].join(' · ');
    if (progress.phase === 'complete' || progress.phase === 'stopped') this.finish(progress.message);
  }

  finish(message?: string) {
    if (this.finished) return;
    this.finished = true;
    this.canCancel = false;
    if (this.timer !== null) { window.clearInterval(this.timer); this.timer = null; }
    element<HTMLButtonElement>('btn-cancel-login').disabled = true;
    element('login-progress-time').textContent = `总用时 ${((performance.now() - this.started) / 1000).toFixed(1)} 秒`;
    if (message) element('login-progress-message').textContent = message;
    element('btn-cancel-login').hidden = true;
    this.onFinished([element('login-progress-message').textContent, element('login-progress-time').textContent, element('login-progress-phases').textContent].filter(Boolean).join('；'));
    this.hideTimer = window.setTimeout(() => { element('login-progress-panel').hidden = true; this.hideTimer = null; }, 15_000);
  }
}
