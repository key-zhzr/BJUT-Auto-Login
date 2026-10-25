import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { writeTextToClipboard } from './platform';
import { presentationDialog } from './presentation-dialog';
import { animateResize } from './layout-motion';
export function setupAdvancedDiagnostics() {
    document.getElementById('btn-route-trace')?.addEventListener('click', () => {
        const { form, close } = presentationDialog('route-trace-dialog', '链路检查');
        const note = document.createElement('p');
        note.className = 'transfer-note';
        note.textContent = '查看到指定目标的中间节点。节点未响应不一定表示链路故障。';
        form.append(note);
        const actions = document.createElement('div');
        actions.className = 'device-usage-filters';
        const result = document.createElement('div');
        result.setAttribute('aria-live', 'polite');
        form.append(actions, result);
        for (const [key, label] of [['campus', '校园网关'], ['billing', '计费系统'], ['internet4', 'IPv4 外网'], ['internet6', 'IPv6 外网']]) {
            const button = document.createElement('button');
            button.type = 'button';
            button.className = 'btn btn-secondary btn-sm';
            button.textContent = label;
            button.onclick = async () => {
                actions.querySelectorAll('button').forEach(btn => btn.disabled = true);
                animateResize(result, () => { result.textContent = '正在检查，最长约 14 秒…'; });
                try {
                    const data = await invoke<{
                        target: string;
                        available: boolean;
                        timedOut: boolean;
                        hops: {
                            index: number;
                            address?: string;
                            detail: string;
                        }[];
                        output: string;
                    }>('trace_network', { targetKey: key });
                    if (!form.isConnected)
                        return;
                    animateResize(result, () => { result.replaceChildren(); for (const hop of data.hops) {
                        const row = document.createElement('div');
                        row.className = 'trace-hop';
                        const step = document.createElement('strong');
                        step.textContent = String(hop.index);
                        const detail = document.createElement('span');
                        detail.textContent = hop.address || '未收到响应';
                        const timing = document.createElement('small');
                        timing.textContent = hop.detail;
                        row.append(step, detail, timing);
                        result.append(row);
                    } if (!data.hops.length)
                        result.textContent = data.output || '暂未取得中间节点信息。'; if (data.timedOut) {
                        const p = document.createElement('p');
                        p.textContent = '已达到检查时限，以上为已取得的节点。';
                        result.append(p);
                    } });
                }
                catch (error) {
                    if (form.isConnected)
                        result.textContent = String(error);
                }
                finally {
                    actions.querySelectorAll('button').forEach(btn => btn.disabled = false);
                }
            };
            actions.append(button);
        }
        const done = document.createElement('button');
        done.type = 'button';
        done.className = 'btn btn-secondary';
        done.textContent = '关闭';
        done.onclick = close;
        form.append(done);
    });
    document.getElementById('btn-issue-preview')?.addEventListener('click', async () => {
        const { form, close } = presentationDialog('issue-preview-dialog', '反馈预览');
        const text = document.createElement('textarea');
        text.rows = 12;
        text.readOnly = true;
        text.style.width = '100%';
        text.setAttribute('aria-label', '脱敏诊断信息');
        text.value = '正在准备脱敏信息…';
        form.append(text);
        const actions = document.createElement('div');
        actions.className = 'modal-actions';
        const cancel = document.createElement('button');
        cancel.type = 'button';
        cancel.className = 'btn btn-secondary';
        cancel.textContent = '取消';
        cancel.onclick = close;
        const open = document.createElement('button');
        open.type = 'button';
        open.className = 'btn btn-primary';
        open.textContent = '复制并打开反馈页';
        open.disabled = true;
        actions.append(cancel, open);
        form.append(actions);
        try {
            text.value = await invoke<string>('create_diagnostic_bundle');
            open.disabled = false;
        }
        catch {
            text.value = '请先运行网络诊断，再复制报告。';
        }
        open.onclick = async () => { await writeTextToClipboard(text.value); await openUrl('https://github.com/key-zhzr/BJUT-Auto-Login/issues/new?title=' + encodeURIComponent('网络问题反馈')); close(); };
    });
}
