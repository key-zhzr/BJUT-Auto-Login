import type { AdapterInventory, DualStackReport, FamilyConnectivity, NetworkSchedule, NetworkStatePayload } from './models';

const element = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
export const compactFamilyLabel = (family: FamilyConnectivity) => family.status === 'reachable'
  ? `${Math.round(family.durationMs)} ms`
  : ({ timeout: '超时', checking: '检测中', unreachable: '未通过', not_configured: '未配置', dns_error: 'DNS 失败', tls_error: 'TLS 失败', connection_error: '连接失败', response_error: '响应异常', unknown: '未确认' } as Record<string, string>)[family.status] || '未确认';

export class NetworkExperience {
  private inventory: AdapterInventory | null = null;
  private busy = false;
  private pending = false;
  private identity = '';
  private health: DualStackReport | null = null;
  private generation = 0;
  private probeId = 0;
  private showDisconnected = false;
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
    this.showDisconnected = this.list.querySelector<HTMLDetailsElement>('details')?.open ?? this.showDisconnected;
    this.list.replaceChildren();
    const disconnected = document.createElement('details'); disconnected.className = 'disconnected-adapters'; disconnected.open = this.showDisconnected;
    const summary = document.createElement('summary');
    summary.textContent = `已断开的网卡（${inventory.adapters.filter(adapter => !adapter.connected && adapter.id !== inventory.preferredInterface).length}）`;
    disconnected.append(summary);
    disconnected.addEventListener('toggle', () => { this.showDisconnected = disconnected.open; });
    const disconnectedList = document.createElement('div'); disconnectedList.className = 'network-adapter-list'; disconnected.append(disconnectedList);
    let rowParent: HTMLElement = this.list;
    const addRow = (id: string, title: string, detail: string, chosen: boolean, available: boolean, active = false) => {
      const row = document.createElement('button'); row.type = 'button';
      row.className = `network-adapter-row${chosen ? ' chosen' : ''}${active ? ' in-use' : ''}`;
      row.dataset.adapterId = id; row.dataset.unavailable = String(!available);
      row.setAttribute('role', 'radio'); row.setAttribute('aria-checked', String(chosen)); row.tabIndex = chosen ? 0 : -1;
      const marker = document.createElement('span'); marker.className = 'adapter-radio-marker'; marker.setAttribute('aria-hidden', 'true');
      const text = document.createElement('span'); text.className = 'network-adapter-text'; const label = document.createElement('strong'); label.textContent = title;
      const description = document.createElement('small'); description.textContent = detail;
      text.append(label, description); row.append(marker, text); rowParent.append(row);
    };
    if (inventory.selectionSupported) addRow('', '自动选择', '优先校园有线，随当前连接自动识别', !inventory.preferredInterface, true);
    const adapters = [...inventory.adapters].sort((a, b) => Number(b.selected) - Number(a.selected) || Number(b.connected) - Number(a.connected));
    for (const adapter of adapters) {
      rowParent = !adapter.connected && adapter.id !== inventory.preferredInterface ? disconnectedList : this.list;
      const kind = ({ wifi: 'Wi-Fi', ethernet: '有线', vpn: '虚拟 / VPN', cellular: '移动数据' } as Record<string, string>)[adapter.transport] || adapter.transport;
      const title = adapter.name === kind ? adapter.name : `${adapter.name} · ${kind}`;
      addRow(adapter.id, `${title}${adapter.selected ? ' · 用于认证' : ''}`,
        `${adapter.interfaceName} · ${adapter.connected ? '已启用' : '已断开'} · IPv4 ${adapter.ipv4.join(' / ') || '--'} · IPv6 ${adapter.ipv6.length ? '已取得地址' : '--'}`,
        inventory.preferredInterface === adapter.id, adapter.selectable && adapter.connected && adapter.ipv4.length > 0, adapter.selected);
    }
    if (disconnectedList.childElementCount) this.list.append(disconnected);
    rowParent = this.list;
    if (inventory.preferredInterface && !inventory.adapters.some(adapter => adapter.id === inventory.preferredInterface)) {
      addRow(inventory.preferredInterface, '指定网卡当前不可用', '等待网卡恢复，或选择其他网卡', true, false);
    }
    const selected = inventory.adapters.find(adapter => adapter.selected);
    element('network-selection-caption').textContent = selected ? `当前认证网卡：${selected.name}${selected.name === selected.interfaceName ? '' : `（${selected.interfaceName}）`}`
      : inventory.preferredInterface ? '指定网卡不可用，认证已暂停' : '尚未取得可用于认证的网卡';
    if (!inventory.selectionSupported) this.showMessage('Android 由系统选择校园 Wi-Fi；下方同时展示其他网络。');
    this.updateBusy();
    if (focused !== undefined) Array.from(this.list.querySelectorAll<HTMLButtonElement>('button')).find(button => button.dataset.adapterId === focused && !button.disabled)?.focus({preventScroll:true});
  }

  renderState(state: NetworkStatePayload) {
    const identity = `${state.interfaceName || ''}|${state.ip || ''}`;
    const matchesIdentity = this.health?.interfaceName === state.interfaceName && this.health?.ipv4.addresses.includes(state.ip || '');
    if (this.identity && identity !== this.identity && !matchesIdentity) this.clearHealth();
    this.identity = identity;
  }

  resetGeneration(generation: number, probeId = 0) {
    if (generation < this.generation || (generation === this.generation && probeId < this.probeId)) return;
    this.generation = generation; this.probeId = probeId; this.clearHealth();
  }

  clearHealth() {
    this.health = null;
    for (const id of ['ipv4-health', 'ipv6-health']) {
      const node = element(id); node.className = 'family-status'; node.title = '等待重新检测';
      node.querySelector('strong')!.textContent = '检测中'; node.querySelector('.family-marker')!.textContent = '…';
    }
  }

  renderHealth(health: DualStackReport) {
    const generation = health.generation || 0; const probeId = health.probeId || 0;
    if (generation < this.generation || (generation === this.generation && probeId < this.probeId)) return;
    if (this.health && generation === this.health.generation && probeId === this.health.probeId && Date.parse(health.checkedAt) < Date.parse(this.health.checkedAt)) return;
    this.generation = generation; this.probeId = probeId;
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
