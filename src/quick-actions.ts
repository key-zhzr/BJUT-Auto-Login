import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { presentationDialog } from './presentation-dialog';
export const quickActionNames = ['dashboard', 'accounts', 'billing', 'check', 'login'] as const;
export function parseQuickAction(uri: string): string | null {
    // No credentials, query parameters, arbitrary URLs or commands in links.
    const match = /^bjut-al:\/\/(dashboard|accounts|billing|check|login)\/?$/.exec(uri);
    return match?.[1] || null;
}
export function setupQuickActions(navigate: (page: string) => void) {
    let confirming = false;
    const run = async (action: string) => {
        if (!quickActionNames.includes(action as typeof quickActionNames[number]))
            return;
        navigate(action === 'accounts' ? 'accounts' : action === 'billing' ? 'billing-center' : 'dashboard');
        if (action === 'check')
            await invoke('notify_network_change', { source: '系统快捷入口' });
        if (action === 'login' && !confirming) {
            confirming = true;
            const { form, close } = presentationDialog('shortcut-login-dialog', '登录校园网');
            form.addEventListener('presentation-close', () => { confirming = false; });
            const p = document.createElement('p');
            p.textContent = '使用控制台中当前选择的账号登录？';
            form.append(p);
            const actions = document.createElement('div');
            actions.className = 'modal-actions';
            for (const [label, login] of [['取消', false], ['登录', true]] as const) {
                const button = document.createElement('button');
                button.type = 'button';
                button.className = login ? 'btn btn-primary' : 'btn btn-secondary';
                button.textContent = label;
                button.onclick = () => { confirming = false; close(); if (login)
                    document.getElementById('btn-login')?.click(); };
                actions.append(button);
            }
            form.addEventListener('keydown', e => { if (e.key === 'Escape')
                confirming = false; });
            form.append(actions);
        }
    };
    window.__nativeQuickAction = action => { void run(window.AndroidBridge?.takeQuickAction?.() || action); };
    void listen<string>('quick-action', event => void run(event.payload));
    const pending = window.AndroidBridge?.takeQuickAction?.();
    if (pending)
        void run(pending);
    else if (window.__TAURI__)
        void invoke<string | null>('take_quick_action').then(action => { if (action)
            void run(action); }).catch(() => { });
}
