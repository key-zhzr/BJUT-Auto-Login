import { invoke } from '@tauri-apps/api/core';
import { UI_TEXT } from './ui-text';

export interface NetworkTrustEvaluation {
  decision: 'allowed' | 'blocked' | 'confirm';
  reason: string;
  networkKey: string;
  ssid: string;
  bssid: string;
  ip: string;
}

export interface NetworkTrustLists {
  whitelist: string[];
  blacklist: string[];
}

export interface NetworkTrustApproval {
  allowed: boolean;
  trustOnce: boolean;
  networkKey: string | null;
}

interface NetworkTrustControllerOptions {
  loginTypeOverride: string | null;
  onLog: (module: string, message: string, type: 'info' | 'error') => void;
  onAlert: (message: string, title: string) => Promise<void>;
  onListsChanged: (lists: NetworkTrustLists) => void;
}

export async function requestNetworkTrustApproval({
  loginTypeOverride,
  onLog,
  onAlert,
  onListsChanged,
}: NetworkTrustControllerOptions): Promise<NetworkTrustApproval> {
  if (!window.__TAURI__) return { allowed: true, trustOnce: false, networkKey: null };

  try {
    const evaluation = await invoke<NetworkTrustEvaluation>('evaluate_manual_network_trust', {
      loginTypeOverride,
    });
    if (evaluation.decision === 'allowed') {
      return { allowed: true, trustOnce: false, networkKey: null };
    }
    if (evaluation.decision === 'blocked') {
      onLog('安全', `网络安全检查已阻止登录：${evaluation.reason}`, 'error');
      await onAlert(evaluation.reason, '网络安全检查');
      return { allowed: false, trustOnce: false, networkKey: null };
    }

    return await new Promise<NetworkTrustApproval>(resolve => {
      const modal = document.getElementById('security-modal')!;
      document.getElementById('sec-ssid')!.textContent = evaluation.ssid;
      document.getElementById('sec-bssid')!.textContent = evaluation.bssid;
      document.getElementById('sec-ip')!.textContent = evaluation.ip;

      const cancelButton = document.getElementById('btn-sec-cancel')!;
      const blacklistButton = document.getElementById('btn-sec-cancel-black')!;
      const onceButton = document.getElementById('btn-sec-trust-once')!;
      const whitelistButton = document.getElementById('btn-sec-trust-white')!;
      const cleanup = () => {
        modal.classList.add('hidden');
        cancelButton.removeEventListener('click', cancel);
        blacklistButton.removeEventListener('click', blacklist);
        onceButton.removeEventListener('click', trustOnce);
        whitelistButton.removeEventListener('click', whitelist);
      };
      const finish = (approval: NetworkTrustApproval) => {
        cleanup();
        resolve(approval);
      };
      const cancel = () => finish({ allowed: false, trustOnce: false, networkKey: null });
      const trustOnce = () => finish({
        allowed: true,
        trustOnce: true,
        networkKey: evaluation.networkKey,
      });
      const setTrust = async (trusted: boolean): Promise<NetworkTrustLists> => {
        const lists = await invoke<NetworkTrustLists>('set_current_network_trust', {
          trusted,
          expectedNetworkKey: evaluation.networkKey,
        });
        onListsChanged(lists);
        return lists;
      };
      const blacklist = async () => {
        try {
          await setTrust(false);
          onLog('安全', `已将 ${evaluation.ssid} 加入黑名单`, 'info');
        } catch (error) {
          await onAlert(`无法更新网络信任设置：${String(error)}`, '网络安全检查');
        }
        finish({ allowed: false, trustOnce: false, networkKey: null });
      };
      const whitelist = async () => {
        try {
          await setTrust(true);
          onLog('安全', `已将 ${evaluation.ssid} 加入白名单`, 'info');
          finish({ allowed: true, trustOnce: false, networkKey: null });
        } catch (error) {
          await onAlert(`无法更新网络信任设置：${String(error)}`, '网络安全检查');
          finish({ allowed: false, trustOnce: false, networkKey: null });
        }
      };

      cancelButton.addEventListener('click', cancel);
      blacklistButton.addEventListener('click', blacklist);
      onceButton.addEventListener('click', trustOnce);
      whitelistButton.addEventListener('click', whitelist);
      modal.classList.remove('hidden');
    });
  } catch (error) {
    console.error('Security check error', error);
    onLog('安全', '网络安全检查失败，已阻止发送账号密码', 'error');
    return { allowed: false, trustOnce: false, networkKey: null };
  }
}

interface NetworkTrustListOptions {
  trusted: boolean;
  whitelist: string[];
  blacklist: string[];
  onListsChanged: (lists: NetworkTrustLists) => void;
  renderIcons: (root: HTMLElement) => void;
}

export function showNetworkTrustListModal({
  trusted,
  whitelist,
  blacklist,
  onListsChanged,
  renderIcons,
}: NetworkTrustListOptions) {
  const modal = document.getElementById('list-manage-modal');
  if (!modal) return;
  document.getElementById('list-manage-title')!.textContent = trusted
    ? UI_TEXT.networkTrust.whitelistTitle
    : UI_TEXT.networkTrust.blacklistTitle;
  document.getElementById('list-manage-description')!.textContent = trusted
    ? UI_TEXT.networkTrust.whitelistDescription
    : UI_TEXT.networkTrust.blacklistDescription;
  const content = document.getElementById('list-manage-content')!;
  const status = document.getElementById('list-manage-status')!;
  const currentButton = document.getElementById('btn-list-add-current') as HTMLButtonElement;
  const form = document.getElementById('list-manage-add-form') as HTMLFormElement;
  const ssidInput = document.getElementById('list-manage-ssid') as HTMLInputElement;
  const bssidInput = document.getElementById('list-manage-bssid') as HTMLInputElement;
  const closeButton = document.getElementById('btn-list-manage-close')!;
  let lists: NetworkTrustLists = {
    whitelist: [...whitelist],
    blacklist: [...blacklist],
  };
  const activeList = () => trusted ? lists.whitelist : lists.blacklist;
  const applyLists = (next: NetworkTrustLists) => {
    lists = {
      whitelist: [...next.whitelist],
      blacklist: [...next.blacklist],
    };
    onListsChanged(lists);
  };
  const setBusy = (busy: boolean) => {
    currentButton.disabled = busy;
    form.querySelectorAll<HTMLButtonElement | HTMLInputElement>('button, input')
      .forEach(element => { element.disabled = busy; });
  };
  const renderList = () => {
    content.replaceChildren();
    if (activeList().length === 0) {
      const empty = document.createElement('div');
      empty.className = 'diagnostic-empty';
      empty.textContent = '暂无记录';
      content.appendChild(empty);
      return;
    }
    activeList().forEach(item => {
      const row = document.createElement('div');
      row.className = 'network-trust-list-item';
      const label = document.createElement('span');
      label.textContent = item;
      const removeButton = document.createElement('button');
      removeButton.className = 'btn-icon danger';
      removeButton.dataset.networkKey = item;
      removeButton.setAttribute('aria-label', '删除');
      removeButton.innerHTML = '<i data-lucide="trash-2"></i>';
      row.append(label, removeButton);
      content.appendChild(row);
    });
    renderIcons(content);
  };
  const cleanup = () => {
    modal.classList.add('hidden');
    content.removeEventListener('click', onRemove);
    currentButton.removeEventListener('click', onAddCurrent);
    form.removeEventListener('submit', onAddOther);
    closeButton.removeEventListener('click', cleanup);
  };
  const onRemove = async (event: Event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>('[data-network-key]');
    if (!button?.dataset.networkKey) return;
    setBusy(true);
    status.textContent = '正在删除…';
    try {
      applyLists(await invoke<NetworkTrustLists>('remove_saved_network_trust', {
        networkKey: button.dataset.networkKey,
      }));
      status.textContent = '已删除该 Wi-Fi 记录。';
      renderList();
    } catch (error) {
      status.textContent = `删除失败：${String(error)}`;
    } finally {
      setBusy(false);
    }
  };
  const onAddCurrent = async () => {
    setBusy(true);
    status.textContent = '正在读取当前 Wi-Fi…';
    try {
      applyLists(await invoke<NetworkTrustLists>('set_current_network_trust', {
        trusted,
        expectedNetworkKey: null,
      }));
      status.textContent = `已将当前 Wi-Fi 加入${trusted ? '白名单' : '黑名单'}。`;
      renderList();
    } catch (error) {
      status.textContent = `无法添加当前 Wi-Fi：${String(error)}`;
    } finally {
      setBusy(false);
    }
  };
  const onAddOther = async (event: SubmitEvent) => {
    event.preventDefault();
    const ssid = ssidInput.value.trim();
    const bssid = bssidInput.value.trim();
    if (!ssid || !bssid) {
      status.textContent = '添加其他 Wi-Fi 时需要同时填写 SSID 与 BSSID。';
      return;
    }
    setBusy(true);
    status.textContent = '正在保存…';
    try {
      applyLists(await invoke<NetworkTrustLists>('set_named_network_trust', {
        trusted,
        ssid,
        bssid,
      }));
      form.reset();
      status.textContent = `已将 ${ssid} 加入${trusted ? '白名单' : '黑名单'}。`;
      renderList();
    } catch (error) {
      status.textContent = `保存失败：${String(error)}`;
    } finally {
      setBusy(false);
    }
  };

  renderList();
  status.textContent = '';
  form.reset();
  content.addEventListener('click', onRemove);
  currentButton.addEventListener('click', onAddCurrent);
  form.addEventListener('submit', onAddOther);
  closeButton.addEventListener('click', cleanup);
  modal.classList.remove('hidden');
  renderIcons(modal);
}
