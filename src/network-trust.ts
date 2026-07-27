import { invoke } from '@tauri-apps/api/core';

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
