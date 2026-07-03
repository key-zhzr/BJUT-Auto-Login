const fs = require('fs');
let code = fs.readFileSync('src/main.ts', 'utf8');

// 1. Titlebar logic
const titlebarLogic = `
import { getCurrentWindow } from '@tauri-apps/api/window';
// Titlebar setup
document.getElementById('titlebar-minimize')?.addEventListener('click', async () => {
  if (window.__TAURI__) { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().minimize(); }
});
document.getElementById('titlebar-maximize')?.addEventListener('click', async () => {
  if (window.__TAURI__) { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().toggleMaximize(); }
});
document.getElementById('titlebar-close')?.addEventListener('click', async () => {
  if (window.__TAURI__) { const { getCurrentWindow } = await import('@tauri-apps/api/window'); getCurrentWindow().close(); }
});
`;

code = code.replace("import { createIcons, icons } from 'lucide';", "import { createIcons, icons } from 'lucide';\n" + titlebarLogic);

// 2. Encryption and Custom Modals
const helpers = `
const ENCRYPT_KEY = 'bjut-al-secret-key-2026';
function encrypt(text) {
  let res = '';
  for(let i=0; i<text.length; i++) res += String.fromCharCode(text.charCodeAt(i) ^ ENCRYPT_KEY.charCodeAt(i % ENCRYPT_KEY.length));
  return btoa(res);
}
function decrypt(base64) {
  let res = '';
  try {
    const text = atob(base64);
    for(let i=0; i<text.length; i++) res += String.fromCharCode(text.charCodeAt(i) ^ ENCRYPT_KEY.charCodeAt(i % ENCRYPT_KEY.length));
  } catch(e) {}
  return res;
}
function getAccounts() {
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
function saveAccounts(accs) {
  localStorage.setItem('bjut_accounts', encrypt(JSON.stringify(accs)));
}

function customAlert(text, title = '提示') {
  return new Promise(resolve => {
    const modal = document.getElementById('alert-modal');
    if (!modal) { alert(text); resolve(); return; }
    document.getElementById('alert-modal-title').textContent = title;
    document.getElementById('alert-modal-text').textContent = text;
    const btnOk = document.getElementById('btn-alert-ok');
    const cleanup = () => {
      modal.classList.add('hidden');
      btnOk.removeEventListener('click', onOk);
    };
    const onOk = () => { cleanup(); resolve(); };
    btnOk.addEventListener('click', onOk);
    modal.classList.remove('hidden');
  });
}
function customConfirm(text, title = '确认') {
  return new Promise(resolve => {
    const modal = document.getElementById('confirm-modal');
    if (!modal) { resolve(confirm(text)); return; }
    document.getElementById('confirm-modal-title').textContent = title;
    document.getElementById('confirm-modal-text').textContent = text;
    const btnOk = document.getElementById('btn-confirm-ok');
    const btnCancel = document.getElementById('btn-confirm-cancel');
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
function showListManageModal(title, list, onSave) {
  const modal = document.getElementById('list-manage-modal');
  if (!modal) { alert(list.join('\\n')); return; }
  document.getElementById('list-manage-title').textContent = title;
  const content = document.getElementById('list-manage-content');
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
      div.innerHTML = \`
        <span style="word-break: break-all;">\${item}</span>
        <button class="btn-icon danger" style="padding: 0 0.5rem;" data-idx="\${index}"><i data-lucide="trash-2"></i></button>
      \`;
      content.appendChild(div);
    });
  }
  
  const closeBtn = document.getElementById('btn-list-manage-close');
  const cleanup = () => {
    modal.classList.add('hidden');
    content.removeEventListener('click', onClickList);
    closeBtn.removeEventListener('click', onClose);
  };
  const onClickList = (e) => {
    const btn = e.target.closest('.danger');
    if (btn) {
      const idx = parseInt(btn.getAttribute('data-idx'));
      list.splice(idx, 1);
      onSave(list);
      showListManageModal(title, list, onSave); // re-render
    }
  };
  const onClose = () => { cleanup(); };
  content.addEventListener('click', onClickList);
  closeBtn.addEventListener('click', onClose);
  modal.classList.remove('hidden');
  createIcons({ icons });
}
`;

code = code.replace("let accounts: any[] = JSON.parse(localStorage.getItem('bjut_accounts') || '[]');", helpers);

code = code.replace(
  "if (accounts.find(a => a.user === user)) {",
  "const accounts = getAccounts();\n      if (accounts.find((a:any) => a.user === user)) {"
);
code = code.replace(
  "accounts.push({ user, pass, isDefault: accounts.length === 0 });",
  "accounts.push({ user, pass, isDefault: accounts.length === 0 });\n      saveAccounts(accounts);"
);

code = code.replace(
  "if (accounts.findIndex((a, i) => a.user === user && i !== index) !== -1) {",
  "const accounts = getAccounts();\n      if (accounts.findIndex((a:any, i:any) => a.user === user && i !== index) !== -1) {"
);
code = code.replace(
  "accounts[index].user = user;\n      accounts[index].pass = pass;\n      saveAccounts();",
  "accounts[index].user = user;\n      accounts[index].pass = pass;\n      saveAccounts(accounts);"
);

code = code.replace(
  "accounts[index].isDisabled = !accounts[index].isDisabled;\n          saveAccounts();",
  "const accounts = getAccounts();\n          accounts[index].isDisabled = !accounts[index].isDisabled;\n          saveAccounts(accounts);"
);

code = code.replace(
  "log('账号管理', `已删除账号 ${accounts[index].user}`);\n        accounts.splice(index, 1);",
  "const accounts = getAccounts();\n        log('账号管理', `已删除账号 ${accounts[index].user}`);\n        accounts.splice(index, 1);"
);

code = code.replace(
  "if (accounts.length > 0 && !accounts.find(a => a.isDefault)) {\n          accounts[0].isDefault = true;\n        }\n        saveAccounts();",
  "if (accounts.length > 0 && !accounts.find((a:any) => a.isDefault)) {\n          accounts[0].isDefault = true;\n        }\n        saveAccounts(accounts);"
);

code = code.replace(
  "deleteText.textContent = `确定要删除账号 ${accounts[index].user} 吗？`;",
  "const accounts = getAccounts();\n        deleteText.textContent = `确定要删除账号 ${accounts[index].user} 吗？`;"
);

code = code.replace(
  "editAccIndex.value = index.toString();\n      editAccUsername.value = accounts[index].user;\n      editAccPassword.value = accounts[index].pass;",
  "const accounts = getAccounts();\n      editAccIndex.value = index.toString();\n      editAccUsername.value = accounts[index].user;\n      editAccPassword.value = accounts[index].pass;"
);

code = code.replace(
  "const account = accounts.splice(index, 1)[0];\n      accounts.unshift(account);\n      accounts.forEach((a, i) => a.isDefault = (i === 0));\n      saveAccounts();",
  "const accounts = getAccounts();\n      const account = accounts.splice(index, 1)[0];\n      accounts.unshift(account);\n      accounts.forEach((a:any, i:any) => a.isDefault = (i === 0));\n      saveAccounts(accounts);"
);

code = code.replace(
  "const item = accounts.splice(oldIndex, 1)[0];\n        accounts.splice(newIndex, 0, item);\n        accounts.forEach((a, i) => a.isDefault = (i === 0));\n        saveAccounts();",
  "const accounts = getAccounts();\n        const item = accounts.splice(oldIndex, 1)[0];\n        accounts.splice(newIndex, 0, item);\n        accounts.forEach((a:any, i:any) => a.isDefault = (i === 0));\n        saveAccounts(accounts);\n        renderAccounts();"
);

code = code.replace(
  "function renderAccounts() {\n  accountsList.innerHTML = '';",
  "function renderAccounts() {\n  const accounts = getAccounts();\n  accountsList.innerHTML = '';"
);

code = code.replace(
  "function updateOverrideOptions() {\n  const select = document.getElementById('override-account') as HTMLSelectElement;",
  "function updateOverrideOptions() {\n  const accounts = getAccounts();\n  const select = document.getElementById('override-account') as HTMLSelectElement;"
);

code = code.replace(
  "function saveAccounts() {\n  localStorage.setItem('bjut_accounts', JSON.stringify(accounts));\n}\n\n",
  ""
);

code = code.replace(
  "setTimeout(() => {\n    const container = document.querySelector('.logs-container');\n    if (container) {\n      container.scrollTop = container.scrollHeight;\n    }\n  }, 10);",
  "requestAnimationFrame(() => {\n    entry.scrollIntoView({ behavior: 'smooth', block: 'end' });\n  });"
);

code = code.replace(
  "if (accounts.length === 0) {\n    alert('请先在账号管理中添加账号');",
  "const accounts = getAccounts();\n  if (accounts.length === 0) {\n    customAlert('请先在账号管理中添加账号');"
);

code = code.replace(/alert\(/g, 'customAlert(');
code = code.replace(/confirm\(/g, 'customConfirm(');

// Manage whitelist
code = code.replace(
  "customAlert('当前信任的 WiFi (白名单):\\n' + (w.length ? w.join('\\n') : '暂无'));",
  "showListManageModal('信任的 WiFi (白名单)', w, (newList) => localStorage.setItem('bjut_whitelist', JSON.stringify(newList)));"
);

// Manage blacklist
code = code.replace(
  "customAlert('当前拒绝的 WiFi (黑名单):\\n' + (b.length ? b.join('\\n') : '暂无'));",
  "showListManageModal('拒绝的 WiFi (黑名单)', b, (newList) => localStorage.setItem('bjut_blacklist', JSON.stringify(newList)));"
);


// import/export logic
const importExportLogic = `
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
        
        customAlert('导入成功，请重启应用以应用更改！');
      } catch (e) {
        customAlert('导入失败：' + String(e));
      }
    });
  }
`;

code = code.replace("// Password visibility toggle", importExportLogic + "\n  // Password visibility toggle");

// remove the custom item.animate in onEnd
code = code.replace(/if \(clientX > 0 && clientY > 0\) \{[\s\S]*?duration: 350,[\s\S]*?\}\);[\s\S]*?\}/, '');
// remove onStart override
code = code.replace(/onStart: \(evt\) => \{[\s\S]*?\},/, '');
// remove manual DOM manipulation in onEnd
code = code.replace(/\/\/ Update DOM attributes manually[\s\S]*?\}\);/, '');

fs.writeFileSync('src/main.ts', code, 'utf8');
