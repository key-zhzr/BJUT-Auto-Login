import { createIcons, icons } from 'lucide';
import Sortable from 'sortablejs';
import { checkInternet, detectLoginType, loginToCampusNetwork, fetchUserInfo, LoginType, NetworkState } from './network';

// UI Elements
const navItems = document.querySelectorAll('.nav-item');
const pages = document.querySelectorAll('.page');
const networkStatus = document.getElementById('network-status')!;
const networkDetail = document.getElementById('network-detail')!;
const networkIcon = document.getElementById('network-icon')!;
const btnLogin = document.getElementById('btn-login') as HTMLButtonElement;
const infoAccount = document.getElementById('info-account')!;
const infoBalance = document.getElementById('info-balance')!;
const infoFlow = document.getElementById('info-flow')!;
const accountsList = document.getElementById('accounts-list')!;
const addAccountForm = document.getElementById('add-account-form') as HTMLFormElement;
const logsContent = document.getElementById('logs-content')!;
const btnClearLogs = document.getElementById('btn-clear-logs')!;
const settingAutoLogin = document.getElementById('setting-auto-login') as HTMLInputElement;
const settingCheckInterval = document.getElementById('setting-check-interval') as HTMLInputElement;

// Add Modal
const addModal = document.getElementById('add-modal')!;
const btnShowAdd = document.getElementById('btn-show-add')!;
const btnCancelAdd = document.getElementById('btn-cancel-add')!;

// Edit Modal
const editModal = document.getElementById('edit-modal')!;
const editAccountForm = document.getElementById('edit-account-form') as HTMLFormElement;
const btnCancelEdit = document.getElementById('btn-cancel-edit')!;
const editAccIndex = document.getElementById('edit-acc-index') as HTMLInputElement;
const editAccUsername = document.getElementById('edit-acc-username') as HTMLInputElement;
const editAccPassword = document.getElementById('edit-acc-password') as HTMLInputElement;

// State
let accounts: any[] = JSON.parse(localStorage.getItem('bjut_accounts') || '[]');
let currentNetworkState = NetworkState.Offline;
let currentLoginType = LoginType.Unknown;
let autoLoginEnabled = localStorage.getItem('bjut_auto_login') === 'true';
let checkInterval = parseInt(localStorage.getItem('bjut_check_interval') || '15', 10);
let checkTimer: number | null = null;
let isLoggingIn = false;

// Initialize
function init() {
  createIcons({ icons });
  settingAutoLogin.checked = autoLoginEnabled;
  settingCheckInterval.value = checkInterval.toString();
  
  setupNavigation();
  setupEventListeners();
  renderAccounts();
  log('系统', '应用启动');
  
  startNetworkCheckLoop();
}

// Navigation
function setupNavigation() {
  navItems.forEach(item => {
    item.addEventListener('click', () => {
      navItems.forEach(n => n.classList.remove('active'));
      pages.forEach(p => p.classList.remove('active'));
      
      item.classList.add('active');
      const target = item.getAttribute('data-target');
      document.getElementById(target!)?.classList.add('active');
    });
  });
}

// Event Listeners
function setupEventListeners() {
  btnLogin.addEventListener('click', manualLogin);
  
  // Add modal toggle
  btnShowAdd.addEventListener('click', () => {
    addModal.classList.remove('hidden');
  });
  btnCancelAdd.addEventListener('click', () => {
    addModal.classList.add('hidden');
    addAccountForm.reset();
  });

  addAccountForm.addEventListener('submit', (e) => {
    e.preventDefault();
    const user = (document.getElementById('acc-username') as HTMLInputElement).value.trim();
    const pass = (document.getElementById('acc-password') as HTMLInputElement).value;
    
    if (user && pass) {
      if (accounts.find(a => a.user === user)) {
        alert('该账号已存在');
        return;
      }
      accounts.push({ user, pass, isDefault: accounts.length === 0 });
      saveAccounts();
      renderAccounts();
      addAccountForm.reset();
      addModal.classList.add('hidden');
      log('账号管理', `已添加账号: ${user}`);
    }
  });

  btnClearLogs.addEventListener('click', () => {
    logsContent.innerHTML = '';
  });

  settingAutoLogin.addEventListener('change', async (e) => {
    autoLoginEnabled = (e.target as HTMLInputElement).checked;
    localStorage.setItem('bjut_auto_login', autoLoginEnabled.toString());
    log('设置', `自动登录已${autoLoginEnabled ? '开启' : '关闭'}`);
    
    // 如果是开启自动登录，且可能在移动端运行
    if (autoLoginEnabled && navigator.userAgent.toLowerCase().includes('android')) {
      alert('【安卓后台保活提示】\\n为确保后台自动登录正常运行，请前往系统设置：\\n1. 授予本应用“通知”权限\\n2. 允许本应用“自启动”和“后台运行”\\n\\n我们将尝试发送常驻通知以防止程序被系统清理。');
      // 尝试请求通知权限并发送常驻通知
      if ('Notification' in window) {
        if (Notification.permission !== 'granted') {
          await Notification.requestPermission();
        }
        if (Notification.permission === 'granted') {
          try {
            // 使用 Service Worker 维持常驻通知
            if ('serviceWorker' in navigator) {
              const registration = await navigator.serviceWorker.ready;
              registration.showNotification('校园网自动登录运行中', {
                body: '保持后台运行以随时检测并自动重连校园网',
                icon: '/icons/128x128.png',
                requireInteraction: true, // 类似常驻通知
                tag: 'bjut-al-keepalive',
                silent: true
              });
            } else {
              new Notification('校园网自动登录运行中', {
                body: '保持后台运行以随时检测并自动重连校园网',
                requireInteraction: true,
                tag: 'bjut-al-keepalive',
                silent: true
              });
            }
          } catch (err) {
            console.error('Failed to show notification:', err);
          }
        }
      }
    }
  });

  settingCheckInterval.addEventListener('change', (e) => {
    const val = parseInt((e.target as HTMLInputElement).value, 10);
    if (val >= 5) {
      checkInterval = val;
      localStorage.setItem('bjut_check_interval', checkInterval.toString());
      log('设置', `前台检测间隔设置为 ${val} 秒`);
      startNetworkCheckLoop();
    }
  });

  const settingCheckIntervalBg = document.getElementById('setting-check-interval-bg') as HTMLInputElement;
  if (settingCheckIntervalBg) {
    let checkIntervalBg = parseInt(localStorage.getItem('bjut_check_interval_bg') || '60', 10);
    settingCheckIntervalBg.value = checkIntervalBg.toString();
    settingCheckIntervalBg.addEventListener('change', (e) => {
      const val = parseInt((e.target as HTMLInputElement).value, 10);
      if (val >= 5) {
        checkIntervalBg = val;
        localStorage.setItem('bjut_check_interval_bg', checkIntervalBg.toString());
        log('设置', `后台检测间隔设置为 ${val} 秒`);
      }
    });
  }

  // Password visibility toggle
  document.querySelectorAll('.toggle-password').forEach(btn => {
    btn.addEventListener('click', (e) => {
      const targetId = (e.currentTarget as HTMLElement).getAttribute('data-target');
      const input = document.getElementById(targetId!) as HTMLInputElement;
      if (input.type === 'password') {
        input.type = 'text';
        btn.classList.remove('hide-password');
      } else {
        input.type = 'password';
        btn.classList.add('hide-password');
      }
    });
  });

  // Edit Modal events
  btnCancelEdit.addEventListener('click', () => {
    editModal.classList.add('hidden');
  });

  editAccountForm.addEventListener('submit', (e) => {
    e.preventDefault();
    const index = parseInt(editAccIndex.value, 10);
    const user = editAccUsername.value.trim();
    const pass = editAccPassword.value;

    if (user && pass && !isNaN(index)) {
      if (accounts.findIndex((a, i) => a.user === user && i !== index) !== -1) {
        alert('该账号名已存在');
        return;
      }
      accounts[index].user = user;
      accounts[index].pass = pass;
      saveAccounts();
      renderAccounts();
      editModal.classList.add('hidden');
      log('账号管理', `已修改账号: ${user}`);
    }
  });

  // Account List Event Delegation
  accountsList.addEventListener('click', (e) => {
    const target = e.target as HTMLElement;
    const btn = target.closest('button');
    if (!btn) return;
    
    const index = parseInt(btn.getAttribute('data-index') || '-1', 10);
    
    // Toggle password visibility
    if (btn.classList.contains('action-toggle-password')) {
      const parent = btn.closest('div');
      const textSpan = parent?.querySelector('.password-text') as HTMLElement;
      if (textSpan) {
        if (textSpan.textContent === '**********') {
          textSpan.textContent = textSpan.getAttribute('data-password') || '';
          btn.classList.remove('hide-password');
        } else {
          textSpan.textContent = '**********';
          btn.classList.add('hide-password');
        }
      }
      return;
    }

    if (index === -1) return;

    if (btn.classList.contains('action-delete')) {
      const deleteModal = document.getElementById('delete-modal');
      const deleteText = document.getElementById('delete-modal-text');
      if (deleteModal && deleteText) {
        deleteModal.setAttribute('data-delete-index', index.toString());
        deleteText.textContent = `确定要删除账号 ${accounts[index].user} 吗？`;
        deleteModal.classList.remove('hidden');
      }
    } else if (btn.classList.contains('action-default')) {
      // First: Get current rects
      const firstRects = new Map<HTMLElement, DOMRect>();
      Array.from(accountsList.children).forEach(child => {
        firstRects.set(child as HTMLElement, child.getBoundingClientRect());
      });
      
      const account = accounts.splice(index, 1)[0];
      accounts.unshift(account);
      accounts.forEach((a, i) => a.isDefault = (i === 0));
      saveAccounts();
      
      // Move DOM element to top
      const itemToMove = accountsList.children[index] as HTMLElement;
      accountsList.prepend(itemToMove);
      
      // Update attributes manually
      Array.from(accountsList.children).forEach((el, i) => {
        el.setAttribute('data-index', i.toString());
        el.querySelectorAll('button[data-index]').forEach(b => b.setAttribute('data-index', i.toString()));
        
        const badge = el.querySelector('.account-badge');
        if (badge) {
          badge.textContent = i === 0 ? '默认' : '备用';
          badge.className = `account-badge ${i === 0 ? 'text-primary font-bold' : 'text-muted'}`;
        }
        
        el.querySelectorAll('.action-default').forEach(defaultBtn => {
          const b = defaultBtn as HTMLButtonElement;
          if (i === 0) {
            b.disabled = true;
            b.style.color = 'var(--text-muted)';
            b.style.cursor = 'not-allowed';
            b.style.opacity = '0.5';
            b.setAttribute('title', '已置顶');
          } else {
            b.disabled = false;
            b.style.color = '';
            b.style.cursor = 'pointer';
            b.style.opacity = '1';
            b.setAttribute('title', '设为默认 (置顶)');
          }
        });
      });
      
      // Last, Invert, Play
      Array.from(accountsList.children).forEach(child => {
        const el = child as HTMLElement;
        const first = firstRects.get(el);
        if (!first) return;
        const last = el.getBoundingClientRect();
        
        const dx = first.left - last.left;
        const dy = first.top - last.top;
        
        if (dx !== 0 || dy !== 0) {
          el.animate([
            { transform: `translate(${dx}px, ${dy}px)` },
            { transform: 'translate(0, 0)' }
          ], {
            duration: 400,
            easing: 'cubic-bezier(0.34, 1.56, 0.64, 1)'
          });
        }
      });
      
      log('账号管理', `已将账号 ${account.user} 置顶`);
    } else if (btn.classList.contains('action-edit')) {
      editAccIndex.value = index.toString();
      editAccUsername.value = accounts[index].user;
      editAccPassword.value = accounts[index].pass;
      editModal.classList.remove('hidden');
    }
  });

  // Delete Modal Events
  const btnCancelDelete = document.getElementById('btn-cancel-delete');
  const btnConfirmDelete = document.getElementById('btn-confirm-delete');
  const deleteModal = document.getElementById('delete-modal');
  
  if (btnCancelDelete && btnConfirmDelete && deleteModal) {
    btnCancelDelete.addEventListener('click', () => {
      deleteModal.classList.add('hidden');
    });
    
    btnConfirmDelete.addEventListener('click', () => {
      const idxStr = deleteModal.getAttribute('data-delete-index');
      if (idxStr) {
        const index = parseInt(idxStr, 10);
        log('账号管理', `已删除账号 ${accounts[index].user}`);
        accounts.splice(index, 1);
        if (accounts.length > 0 && !accounts.find(a => a.isDefault)) {
          accounts[0].isDefault = true;
        }
        saveAccounts();
        renderAccounts();
        deleteModal.classList.add('hidden');
      }
    });
  }

  // Drag and Drop using SortableJS
  let dragOffsetX = 0;
  let dragOffsetY = 0;

  Sortable.create(accountsList, {
    handle: '.drag-handle',
    animation: 300,
    easing: "cubic-bezier(0.25, 1, 0.5, 1)",
    ghostClass: 'dragging',
    forceFallback: true,
    fallbackClass: 'dragging-fallback',
    fallbackOnBody: true,
    onStart: (evt) => {
      const e = (evt as any).originalEvent as MouseEvent | TouchEvent;
      const rect = evt.item.getBoundingClientRect();
      let clientX = 0, clientY = 0;
      if (e) {
        if ('touches' in e && e.touches.length > 0) {
          clientX = e.touches[0].clientX;
          clientY = e.touches[0].clientY;
        } else if ('clientX' in e) {
          clientX = (e as MouseEvent).clientX;
          clientY = (e as MouseEvent).clientY;
        }
      }
      dragOffsetX = clientX - rect.left;
      dragOffsetY = clientY - rect.top;
    },
    onEnd: (evt) => {
      const { oldIndex, newIndex, item } = evt;
      
      // Calculate drop position for custom animation
      const e = (evt as any).originalEvent as MouseEvent | TouchEvent;
      let clientX = 0, clientY = 0;
      if (e) {
        if ('changedTouches' in e && e.changedTouches && e.changedTouches.length > 0) {
          clientX = e.changedTouches[0].clientX;
          clientY = e.changedTouches[0].clientY;
        } else if ('clientX' in e) {
          clientX = (e as MouseEvent).clientX;
          clientY = (e as MouseEvent).clientY;
        }
      }
      
      if (clientX > 0 && clientY > 0) {
        const rect = item.getBoundingClientRect();
        const startX = clientX - dragOffsetX;
        const startY = clientY - dragOffsetY;
        const dx = startX - rect.left;
        const dy = startY - rect.top;
        
        item.animate([
          { transform: `translate(${dx}px, ${dy}px)`, zIndex: 9999, boxShadow: '0 10px 25px rgba(0,0,0,0.5)', opacity: 0.95 },
          { transform: 'translate(0, 0)', zIndex: 9999, boxShadow: '0 4px 6px -1px rgba(0, 0, 0, 0.1), 0 2px 4px -1px rgba(0, 0, 0, 0.06)', opacity: 1 }
        ], {
          duration: 350,
          easing: 'cubic-bezier(0.34, 1.56, 0.64, 1)'
        });
      }
      if (oldIndex !== undefined && newIndex !== undefined && oldIndex !== newIndex) {
        const item = accounts.splice(oldIndex, 1)[0];
        accounts.splice(newIndex, 0, item);
        accounts.forEach((a, i) => a.isDefault = (i === 0));
        saveAccounts();
        
        // Update DOM attributes manually to allow Sortable drop animation to complete smoothly
        Array.from(accountsList.children).forEach((el, i) => {
          el.setAttribute('data-index', i.toString());
          el.querySelectorAll('button[data-index]').forEach(btn => btn.setAttribute('data-index', i.toString()));
          
          const badge = el.querySelector('.account-badge');
          if (badge) {
            badge.textContent = i === 0 ? '默认' : '备用';
            badge.className = `account-badge ${i === 0 ? 'text-primary font-bold' : 'text-muted'}`;
          }
          
          el.querySelectorAll('.action-default').forEach(btn => {
            const b = btn as HTMLButtonElement;
            if (i === 0) {
              b.disabled = true;
              b.style.color = 'var(--text-muted)';
              b.style.cursor = 'not-allowed';
              b.style.opacity = '0.5';
              b.setAttribute('title', '已置顶');
            } else {
              b.disabled = false;
              b.style.color = '';
              b.style.cursor = 'pointer';
              b.style.opacity = '1';
              b.setAttribute('title', '设为默认 (置顶)');
            }
          });
        });
        log('账号管理', '账号顺序已更新，最高优先级将作为默认账号');
      }
    }
  });
}

// Logging
function log(module: string, message: string, type: 'info' | 'error' | 'success' = 'info') {
  const time = new Date().toLocaleTimeString();
  const entry = document.createElement('div');
  entry.className = 'log-entry';
  entry.innerHTML = `<span class="log-time">[${time}]</span><span class="log-${type}">[${module}] ${message}</span>`;
  logsContent.appendChild(entry);
  
  const container = document.querySelector('.logs-container');
  if (container) {
    container.scrollTop = container.scrollHeight;
  }
}

// Accounts
function saveAccounts() {
  localStorage.setItem('bjut_accounts', JSON.stringify(accounts));
}

function renderAccounts() {
  accountsList.innerHTML = '';
  if (accounts.length === 0) {
    accountsList.innerHTML = '<div style="color: var(--text-muted); padding: 1rem;">暂无账号，请添加。</div>';
    return;
  }
  
  accounts.forEach((acc, index) => {
    const item = document.createElement('div');
    item.className = 'account-item glass-card';
    item.setAttribute('data-index', index.toString());
    const avatarText = acc.user.length >= 2 ? acc.user.slice(-2) : acc.user;
    
    item.innerHTML = `
      <div class="account-left">
        <div class="drag-handle"><i data-lucide="grip-vertical"></i></div>
        <div class="account-avatar">${avatarText}</div>
        <div class="account-user">
          <h4>${acc.user}</h4>
          <span class="account-badge ${acc.isDefault ? 'text-primary font-bold' : 'text-muted'}">${acc.isDefault ? '默认' : '备用'}</span>
        </div>
        <div class="account-mobile-actions">
          <button class="btn-icon action-edit" data-index="${index}" title="编辑"><i data-lucide="edit-2"></i></button>
          <button class="btn-icon action-default" style="${acc.isDefault ? 'color: var(--text-muted); cursor: not-allowed; opacity: 0.5;' : ''}" data-index="${index}" title="${acc.isDefault ? '已置顶' : '设为默认 (置顶)'}" ${acc.isDefault ? 'disabled' : ''}><i data-lucide="arrow-up-circle"></i></button>
        </div>
      </div>
      <div class="account-right">
        <div class="account-password">
          <span class="password-text" data-password="${acc.pass}" style="font-family: monospace; font-size: 0.9rem; color: var(--text-muted);">**********</span>
          <button class="btn-icon action-toggle-password hide-password" style="padding: 0.2rem;" title="显示/隐藏密码"><i data-lucide="eye"></i></button>
        </div>
        <div class="account-desktop-actions">
          <button class="btn-icon action-edit" data-index="${index}" title="编辑"><i data-lucide="edit-2"></i></button>
          <button class="btn-icon action-default" style="${acc.isDefault ? 'color: var(--text-muted); cursor: not-allowed; opacity: 0.5;' : ''}" data-index="${index}" title="${acc.isDefault ? '已置顶' : '设为默认 (置顶)'}" ${acc.isDefault ? 'disabled' : ''}><i data-lucide="arrow-up-circle"></i></button>
        </div>
        <button class="btn-icon danger action-delete" data-index="${index}" title="删除"><i data-lucide="trash-2"></i></button>
      </div>
    `;
    accountsList.appendChild(item);
  });
  createIcons({ icons });
}

// Network Check Loop
function startNetworkCheckLoop() {
  if (checkTimer) clearTimeout(checkTimer);
  
  const tick = async () => {
    await checkNetwork();
    const intervalFg = parseInt(localStorage.getItem('bjut_check_interval') || '15', 10);
    const intervalBg = parseInt(localStorage.getItem('bjut_check_interval_bg') || '60', 10);
    const currentInterval = document.hidden ? intervalBg : intervalFg;
    checkTimer = setTimeout(tick, currentInterval * 1000) as any;
  };
  
  tick();
}

async function checkNetwork() {
  if (isLoggingIn) return;
  
  const isInternetOk = await checkInternet();
  if (isInternetOk) {
    updateNetworkStatus(NetworkState.Online);
    await updateUserInfo();
  } else {
    const loginType = await detectLoginType();
    if (loginType !== LoginType.Unknown) {
      updateNetworkStatus(NetworkState.BjutCampus, loginType);
      currentLoginType = loginType;
      
      if (autoLoginEnabled && !isLoggingIn) {
        log('网络', '检测到校园网，准备自动登录');
        manualLogin();
      }
    } else {
      updateNetworkStatus(NetworkState.Offline);
    }
  }
}

function updateNetworkStatus(state: NetworkState, type?: LoginType) {
  currentNetworkState = state;
  networkIcon.className = 'status-icon';
  
  if (state === NetworkState.Online) {
    networkStatus.textContent = '互联网已连接';
    networkDetail.textContent = '网络畅通无阻';
    networkIcon.classList.add('success');
    networkIcon.innerHTML = '<i data-lucide="check-circle"></i>';
    btnLogin.disabled = true;
  } else if (state === NetworkState.BjutCampus) {
    networkStatus.textContent = '检测到校园网';
    networkDetail.textContent = `需要认证 (登录类型: ${type})`;
    networkIcon.classList.add('warning');
    networkIcon.innerHTML = '<i data-lucide="alert-circle"></i>';
    btnLogin.disabled = false;
  } else {
    networkStatus.textContent = '网络断开或非校园网';
    networkDetail.textContent = '无法访问互联网和校园网登录页';
    networkIcon.classList.add('error');
    networkIcon.innerHTML = '<i data-lucide="wifi-off"></i>';
    btnLogin.disabled = true;
    
    // Clear user info
    infoAccount.textContent = '未登录';
    infoBalance.textContent = '--';
    infoFlow.textContent = '--';
  }
  createIcons({ icons });
}

async function updateUserInfo() {
  const info = await fetchUserInfo();
  if (info) {
    infoAccount.textContent = info.account;
    infoBalance.textContent = info.balance;
    infoFlow.textContent = info.flow;
  }
}

async function manualLogin() {
  if (currentNetworkState !== NetworkState.BjutCampus) {
    alert('当前无需登录或未连接校园网');
    return;
  }
  
  // Actually, we use the first account by order now since it's drag and drop sorted
  // But we still maintain the 'isDefault' flag. The prompt said "用户上下拖动决定账号使用顺序".
  // So we should try to login using the first account, if it fails, try the second, etc.
  // Wait, the prompt says "上下拖动决定账号使用顺序", meaning we might want to loop.
  // Let's implement multi-account sequential login fallback!
  
  if (accounts.length === 0) {
    alert('请先在账号管理中添加账号');
    return;
  }

  isLoggingIn = true;
  btnLogin.disabled = true;
  btnLogin.innerHTML = '<i data-lucide="loader"></i> 登录中...';
  createIcons({ icons });
  
  let success = false;
  for (let acc of accounts) {
    log('登录', `尝试使用账号 ${acc.user} 登录...`);
    const result = await loginToCampusNetwork(currentLoginType, acc.user, acc.pass);
    if (result.success) {
      log('登录', '登录成功！', 'success');
      success = true;
      break;
    } else {
      log('登录', `登录失败: ${result.msg}`, 'error');
    }
  }

  if (success) {
    btnLogin.innerHTML = '<i data-lucide="check"></i> 已连接';
    updateNetworkStatus(NetworkState.Online);
    setTimeout(updateUserInfo, 2000);
  } else {
    btnLogin.disabled = false;
    btnLogin.innerHTML = '<i data-lucide="log-in"></i> 立即登录';
  }
  createIcons({ icons });
  isLoggingIn = false;
}

// Start
init();
