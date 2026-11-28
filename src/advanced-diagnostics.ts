import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { writeTextToClipboard } from './platform';
import { presentationDialog } from './presentation-dialog';

export function setupAdvancedDiagnostics() {
  document.getElementById('btn-issue-preview')?.addEventListener('click', async () => {
    const { form, close } = presentationDialog('issue-preview-dialog', '反馈问题');
    const note = document.createElement('p');
    note.className = 'transfer-note';
    note.textContent = '以下诊断信息已自动隐藏学号、密码与本机敏感地址，核对后可复制并前往 GitHub 提交反馈。';
    const text = document.createElement('textarea');
    text.className = 'issue-preview-textarea';
    text.rows = 12;
    text.readOnly = true;
    text.style.width = '100%';
    text.setAttribute('aria-label', '脱敏诊断信息');
    text.value = '正在准备脱敏诊断信息…';
    form.append(note, text);
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
    open.textContent = '复制并前往反馈';
    open.disabled = true;
    actions.append(cancel, open);
    form.append(actions);
    try {
      text.value = await invoke<string>('create_diagnostic_bundle');
      open.disabled = false;
    } catch {
      text.value = '请先运行一次网络诊断，再生成反馈信息。';
    }
    open.onclick = async () => {
      await writeTextToClipboard(text.value);
      await openUrl('https://github.com/key-zhzr/BJUT-Auto-Login/issues/new?title=' + encodeURIComponent('网络问题反馈'));
      close();
    };
  });
}
