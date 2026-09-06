import type { AdapterInventory, DualStackReport, FamilyConnectivity, LoginProgress, NetworkSchedule, NetworkStatePayload } from './models';

const element = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const familyLabel = (family: FamilyConnectivity) => ({
  reachable: '外网可达', unreachable: '外网探测未通过', not_configured: '未取得可用地址', unknown: '尚未确认',
})[family.status];

export class NetworkExperience {
  private inventory: AdapterInventory | null = null;
  private busy = false;
  private pending = false;
  private identity = '';
  private health: DualStackReport | null = null;
  private readonly select = element<HTMLSelectElement>('preferred-network-adapter');

  constructor(onSelect: (id: string) => Promise<void>, onRefresh: () => Promise<void>) {
    this.select.addEventListener('change', async () => {
      this.pending = true; this.updateBusy();
      element('network-adapter-message').textContent = '正在保存认证网卡选择…';
      try {
        await onSelect(this.select.value);
        this.clearHealth();
        element('network-adapter-message').textContent = '选择已保存，正在重新检测。指定网卡断开时会等待恢复。';
        await onRefresh();
      } catch (error) {
        this.select.value = this.inventory?.preferredInterface || '';
        element('network-adapter-message').textContent = String(error);
      } finally { this.pending = false; this.updateBusy(); }
    });
    element('btn-refresh-adapters').addEventListener('click', async () => {
      this.pending = true; this.updateBusy();
      try { await onRefresh(); } catch (error) { element('network-adapter-message').textContent = String(error); }
      finally { this.pending = false; this.updateBusy(); }
    });
  }

  setBusy(busy: boolean) {
    this.busy = busy;
    this.updateBusy();
  }

  private updateBusy() {
    this.select.disabled = this.busy || this.pending || this.inventory?.selectionSupported === false;
    element<HTMLButtonElement>('btn-refresh-adapters').disabled = this.busy || this.pending;
  }

  renderInventory(inventory: AdapterInventory) {
    this.inventory = inventory;
    this.select.replaceChildren(new Option('自动选择（优先校园有线）', ''));
    const list = element('network-adapter-list'); list.replaceChildren();
    for (const adapter of inventory.adapters) {
      if (adapter.selectable) {
        const option = new Option(`${adapter.name} · ${adapter.interfaceName}${adapter.connected ? '' : ' · 已断开'}`, adapter.id);
        option.disabled = !adapter.connected || adapter.ipv4.length === 0;
        this.select.add(option);
      }
      const row = document.createElement('div'); row.className = `network-adapter-row${adapter.selected ? ' selected' : ''}`;
      const title = document.createElement('strong');
      const kind = ({ wifi: 'Wi-Fi', ethernet: '有线', vpn: '虚拟 / VPN', cellular: '移动数据' } as Record<string, string>)[adapter.transport] || adapter.transport;
      title.textContent = `${adapter.name} · ${kind}${adapter.selected ? ' · 用于认证' : ''}`;
      const detail = document.createElement('small');
      detail.textContent = `${adapter.interfaceName} · ${adapter.connected ? '已启用' : '已断开'} · IPv4 ${adapter.ipv4.join(' / ') || '--'} · IPv6 ${adapter.ipv6.length ? '已取得地址' : '--'}`;
      row.append(title, detail); list.append(row);
    }
    if (inventory.preferredInterface && !inventory.adapters.some(adapter => adapter.id === inventory.preferredInterface && adapter.selectable)) {
      this.select.add(new Option(`已指定的网卡不可用 · ${inventory.preferredInterface}`, inventory.preferredInterface));
    }
    this.select.value = inventory.preferredInterface;
    const selected = inventory.adapters.find(adapter => adapter.selected);
    element('network-selection-caption').textContent = selected ? `当前认证网卡：${selected.name}（${selected.interfaceName}）`
      : inventory.preferredInterface ? '指定网卡不可用，认证已暂停' : '尚未取得可用于认证的网卡';
    if (!inventory.selectionSupported) element('network-adapter-message').textContent = 'Android 由系统选择校园 Wi-Fi；下方同时展示其他网络。';
    this.setBusy(this.busy);
  }

  renderState(state: NetworkStatePayload) {
    const identity = `${state.interfaceName || ''}|${state.ip || ''}`;
    const matches = this.health && this.health.interfaceName === state.interfaceName && this.health.ipv4.addresses.includes(state.ip || '');
    if (this.identity && identity !== this.identity && !matches) this.clearHealth();
    this.identity = identity;
    element('network-authentication-status').textContent = `门户认证：${state.state === 'BjutCampus' ? '需要认证' : state.state === 'Online' && state.loginType && state.loginType !== 'unknown' ? '已建立会话' : '尚未核对'}`;
    element('network-system-status').textContent = `${this.inventory?.selectionSupported === false ? '应用当前路径' : '系统互联网'}：${typeof state.systemOnline !== 'boolean' ? '待检测' : state.systemOnline ? '可达' : '探测未通过'}`;
    if (this.health && this.health.interfaceName === state.interfaceName && this.health.ipv4.addresses.includes(state.ip || '')) this.renderHealth(this.health);
  }

  clearHealth() {
    this.health = null;
    element('dual-stack-checked-at').textContent = '认证网卡已变化，等待重新检测 IPv4 与 IPv6。';
    for (const id of ['ipv4-health', 'ipv6-health']) {
      const node = element(id); node.className = 'family-health';
      node.querySelector('strong')!.textContent = '等待重新检测'; node.querySelector('small')!.textContent = '';
    }
  }

  renderHealth(health: DualStackReport) {
    if (this.health && Date.parse(health.checkedAt) < Date.parse(this.health.checkedAt)) return;
    this.health = health;
    if (this.identity && this.identity !== `${health.interfaceName}|${health.ipv4.addresses[0] || ''}`) return;
    for (const [id, family] of [['ipv4-health', health.ipv4], ['ipv6-health', health.ipv6]] as const) {
      const node = element(id); node.className = `family-health ${family.status}`;
      node.querySelector('strong')!.textContent = familyLabel(family);
      node.querySelector('small')!.textContent = `${family.addresses.length ? '地址已取得' : '地址未取得'} · ${family.durationMs} ms\n${family.detail}`;
    }
    element('dual-stack-checked-at').textContent = `${health.interfaceName || '未选择网卡'} · ${new Date(health.checkedAt).toLocaleTimeString()} · IPv4、IPv6 分别通过此网卡实测`;
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
  private onCancel: (id: string) => Promise<void>;

  constructor(onCancel: (id: string) => Promise<void>) {
    this.onCancel = onCancel;
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
    this.id = crypto.randomUUID(); this.started = performance.now(); this.cancelled = false; this.safetyMs = null;
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
    this.canCancel = false;
    if (this.timer !== null) { window.clearInterval(this.timer); this.timer = null; }
    element<HTMLButtonElement>('btn-cancel-login').disabled = true;
    element('login-progress-time').textContent = `总用时 ${((performance.now() - this.started) / 1000).toFixed(1)} 秒`;
    if (message) element('login-progress-message').textContent = message;
  }
}
