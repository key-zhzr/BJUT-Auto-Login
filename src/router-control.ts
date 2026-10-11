import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { CustomSelect } from './custom-select';
import type { AccountView } from './models';
import type { RouterSnapshot } from './router-model';
import { RouterWorkbench } from './router-workbench';

export function setupRouterControl(getAccounts: () => AccountView[], log: (message: string) => void,
  confirmCertificate: (text: string, title: string, confirmLabel: string) => Promise<boolean>): RouterWorkbench {
  const form = document.getElementById('router-connect-form') as HTMLFormElement;
  const address = document.getElementById('router-address') as HTMLInputElement;
  const username = document.getElementById('router-username') as HTMLInputElement;
  const password = document.getElementById('router-password') as HTMLInputElement;
  const certificate = document.getElementById('router-certificate') as HTMLTextAreaElement;
  const allowHttp = document.getElementById('router-allow-http') as HTMLInputElement;
  const modal = document.getElementById('router-connect-modal')!;
  const message = document.getElementById('router-message')!;
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
  let lastPoll = 0;
  let connectionGeneration = 0;
  address.value = localStorage.getItem('bjut_router_address') || 'https://192.168.1.1';
  username.value = localStorage.getItem('bjut_router_username') || 'root';
  certificate.value = localStorage.getItem('bjut_router_certificate') || '';
  allowHttp.checked = localStorage.getItem('bjut_router_allow_http') === 'true';
  let returnFocus: HTMLElement | null = null;
  function openConnection(endpoint?: string) {
    if (endpoint) address.value = endpoint;
    returnFocus = document.activeElement as HTMLElement | null;
    (document.getElementById('router-connection-settings') as HTMLDetailsElement).open = true;
    modal.classList.remove('hidden');
    window.setTimeout(() => password.focus({preventScroll:true}), 150);
  }
  function closeConnection() {
    modal.classList.add('hidden'); returnFocus?.focus({preventScroll:true});
  }
  document.getElementById('btn-router-connect-cancel')!.addEventListener('click', closeConnection);
  modal.addEventListener('keydown', event => { if (event.key === 'Escape') { event.stopPropagation(); closeConnection(); } });
  const workbench = new RouterWorkbench({connect:openConnection,refresh,login:loginUplink,log});

  function showMessage(text: string, failed = false) {
    message.textContent = text.replace('ROUTER_CERT_UNTRUSTED:', '');
    message.classList.toggle('error', failed);
    document.getElementById('router-connect-message')!.textContent = message.textContent;
    if (failed && connected) {
      document.getElementById('router-discovery-message')!.textContent = message.textContent;
      document.getElementById('btn-router-auto-connect')!.hidden = false;
    }
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
    workbench.render(data);
  }
  async function refresh() {
    if (!connected || busy) return;
    busy = true;
    const generation = connectionGeneration;
    lastPoll = Date.now();
    try {
      const data = await invoke<RouterSnapshot>('get_router_status');
      if (connected && generation === connectionGeneration) render(data);
    }
    catch (error) {
      if (connected && generation === connectionGeneration) {
        showMessage(String(error), true); (document.getElementById('router-connection-settings') as HTMLDetailsElement).open = true;
      }
    }
    finally { busy = false; }
  }
  form.addEventListener('submit', async event => {
    event.preventDefault(); if (busy) return;
    const generation = ++connectionGeneration;
    busy = true; connectButton.disabled = true; showMessage('正在连接路由器…');
    try {
      const connection = { address: address.value.trim(), username: username.value.trim(), password: password.value, certificate: certificate.value.trim() || null, allowHttp:allowHttp.checked };
      let data: RouterSnapshot;
      try { data = await invoke<RouterSnapshot>('connect_router', connection); }
      catch (error) {
        if (!String(error).includes('ROUTER_CERT_UNTRUSTED:') || connection.certificate) throw error;
        showMessage('正在读取路由器证书…');
        const cert = await invoke<{pem: string; fingerprint: string; address: string}>('probe_router_certificate', { address: connection.address });
        const accepted = await confirmCertificate(`地址：${cert.address}\nSHA-256 指纹：\n${cert.fingerprint}\n\n请核对这是你的路由器。确认后将记住此证书。`, '确认路由器证书', '信任并连接');
        if (!accepted) throw new Error('未信任路由器证书');
        if (generation !== connectionGeneration) return;
        if (address.value.trim() !== connection.address) throw new Error('路由器地址已更改，请重新连接');
        certificate.value = cert.pem; connection.certificate = cert.pem;
        showMessage('正在连接路由器…');
        data = await invoke<RouterSnapshot>('connect_router', connection);
      }
      if (generation !== connectionGeneration) { await invoke('disconnect_router'); return; }
      password.value = '';
      localStorage.setItem('bjut_router_address', connection.address);
      localStorage.setItem('bjut_router_username', connection.username);
      localStorage.setItem('bjut_router_certificate', connection.certificate || '');
      localStorage.setItem('bjut_router_allow_http', String(connection.allowHttp));
      render(data); closeConnection(); showMessage('路由器已连接'); log('路由器管理会话已连接');
    } catch (error) { if (generation === connectionGeneration) showMessage(String(error), true); }
    finally { busy = false; connectButton.disabled = false; }
  });
  document.getElementById('btn-router-refresh')!.addEventListener('click', () => { void refresh(); });
  document.getElementById('btn-router-disconnect')!.addEventListener('click', async () => {
    const generation = ++connectionGeneration;
    connected = false; snapshot = null; connectedPanel.hidden = true;
    workbench.disconnect();
    await invoke('disconnect_router');
    if (generation === connectionGeneration) showMessage('已断开路由管理连接');
  });
  async function loginUplink(target: string, selectedAccount?: string) {
    if (!connected || busy || !target) return;
    const generation = connectionGeneration;
    busy = true;
    try {
      const result = await invoke<string>('router_login', { uplink: target, account: selectedAccount || account.value || null });
      if (!connected || generation !== connectionGeneration) return;
      showMessage(result); log(result);
      window.setTimeout(() => { void refresh(); }, 3000);
    } catch (error) { if (connected && generation === connectionGeneration) showMessage(String(error), true); }
    finally { busy = false; }
  }
  document.getElementById('btn-router-login')!.addEventListener('click', () => { void loginUplink(uplink.value); });
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
    const generation = connectionGeneration;
    const users = Array.from(localAccounts.querySelectorAll<HTMLInputElement>('input:checked')).map(input => input.value);
    if (!users.length) { showMessage('请勾选要同步的账号'); return; }
    busy = true; syncButton.disabled = true;
    try {
      const result = await invoke<string>('sync_router_accounts', { users }); showMessage(result); log(result);
      const data = await invoke<RouterSnapshot>('get_router_status');
      if (connected && generation === connectionGeneration) { optionsStamp = ''; render(data); }
    } catch (error) { if (connected && generation === connectionGeneration) showMessage(String(error), true); }
    finally { busy = false; syncButton.disabled = false; }
  });
  window.setInterval(() => {
    const interval = snapshot?.uplinks.some(item => item.checking) ? 2000 : 10000;
    if (!document.hidden && connected && Date.now() - lastPoll >= interval) void refresh();
  }, 2000);
  return workbench;
}
