/** Shared keyboard behavior, independent of the current visual theme. */
export function setupKeyboardNavigation() {
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
  const refreshModal = () => {
    const visible = Array.from(document.querySelectorAll<HTMLElement>('.modal-overlay:not(.hidden):not([hidden])')).filter(node => node.getClientRects().length > 0);
    const next = visible[visible.length - 1] || null;
    if (next === modal) return;
    if (next) {
      if (!modal) returnFocus = document.activeElement as HTMLElement;
      modal = next; modal.setAttribute('role', 'dialog'); modal.setAttribute('aria-modal', 'true');
      if (!modal.contains(document.activeElement)) (focusable(modal)[0] || modal).focus();
    } else { modal = null; if (returnFocus?.isConnected) returnFocus.focus(); returnFocus = null; }
  };
  new MutationObserver(refreshModal).observe(document.body, { attributes: true, attributeFilter: ['class', 'hidden'], childList: true, subtree: true });
  document.addEventListener('keydown', event => {
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
