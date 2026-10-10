import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { CustomSelect } from './custom-select';
import type { AccountView } from './models';

interface RouterUplink {
  id: string; network?: string; network6?: string; device?: string; ipv4?: string; ipv6?: string;
  state: string; message?: string; internet4?: string; internet6?: string; account?: string;
}
interface RouterSnapshot {
  version: string; uplinks: RouterUplink[];
  accounts: { id: string; user: string; enabled: number; hasPassword: number }[];
  managementUrl: string;
}

const labels: Record<string, string> = {
  online: '互联网已连接', busy: '此出口正在检测', authenticated_limited: '认证有效，外网探测受限',
  probe_uncertain: '连通性待确认', offline_suspected: '正在复核', waiting_interface: '等待接口',
  cooldown: '等待重试', auth_rejected: '账号未通过', portal_not_found: '未发现认证网关',
  portal_not_ready: '等待认证网关', no_account: '请选择账号', stopped: '尚未运行',
  login_accepted: '登录已受理', blocked: '请检查上游设置', request_failed: '认证结果待确认',
};

export function setupRouterControl(getAccounts: () => AccountView[], log: (message: string) => void): void {
  const panel = document.getElementById('router-control')!;
  const form = document.getElementById('router-connect-form') as HTMLFormElement;
  const address = document.getElementById('router-address') as HTMLInputElement;
  const username = document.getElementById('router-username') as HTMLInputElement;
  const password = document.getElementById('router-password') as HTMLInputElement;
  const certificate = document.getElementById('router-certificate') as HTMLTextAreaElement;
  const message = document.getElementById('router-message')!;
  const status = document.getElementById('router-uplink-status')!;
  const connectedPanel = document.getElementById('router-connected')!;
  const connectButton = document.getElementById('btn-router-connect') as HTMLButtonElement;
  const syncButton = document.getElementById('btn-router-sync') as HTMLButtonElement;
  const localAccounts = document.getElementById('router-sync-accounts')!;
  const uplink = new CustomSelect('router-uplink-select');
  const account = new CustomSelect('router-account-select');
  let connected = false;
  let busy = false;
  let snapshot: RouterSnapshot | null = null;
  let optionsStamp = '';
  address.value = localStorage.getItem('bjut_router_address') || 'https://192.168.1.1';
  username.value = localStorage.getItem('bjut_router_username') || 'root';
  certificate.value = localStorage.getItem('bjut_router_certificate') || '';

  function showMessage(text: string, failed = false) {
    message.textContent = text;
    message.classList.toggle('error', failed);
  }
  function render(data: RouterSnapshot) {
    snapshot = data;
    connected = true; connectedPanel.hidden = false;
    document.getElementById('router-version')!.textContent = `路由端 v${data.version}`;
    const stamp = JSON.stringify([data.uplinks.map(item => [item.id, item.network]), data.accounts]);
    if (stamp !== optionsStamp) {
      const previous = uplink.value;
      uplink.setOptions(data.uplinks.length ? data.uplinks.map(item => ({ value: item.id, text: [item.id, item.network].filter(Boolean).join(' · ') })) : [{value:'',text:'请先配置上游'}]);
      uplink.setDisabled(!data.uplinks.length);
      uplink.value = data.uplinks.some(item => item.id === previous) ? previous : data.uplinks[0]?.id || '';
      const previousAccount = account.value;
      account.setOptions([{ value: '', text: '自动选择账号' }, ...data.accounts.filter(item => item.hasPassword).map(item => ({ value: item.id, text: item.user + (item.enabled ? '' : '（手动）') }))]);
      account.value = data.accounts.some(item => item.id === previousAccount) ? previousAccount : '';
      optionsStamp = stamp;
    }
    status.replaceChildren(...data.uplinks.map(item => {
      const card = document.createElement('div'); card.className = 'router-uplink-card';
      const title = document.createElement('strong'); title.textContent = [item.id, item.network, labels[item.state] || item.state].filter(Boolean).join(' · ');
      const detail = document.createElement('p'); detail.textContent = item.message || '';
      const paths = document.createElement('p');
      const family = (name: string, state?: string) => `${state === 'reachable' ? '✓' : state === 'not_configured' ? '—' : '×'} ${name}`;
      paths.textContent = `${family('IPv4', item.internet4)} · ${family('IPv6', item.internet6)}${item.account ? ` · ${item.account}` : ''}`;
      card.append(title, detail, paths); return card;
    }));
  }
  async function refresh() {
    if (!connected || busy) return;
    busy = true;
    try { render(await invoke<RouterSnapshot>('get_router_status')); }
    catch (error) { showMessage(String(error), true); (document.getElementById('router-connection-settings') as HTMLDetailsElement).open = true; }
    finally { busy = false; }
  }
  form.addEventListener('submit', async event => {
    event.preventDefault(); if (busy) return;
    busy = true; connectButton.disabled = true; showMessage('正在连接路由器…');
    try {
      const data = await invoke<RouterSnapshot>('connect_router', { address: address.value.trim(), username: username.value.trim(), password: password.value, certificate: certificate.value.trim() || null });
      password.value = '';
      localStorage.setItem('bjut_router_address', address.value.trim());
      localStorage.setItem('bjut_router_username', username.value.trim());
      localStorage.setItem('bjut_router_certificate', certificate.value.trim());
      render(data); (document.getElementById('router-connection-settings') as HTMLDetailsElement).open = false; showMessage('路由器已连接'); log('路由器管理会话已连接');
    } catch (error) { showMessage(String(error), true); }
    finally { busy = false; connectButton.disabled = false; }
  });
  document.getElementById('btn-router-refresh')!.addEventListener('click', () => { void refresh(); });
  document.getElementById('btn-router-disconnect')!.addEventListener('click', async () => {
    connected = false; snapshot = null; connectedPanel.hidden = true;
    await invoke('disconnect_router'); showMessage('已断开路由管理连接');
  });
  document.getElementById('btn-router-login')!.addEventListener('click', async () => {
    if (!connected || busy || !uplink.value) return;
    busy = true;
    try {
      const result = await invoke<string>('router_login', { uplink: uplink.value, account: account.value || null });
      showMessage(result); log(result);
      window.setTimeout(() => { void refresh(); }, 3000);
    } catch (error) { showMessage(String(error), true); }
    finally { busy = false; }
  });
  document.getElementById('btn-router-billing')!.addEventListener('click', () => {
    if (snapshot) void openUrl(snapshot.managementUrl).catch(error => showMessage(String(error), true));
  });
  document.getElementById('router-sync-details')!.addEventListener('toggle', event => {
    if (!(event.target as HTMLDetailsElement).open) return;
    const checked = new Set(Array.from(localAccounts.querySelectorAll<HTMLInputElement>('input:checked')).map(input => input.value));
    localAccounts.replaceChildren(...getAccounts().filter(item => item.hasPassword).map(item => {
      const label = document.createElement('label'); label.className = 'router-sync-account';
      const input = document.createElement('input'); input.type = 'checkbox'; input.value = item.user; input.checked = checked.has(item.user);
      const text = document.createElement('span'); text.textContent = item.user + (item.isDisabled ? '（自动登录已停用）' : '');
      label.append(input, text); return label;
    }));
    if (!localAccounts.childElementCount) localAccounts.textContent = '请先在账号管理中保存账号密码。';
  });
  syncButton.addEventListener('click', async () => {
    if (!connected || busy) return;
    const users = Array.from(localAccounts.querySelectorAll<HTMLInputElement>('input:checked')).map(input => input.value);
    if (!users.length) { showMessage('请勾选要同步的账号'); return; }
    busy = true; syncButton.disabled = true;
    try {
      const result = await invoke<string>('sync_router_accounts', { users }); showMessage(result); log(result);
      optionsStamp = ''; render(await invoke<RouterSnapshot>('get_router_status'));
    } catch (error) { showMessage(String(error), true); }
    finally { busy = false; syncButton.disabled = false; }
  });
  window.setInterval(() => {
    if (!document.hidden && panel.closest('.page')?.classList.contains('active')) void refresh();
  }, 10000);
}
