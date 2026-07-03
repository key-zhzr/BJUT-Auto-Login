import re

with open('src/main.ts', 'r') as f:
    code = f.read()

# 1. Imports and Modals
header = """import { getCurrentWindow } from '@tauri-apps/api/window';

document.getElementById('titlebar-minimize')?.addEventListener('click', async () => {
  if ((window as any).__TAURI__) { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().minimize(); }
});
document.getElementById('titlebar-maximize')?.addEventListener('click', async () => {
  if ((window as any).__TAURI__) { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().toggleMaximize(); }
});
document.getElementById('titlebar-close')?.addEventListener('click', async () => {
  if ((window as any).__TAURI__) { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().close(); }
});

const ENCRYPT_KEY = 'bjut-al-secret-key-2026';
function encrypt(text: string): string {
  let res = '';
  for(let i=0; i<text.length; i++) res += String.fromCharCode(text.charCodeAt(i) ^ ENCRYPT_KEY.charCodeAt(i % ENCRYPT_KEY.length));
  return btoa(res);
}
function decrypt(base64: string): string {
  let res = '';
  try {
    const text = atob(base64);
    for(let i=0; i<text.length; i++) res += String.fromCharCode(text.charCodeAt(i) ^ ENCRYPT_KEY.charCodeAt(i % ENCRYPT_KEY.length));
  } catch(e) {}
  return res;
}
function getAccounts(): any[] {
  try {
    const raw = localStorage.getItem('bjut_accounts');
    if (!raw) return [];
    if (raw.startsWith('[')) {
      const parsed = JSON.parse(raw);
      saveAccounts(parsed);
      return parsed;
    }
    return JSON.parse(decrypt(raw));
  } catch(e) { return []; }
}
function saveAccounts(accs: any[]) {
  localStorage.setItem('bjut_accounts', encrypt(JSON.stringify(accs)));
}

function customAlert(text: string, title = '提示'): Promise<void> {
  return new Promise(resolve => {
    const modal = document.getElementById('alert-modal');
    if (!modal) { alert(text); resolve(); return; }
    document.getElementById('alert-modal-title')!.textContent = title;
    document.getElementById('alert-modal-text')!.textContent = text;
    const btnOk = document.getElementById('btn-alert-ok')!;
    const cleanup = () => {
      modal.classList.add('hidden');
      btnOk.removeEventListener('click', onOk);
    };
    const onOk = () => { cleanup(); resolve(); };
    btnOk.addEventListener('click', onOk);
    modal.classList.remove('hidden');
  });
}
function customConfirm(text: string, title = '确认'): Promise<boolean> {
  return new Promise(resolve => {
    const modal = document.getElementById('confirm-modal');
    if (!modal) { resolve(confirm(text)); return; }
    document.getElementById('confirm-modal-title')!.textContent = title;
    document.getElementById('confirm-modal-text')!.textContent = text;
    const btnOk = document.getElementById('btn-confirm-ok')!;
    const btnCancel = document.getElementById('btn-confirm-cancel')!;
    const cleanup = () => {
      modal.classList.add('hidden');
      btnOk.removeEventListener('click', onOk);
      btnCancel.removeEventListener('click', onCancel);
    };
    const onOk = () => { cleanup(); resolve(true); };
    const onCancel = () => { cleanup(); resolve(false); };
    btnOk.addEventListener('click', onOk);
    btnCancel.addEventListener('click', onCancel);
    modal.classList.remove('hidden');
  });
}
function showListManageModal(title: string, list: string[], onSave: (list: string[]) => void) {
  const modal = document.getElementById('list-manage-modal');
  if (!modal) { alert(list.join('\\n')); return; }
  document.getElementById('list-manage-title')!.textContent = title;
  const content = document.getElementById('list-manage-content')!;
  content.innerHTML = '';
  
  if (list.length === 0) {
    content.innerHTML = '<div style="color: var(--text-muted); padding: 0.5rem;">暂无数据</div>';
  } else {
    list.forEach((item, index) => {
      const div = document.createElement('div');
      div.style.display = 'flex';
      div.style.justifyContent = 'space-between';
      div.style.padding = '0.5rem';
      div.style.borderBottom = '1px solid var(--card-border)';
      div.innerHTML = `
        <span style="word-break: break-all;">${item}</span>
        <button class="btn-icon danger" style="padding: 0 0.5rem;" data-idx="${index}"><i data-lucide="trash-2"></i></button>
      `;
      content.appendChild(div);
    });
  }
  
  const closeBtn = document.getElementById('btn-list-manage-close')!;
  const cleanup = () => {
    modal.classList.add('hidden');
    content.removeEventListener('click', onClickList);
    closeBtn.removeEventListener('click', onClose);
  };
  const onClickList = (e: Event) => {
    const btn = (e.target as HTMLElement).closest('.danger');
    if (btn) {
      const idx = parseInt(btn.getAttribute('data-idx') || '0', 10);
      list.splice(idx, 1);
      onSave(list);
      showListManageModal(title, list, onSave);
    }
  };
  const onClose = () => { cleanup(); };
  content.addEventListener('click', onClickList);
  closeBtn.addEventListener('click', onClose);
  modal.classList.remove('hidden');
  createIcons({ icons });
}
"""

code = code.replace("import Sortable from 'sortablejs';", "import Sortable from 'sortablejs';\n" + header)

# remove global accounts
code = code.replace("let accounts: any[] = JSON.parse(localStorage.getItem('bjut_accounts') || '[]');", "")

# add modal replaces
code = code.replace("alert('该账号已存在');", "customAlert('该账号已存在');")
code = code.replace("alert('该账号名已存在');", "customAlert('该账号名已存在');")
code = code.replace("alert('请先在账号管理中添加账号');", "customAlert('请先在账号管理中添加账号');")
code = code.replace("alert('当前无需登录或未连接校园网');", "customAlert('当前无需登录或未连接校园网');")
code = code.replace("alert('【安卓后台保活提示】\\n为确保后台自动登录正常运行，请前往系统设置：\\n1. 授予本应用“通知”权限\\n2. 允许本应用“自启动”和“后台运行”\\n\\n我们将尝试发送常驻通知以防止程序被系统清理。');", "customAlert('【安卓后台保活提示】\\n为确保后台自动登录正常运行，请前往系统设置：\\n1. 授予本应用“通知”权限\\n2. 允许本应用“自启动”和“后台运行”\\n\\n我们将尝试发送常驻通知以防止程序被系统清理。');")
code = code.replace("alert(`正在检查更新 (通道: ${channel === 'release' ? '正式版' : '预览版'})...\\n当前已经是最新版本！`);", "customAlert(`正在检查更新 (通道: ${channel === 'release' ? '正式版' : '预览版'})...\\n当前已经是最新版本！`);")

code = code.replace("alert('当前信任的 WiFi (白名单):\\n' + (w.length ? w.join('\\n') : '暂无'));", "showListManageModal('信任的 WiFi (白名单)', w, (newList) => localStorage.setItem('bjut_whitelist', JSON.stringify(newList)));")
code = code.replace("alert('当前拒绝的 WiFi (黑名单):\\n' + (b.length ? b.join('\\n') : '暂无'));", "showListManageModal('拒绝的 WiFi (黑名单)', b, (newList) => localStorage.setItem('bjut_blacklist', JSON.stringify(newList)));")

# replace addAccount
code = code.replace(
"""    if (user && pass) {
      if (accounts.find(a => a.user === user)) {""",
"""    if (user && pass) {
      const accounts = getAccounts();
      if (accounts.find((a:any) => a.user === user)) {""")

code = code.replace(
"accounts.push({ user, pass, isDefault: accounts.length === 0 });\n      saveAccounts();",
"accounts.push({ user, pass, isDefault: accounts.length === 0 });\n      saveAccounts(accounts);"
)

# replace editAccount
code = code.replace(
"""    if (user && pass && !isNaN(index)) {
      if (accounts.findIndex((a, i) => a.user === user && i !== index) !== -1) {""",
"""    if (user && pass && !isNaN(index)) {
      const accounts = getAccounts();
      if (accounts.findIndex((a:any, i) => a.user === user && i !== index) !== -1) {""")

code = code.replace(
"accounts[index].user = user;\n      accounts[index].pass = pass;\n      saveAccounts();",
"accounts[index].user = user;\n      accounts[index].pass = pass;\n      saveAccounts(accounts);"
)

# delegate actions (disabled toggle)
code = code.replace(
"""        if (index !== -1) {
          accounts[index].isDisabled = !accounts[index].isDisabled;
          saveAccounts();""",
"""        if (index !== -1) {
          const accounts = getAccounts();
          accounts[index].isDisabled = !accounts[index].isDisabled;
          saveAccounts(accounts);""")

# delete confirm
code = code.replace(
"""        log('账号管理', `已删除账号 ${accounts[index].user}`);
        accounts.splice(index, 1);
        if (accounts.length > 0 && !accounts.find(a => a.isDefault)) {
          accounts[0].isDefault = true;
        }
        saveAccounts();""",
"""        const accounts = getAccounts();
        log('账号管理', `已删除账号 ${accounts[index].user}`);
        accounts.splice(index, 1);
        if (accounts.length > 0 && !accounts.find((a:any) => a.isDefault)) {
          accounts[0].isDefault = true;
        }
        saveAccounts(accounts);""")

# delete prepare
code = code.replace(
"""      if (deleteModal && deleteText) {
        deleteModal.setAttribute('data-delete-index', index.toString());
        deleteText.textContent = `确定要删除账号 ${accounts[index].user} 吗？`;""",
"""      if (deleteModal && deleteText) {
        const accounts = getAccounts();
        deleteModal.setAttribute('data-delete-index', index.toString());
        deleteText.textContent = `确定要删除账号 ${accounts[index].user} 吗？`;""")

# edit prepare
code = code.replace(
"""    } else if (btn.classList.contains('action-edit')) {
      editAccIndex.value = index.toString();
      editAccUsername.value = accounts[index].user;
      editAccPassword.value = accounts[index].pass;""",
"""    } else if (btn.classList.contains('action-edit')) {
      const accounts = getAccounts();
      editAccIndex.value = index.toString();
      editAccUsername.value = accounts[index].user;
      editAccPassword.value = accounts[index].pass;""")

# default prepare
code = code.replace(
"""      const account = accounts.splice(index, 1)[0];
      accounts.unshift(account);
      accounts.forEach((a, i) => a.isDefault = (i === 0));
      saveAccounts();""",
"""      const accounts = getAccounts();
      const account = accounts.splice(index, 1)[0];
      accounts.unshift(account);
      accounts.forEach((a:any, i:any) => a.isDefault = (i === 0));
      saveAccounts(accounts);""")

# renderAccounts
code = code.replace(
"function renderAccounts() {\n  accountsList.innerHTML = '';",
"function renderAccounts() {\n  const accounts = getAccounts();\n  accountsList.innerHTML = '';")

# updateOverrideOptions
code = code.replace(
"function updateOverrideOptions() {\n  const select = document.getElementById('override-account') as HTMLSelectElement;",
"function updateOverrideOptions() {\n  const accounts = getAccounts();\n  const select = document.getElementById('override-account') as HTMLSelectElement;")

# remove function saveAccounts()
code = code.replace(
"function saveAccounts() {\n  localStorage.setItem('bjut_accounts', JSON.stringify(accounts));\n}\n\n",
"")

# manual login top
code = code.replace(
"  if (accounts.length === 0) {\n    customAlert('请先在账号管理中添加账号');",
"  const accounts = getAccounts();\n  if (accounts.length === 0) {\n    customAlert('请先在账号管理中添加账号');")

# log scrolling fix
code = code.replace(
"""  setTimeout(() => {
    const container = document.querySelector('.logs-container');
    if (container) {
      container.scrollTop = container.scrollHeight;
    }
  }, 10);""",
"""  requestAnimationFrame(() => {
    entry.scrollIntoView({ behavior: 'smooth', block: 'end' });
  });""")

# sortable drag replace entire
old_sortable = """  Sortable.create(accountsList, {
    handle: '.drag-handle',
    animation: 300,
    easing: "cubic-bezier(0.25, 1, 0.5, 1)",
    ghostClass: 'dragging',
    forceFallback: true,
    fallbackClass: 'dragging-fallback',
    fallbackOnBody: true,"""

new_sortable = """  Sortable.create(accountsList, {
    handle: '.drag-handle',
    animation: 300,
    easing: "cubic-bezier(0.25, 1, 0.5, 1)",
    ghostClass: 'dragging',
    forceFallback: true,
    fallbackClass: 'dragging-fallback',
    fallbackOnBody: true,
    onEnd: (evt) => {
      const { oldIndex, newIndex } = evt;
      if (oldIndex !== undefined && newIndex !== undefined && oldIndex !== newIndex) {
        const accounts = getAccounts();
        const item = accounts.splice(oldIndex, 1)[0];
        accounts.splice(newIndex, 0, item);
        accounts.forEach((a:any, i) => a.isDefault = (i === 0));
        saveAccounts(accounts);
        renderAccounts();
        log('账号管理', '账号顺序已更新，最高优先级将作为默认账号');
      }
    }
  });
}

// Logging"""

# Replace Sortable up to Logging
sortable_regex = re.compile(r"  Sortable\.create\(accountsList, \{.*?\}\n  \}\);\n\}\n\n// Logging", re.DOTALL)
code = sortable_regex.sub(new_sortable, code)


import_export = """
  const btnExportConfig = document.getElementById('btn-export-config');
  const btnImportConfig = document.getElementById('btn-import-config');
  
  if (btnExportConfig) {
    btnExportConfig.addEventListener('click', async () => {
      const config = {
        accounts: getAccounts(),
        autoLogin: localStorage.getItem('bjut_auto_login'),
        checkInterval: localStorage.getItem('bjut_check_interval'),
        checkIntervalBg: localStorage.getItem('bjut_check_interval_bg'),
        whitelist: localStorage.getItem('bjut_whitelist'),
        blacklist: localStorage.getItem('bjut_blacklist'),
        moreOptions: localStorage.getItem('bjut_more_options')
      };
      const encrypted = encrypt(JSON.stringify(config));
      try {
        await navigator.clipboard.writeText(encrypted);
        customAlert('配置已加密并复制到剪贴板。');
      } catch (e) {
        customAlert('复制到剪贴板失败，请手动复制：\\n' + encrypted);
      }
    });
  }

  if (btnImportConfig) {
    btnImportConfig.addEventListener('click', async () => {
      try {
        const text = await navigator.clipboard.readText();
        if (!text) {
          customAlert('剪贴板为空');
          return;
        }
        const confirmResult = await customConfirm('导入配置将覆盖当前设置和账号，是否继续？');
        if (!confirmResult) return;
        
        const decrypted = decrypt(text.trim());
        if (!decrypted) {
          customAlert('无效的配置数据或解密失败');
          return;
        }
        
        const config = JSON.parse(decrypted);
        if (config.accounts) saveAccounts(config.accounts);
        if (config.autoLogin !== undefined && config.autoLogin !== null) localStorage.setItem('bjut_auto_login', config.autoLogin);
        if (config.checkInterval !== undefined && config.checkInterval !== null) localStorage.setItem('bjut_check_interval', config.checkInterval);
        if (config.checkIntervalBg !== undefined && config.checkIntervalBg !== null) localStorage.setItem('bjut_check_interval_bg', config.checkIntervalBg);
        if (config.whitelist) localStorage.setItem('bjut_whitelist', config.whitelist);
        if (config.blacklist) localStorage.setItem('bjut_blacklist', config.blacklist);
        if (config.moreOptions !== undefined && config.moreOptions !== null) localStorage.setItem('bjut_more_options', config.moreOptions);
        
        customAlert('导入成功，请刷新或重启应用以应用更改！');
        setTimeout(() => location.reload(), 1500);
      } catch (e) {
        customAlert('导入失败：' + String(e));
      }
    });
  }

  // Password visibility toggle"""

code = code.replace("// Password visibility toggle", import_export)

with open('src/main.ts', 'w') as f:
    f.write(code)

