let navigationInstalled = false;

/** Shared keyboard behavior, independent of the current visual theme. */
export function setupKeyboardNavigation() {
  if (navigationInstalled) return;
  navigationInstalled = true;
  const labels: Record<string, string> = { 'titlebar-minimize': '最小化窗口', 'titlebar-maximize': '最大化或还原窗口', 'titlebar-close': '关闭窗口' };
  document.querySelectorAll<HTMLElement>('.nav-item[data-target], .titlebar-button').forEach(node => {
    node.tabIndex = 0; node.setAttribute('role', 'button'); node.dataset.keyboardActivate = 'true';
    if (labels[node.id]) node.setAttribute('aria-label', labels[node.id]);
  });
  let pressed: HTMLElement | null = null;
  const release = () => { pressed?.classList.remove('keyboard-pressed'); pressed = null; };
  document.addEventListener('keydown', event => {
    const target = event.target as HTMLElement;
    if (!['Enter', ' '].includes(event.key) || event.repeat || target.matches(':disabled,[aria-disabled="true"]')) return;
    if (target.matches('button,[data-keyboard-activate],.custom-select-trigger,summary')) {
      release(); pressed = target; target.classList.add('keyboard-pressed');
    }
    if (target.dataset.keyboardActivate) { event.preventDefault(); target.click(); }
  });
  document.addEventListener('keyup', release);
  window.addEventListener('blur', release);
  document.addEventListener('focusout', release);
  // Keep tab navigation within visible dialogs and restore focus on dismissal.
  let modal: HTMLElement | null = null;
  let returnFocus: HTMLElement | null = null;
  const focusable = (root: HTMLElement) => Array.from(root.querySelectorAll<HTMLElement>('button,a[href],input,select,textarea,[tabindex="0"]')).filter(node => !node.matches(':disabled,[aria-disabled="true"]') && node.getClientRects().length > 0);
  const dialogs = new Set<HTMLElement>();
  let refreshFrame: number | null = null;
  const refreshModal = () => {
    if (refreshFrame !== null) { cancelAnimationFrame(refreshFrame); refreshFrame = null; }
    const visible = Array.from(dialogs).filter(node => node.isConnected && !node.hidden && !node.classList.contains('hidden') && node.getClientRects().length > 0);
    const next = visible[visible.length - 1] || null;
    if (next === modal) return;
    if (next) {
      if (!modal) returnFocus = document.activeElement as HTMLElement;
      modal = next; modal.setAttribute('role', 'dialog'); modal.setAttribute('aria-modal', 'true');
      if (!modal.contains(document.activeElement)) (focusable(modal)[0] || modal).focus({ preventScroll: true });
    } else { modal = null; if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true }); returnFocus = null; }
  };
  // Only dialog roots change modal visibility. Watching every class/child in
  // the WebView made each dropdown/log update trigger synchronous layout reads.
  const scheduleRefresh = () => {
    if (refreshFrame === null) refreshFrame = requestAnimationFrame(refreshModal);
  };
  const dialogObserver = new MutationObserver(scheduleRefresh);
  const registerDialogs = (root: ParentNode) => {
    if (root instanceof HTMLElement && root.matches('.modal-overlay')) dialogs.add(root);
    root.querySelectorAll<HTMLElement>('.modal-overlay').forEach(dialog => dialogs.add(dialog));
  };
  const observeDialogs = () => {
    dialogObserver.disconnect();
    for (const dialog of dialogs) {
      if (!dialog.isConnected) dialogs.delete(dialog);
      else dialogObserver.observe(dialog, { attributes: true, attributeFilter: ['class', 'hidden'] });
    }
  };
  registerDialogs(document); observeDialogs();
  // Runtime dialogs are appended to body. Their contents need no observation.
  new MutationObserver(records => {
    let changed = false;
    for (const record of records) {
      for (const node of Array.from(record.addedNodes)) {
        if (!(node instanceof HTMLElement)) continue;
        const before = dialogs.size; registerDialogs(node); changed ||= dialogs.size !== before;
      }
    }
    if (changed || Array.from(dialogs).some(dialog => !dialog.isConnected)) {
      observeDialogs(); scheduleRefresh();
    }
  }).observe(document.body, { childList: true });
  document.addEventListener('keydown', event => {
    if (refreshFrame !== null) refreshModal();
    if (!modal) return;
    if (event.key === 'Escape' && !event.defaultPrevented) {
      const cancel = modal.querySelector<HTMLButtonElement>('#btn-confirm-cancel,#btn-cancel-add,#btn-cancel-edit,#btn-cancel-delete,#btn-sec-cancel,#btn-switch-account-cancel,#btn-cancel-network-profile,#btn-password-prompt-cancel,#btn-update-cancel,#btn-list-manage-close');
      if (cancel && !cancel.disabled && cancel.getClientRects().length) { event.preventDefault(); cancel.click(); }
      return;
    }
    if (event.key !== 'Tab') return;
    const items = focusable(modal);
    if (!items.length) { event.preventDefault(); return; }
    const first = items[0], last = items[items.length - 1];
    if (!modal.contains(document.activeElement) || (event.shiftKey ? document.activeElement === first : document.activeElement === last)) {
      event.preventDefault(); (event.shiftKey ? last : first).focus();
    }
  });
  refreshModal();
}
