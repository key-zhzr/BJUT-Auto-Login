import { isEditableElement, isTextSelectionAllowed } from './native-shell-guard';
import { IS_ANDROID, readTextFromClipboard, writeTextToClipboard } from './platform';

export type AppPageId = 'dashboard' | 'billing-center' | 'accounts' | 'diagnostics' | 'logs' | 'settings';

export interface ContextMenuLogEntry {
  time: string;
  module: string;
  message: string;
  type: 'info' | 'error' | 'success' | 'debug';
}

export interface DesktopContextMenuOptions {
  getActivePage: () => AppPageId | string;
  runPrimaryPageAction?: (pageId: string) => void | Promise<void>;
  triggerNetworkCheck?: () => void | Promise<void>;
  openPage?: (pageId: AppPageId | string) => void;
  openLogs?: () => void;
  exportLogs?: () => void | Promise<void>;
  openSearchLogs?: () => void;
  runDiagnostics?: () => void | Promise<void>;
  copyDiagnosticReport?: () => void | Promise<void>;
  hasDiagnosticReport?: () => boolean;
  openBillingCenter?: () => void;
  openAddAccount?: () => void;
  getLogEntryByIndex?: (index: number) => ContextMenuLogEntry | null | undefined;
  filterLogsByModule?: (module: string) => void;
  copyAllFilteredLogs?: () => void | Promise<void>;
  writeClipboard?: (text: string) => Promise<void>;
  readClipboard?: () => Promise<string>;
  notify?: (message: string) => void;
}

export interface DesktopContextMenuHandle {
  close: () => void;
  destroy: () => void;
}

type MenuItemIcon =
  | 'scissors'
  | 'copy'
  | 'paste'
  | 'select-all'
  | 'trash'
  | 'filter'
  | 'search'
  | 'download'
  | 'refresh'
  | 'activity'
  | 'logs'
  | 'plus'
  | 'receipt'
  | 'home'
  | 'info';

type MenuEntry =
  | { type: 'divider' }
  | { type: 'label'; text: string }
  | {
      type: 'action';
      id: string;
      label: string;
      icon: MenuItemIcon;
      shortcut?: string;
      disabled?: boolean;
      danger?: boolean;
      onSelect: () => void | Promise<void>;
    };

type EditableControl = HTMLInputElement | HTMLTextAreaElement;

interface SavedEditableSelection {
  kind: 'control';
  control: EditableControl;
  start: number;
  end: number;
  supportsRange: boolean;
}

interface SavedContentEditableSelection {
  kind: 'contenteditable';
  host: HTMLElement;
  range: Range | null;
}

type SavedSelectionSnapshot = SavedEditableSelection | SavedContentEditableSelection;

const ICON_SVG_PATHS: Record<MenuItemIcon, string> = {
  scissors:
    '<circle cx="6" cy="6" r="3"/><path d="M8.12 8.12 12 12"/><path d="M20 4 8.12 15.88"/><circle cx="6" cy="18" r="3"/><path d="M14.8 14.8 20 20"/>',
  copy:
    '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  paste:
    '<rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/><path d="M12 11h4"/><path d="M12 16h4"/><path d="M8 11h.01"/><path d="M8 16h.01"/>',
  'select-all':
    '<path d="M4 7V5a1 1 0 0 1 1-1h2"/><path d="M17 4h2a1 1 0 0 1 1 1v2"/><path d="M20 17v2a1 1 0 0 1-1 1h-2"/><path d="M7 20H5a1 1 0 0 1-1-1v-2"/><rect x="8" y="8" width="8" height="8" rx="1"/>',
  trash:
    '<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/>',
  filter:
    '<polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>',
  search:
    '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  download:
    '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>',
  refresh:
    '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
  activity:
    '<path d="M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2"/>',
  logs:
    '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>',
  plus:
    '<path d="M5 12h14"/><path d="M12 5v14"/>',
  receipt:
    '<path d="M4 2v20l2-1 2 1 2-1 2 1 2-1 2 1 2-1 2 1V2l-2 1-2-1-2 1-2-1-2 1-2-1-2 1Z"/><path d="M16 8h-6a2 2 0 1 0 0 4h4a2 2 0 1 1 0 4H8"/><path d="M12 17.5v-11"/>',
  home:
    '<path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/>',
  info:
    '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
};

const COPYABLE_FIELD_SELECTOR = [
  '.status-value',
  '.account-username',
  '.diagnostic-step',
  '.billing-summary-value',
  '.billing-detail-value',
  '.network-info-value',
  '.security-info-value',
  '.info-card',
  '.billing-summary-card',
  '.billing-detail-item',
  '.billing-recharge-preview > div',
  '.alipay-payment-order-card > div',
  '.dashboard-network-info > div',
  '.dashboard-update-value',
  '.account-item .account-user h4',
  '.account-health-reason',
  '.diagnostic-meta-chip',
  '.diagnostic-connectivity td',
  '#network-events-list li',
  '.security-info-box',
  '.router-port-account',
  '.router-port-traffic',
  '.trace-hop',
  'code',
  'pre',
  '[data-copy-value]',
].join(', ');

const INTERACTIVE_SKIP_SELECTOR = 'button, [role="button"], .btn, .btn-icon, .nav-item, .nav-tree-link, .switch, summary, .custom-select-trigger, .diagnostic-details-button, .drag-handle';

function isDevEnvironment(): boolean {
  return Boolean((import.meta as ImportMeta & { env?: { DEV?: boolean } }).env?.DEV);
}

function isMacOS(): boolean {
  return navigator.userAgent.includes('Mac');
}

function resolveElement(target: EventTarget | null): Element | null {
  if (target instanceof Element) return target;
  if (target instanceof Node) return target.parentElement;
  return null;
}

function createIconSvg(icon: MenuItemIcon): SVGSVGElement {
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '15');
  svg.setAttribute('height', '15');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '1.8');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add('desktop-context-menu-icon');
  svg.innerHTML = ICON_SVG_PATHS[icon];
  return svg;
}

function getInputSelectionRange(control: EditableControl): {
  start: number;
  end: number;
  supportsRange: boolean;
} {
  try {
    const start = control.selectionStart;
    const end = control.selectionEnd;
    if (typeof start === 'number' && typeof end === 'number') {
      return {
        start: Math.min(start, end),
        end: Math.max(start, end),
        supportsRange: true,
      };
    }
  } catch {
    // Inputs like type="number" or type="date" throw InvalidStateError when reading selectionStart.
  }
  const len = control.value.length;
  return { start: len, end: len, supportsRange: false };
}

function setInputSelectionRangeSafe(control: EditableControl, start: number, end: number): void {
  try {
    control.setSelectionRange(start, end);
  } catch {
    // Ignore for input types that do not support selection ranges.
  }
}

function dispatchValueChanged(el: HTMLElement, inputType: string): void {
  try {
    el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType }));
  } catch {
    el.dispatchEvent(new Event('input', { bubbles: true }));
  }
  el.dispatchEvent(new Event('change', { bubbles: true }));
}

function resolveEditableTarget(el: Element | null): EditableControl | HTMLElement | null {
  if (!el || !isEditableElement(el)) return null;
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    return el;
  }
  const ce = el.closest<HTMLElement>('[contenteditable=""], [contenteditable="true"], [contenteditable="plaintext-only"]');
  if (ce) return ce;
  return el instanceof HTMLElement && el.isContentEditable ? el : null;
}

function captureEditableSnapshot(target: EditableControl | HTMLElement): SavedSelectionSnapshot {
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    const range = getInputSelectionRange(target);
    return {
      kind: 'control',
      control: target,
      start: range.start,
      end: range.end,
      supportsRange: range.supportsRange,
    };
  }
  const sel = window.getSelection();
  let range: Range | null = null;
  if (sel && sel.rangeCount > 0) {
    const candidate = sel.getRangeAt(0);
    if (target.contains(candidate.commonAncestorContainer)) {
      range = candidate.cloneRange();
    }
  }
  return {
    kind: 'contenteditable',
    host: target,
    range,
  };
}

function resolveSelectableContainer(el: Element | null): HTMLElement | null {
  if (!el) return null;
  const candidate = el.closest<HTMLElement>(
    '#logs-content, .router-log-content, .diagnostic-details-body, .diagnostic-advice, .diagnostic-step-info, .tos-modal-body, #update-release-notes, pre, code, [data-selectable-text="true"]',
  );
  return candidate;
}

function extractBracketModule(rawText: string | null | undefined): string {
  if (!rawText) return '';
  const match = rawText.match(/\]\s*\[([^\]]+)\]/);
  return match?.[1]?.trim() || '';
}

function isPlaceholderValue(text: string): boolean {
  const trimmed = text.trim();
  return !trimmed || trimmed === '--' || trimmed === '—' || trimmed === '暂无' || trimmed === '未知';
}

function extractCopyableField(el: Element | null): { text: string; preview: string } | null {
  if (!el) return null;
  if (el.closest(INTERACTIVE_SKIP_SELECTOR)) return null;

  const field = el.closest<HTMLElement>(COPYABLE_FIELD_SELECTOR);
  if (!field) return null;
  if (field.closest('.password-text') || field.matches('.password-text')) return null;

  const explicit = field.dataset.copyValue?.trim();
  if (explicit && !isPlaceholderValue(explicit)) {
    return { text: explicit, preview: explicit };
  }

  if (field.matches('.diagnostic-step')) {
    const title = field.querySelector('.diagnostic-step-header strong')?.textContent?.trim() || '';
    const lead = field.querySelector('.diagnostic-step-lead')?.textContent?.trim() || '';
    const table = field.querySelector('.diagnostic-connectivity');
    const tableText = table
      ? Array.from(table.querySelectorAll('tbody tr'))
          .map(tr => Array.from(tr.children).map(cell => cell.textContent?.trim() || '').join(' | '))
          .filter(Boolean)
          .join('\n')
      : '';
    const body = lead || tableText;
    const combined = title && body ? `${title}：${body}` : title || body;
    if (isPlaceholderValue(combined)) return null;
    return { text: combined, preview: title || combined };
  }

  if (field.matches('.info-card')) {
    const val = field.querySelector('.info-content p')?.textContent?.trim() || '';
    if (isPlaceholderValue(val)) return null;
    return { text: val, preview: val };
  }

  if (field.matches('.billing-summary-card, .billing-detail-item, .billing-recharge-preview > div, .alipay-payment-order-card > div, .diagnostic-meta-chip')) {
    const val = field.querySelector('strong')?.textContent?.trim() || '';
    if (isPlaceholderValue(val)) return null;
    return { text: val, preview: val };
  }

  if (field.matches('.dashboard-network-info > div')) {
    const val = field.querySelector('span')?.textContent?.trim() || '';
    if (isPlaceholderValue(val)) return null;
    return { text: val, preview: val };
  }

  const rawText = field.textContent?.trim() || '';
  if (isPlaceholderValue(rawText)) return null;
  return { text: rawText, preview: rawText };
}

function formatCopyMenuLabel(preview: string): string {
  const singleLine = preview.replace(/\s+/g, ' ').trim();
  if (!singleLine || singleLine.length > 26 || preview.includes('\n')) {
    return '复制此项内容';
  }
  const clipped = singleLine.length > 16 ? `${singleLine.slice(0, 15)}…` : singleLine;
  return `复制「${clipped}」`;
}

function formatShortLabel(text: string, maxLen = 12): string {
  const trimmed = text.trim();
  return trimmed.length > maxLen ? `${trimmed.slice(0, maxLen - 1)}…` : trimmed;
}

let activeContextMenuHandle: DesktopContextMenuHandle | null = null;

/**
 * Installs the desktop right-click context menu.
 * No-op on Android so Android WebView uses its native input selection ActionMode when allowed.
 */
export function setupDesktopContextMenu(options: DesktopContextMenuOptions): DesktopContextMenuHandle {
  if (activeContextMenuHandle) {
    activeContextMenuHandle.destroy();
  }

  if (IS_ANDROID) {
    const noop: DesktopContextMenuHandle = { close: () => {}, destroy: () => {} };
    activeContextMenuHandle = noop;
    return noop;
  }

  const writeClipboard = options.writeClipboard ?? writeTextToClipboard;
  const readClipboard = options.readClipboard ?? readTextFromClipboard;
  const notify = (msg: string) => options.notify?.(msg);

  const menu = document.createElement('div');
  menu.id = 'desktop-context-menu';
  menu.className = 'desktop-context-menu';
  menu.setAttribute('role', 'menu');
  menu.setAttribute('aria-label', '快捷操作菜单');
  menu.tabIndex = -1;
  menu.hidden = true;
  menu.inert = true;
  if ('popover' in HTMLElement.prototype) {
    menu.setAttribute('popover', 'manual');
  }
  document.body.appendChild(menu);

  let isOpen = false;
  let openedAt = 0;
  let closeTimer: number | null = null;
  let returnFocusEl: HTMLElement | null = null;
  let actionButtons: HTMLButtonElement[] = [];
  let activeButtonIndex = -1;

  const supportsPopover = () =>
    typeof (menu as HTMLElement & { showPopover?: () => void }).showPopover === 'function';

  const clearCloseTimer = () => {
    if (closeTimer !== null) {
      window.clearTimeout(closeTimer);
      closeTimer = null;
    }
  };

  const finishHide = () => {
    clearCloseTimer();
    const popoverEl = menu as HTMLElement & { hidePopover?: () => void };
    if (typeof popoverEl.hidePopover === 'function' && menu.matches(':popover-open')) {
      try {
        popoverEl.hidePopover();
      } catch {
        // Ignore popover state mismatch during teardown.
      }
    }
    menu.hidden = true;
    menu.inert = true;
    menu.classList.remove('is-open', 'is-closing', 'is-fallback-layer');
    menu.style.left = '';
    menu.style.top = '';
    menu.style.transformOrigin = '';
    menu.replaceChildren();
    actionButtons = [];
    activeButtonIndex = -1;
  };

  const close = (restoreFocus = false) => {
    if (!isOpen && menu.hidden) return;
    isOpen = false;
    menu.classList.remove('is-open');
    menu.classList.add('is-closing');
    menu.inert = true;

    const focusTarget = restoreFocus ? returnFocusEl : null;
    returnFocusEl = null;

    const reducedMotion =
      typeof window.matchMedia === 'function' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    clearCloseTimer();
    if (reducedMotion) {
      finishHide();
    } else {
      closeTimer = window.setTimeout(finishHide, 130);
    }

    if (focusTarget?.isConnected) {
      focusTarget.focus({ preventScroll: true });
    }
  };

  const highlightIndex = (nextIndex: number) => {
    if (!actionButtons.length) {
      activeButtonIndex = -1;
      return;
    }
    const total = actionButtons.length;
    const normalized = ((nextIndex % total) + total) % total;
    activeButtonIndex = normalized;
    const btn = actionButtons[normalized];
    btn.focus({ preventScroll: true });
  };

  const moveHighlight = (delta: 1 | -1) => {
    if (!actionButtons.length) return;
    if (activeButtonIndex < 0) {
      highlightIndex(delta === 1 ? 0 : actionButtons.length - 1);
      return;
    }
    highlightIndex(activeButtonIndex + delta);
  };

  const copyWithFeedback = async (text: string, feedback = '已复制到剪贴板') => {
    try {
      await writeClipboard(text);
      notify(feedback);
    } catch {
      notify('复制失败，无法访问剪贴板');
    }
  };

  const buildEditableMenu = (
    editable: EditableControl | HTMLElement,
    snapshot: SavedSelectionSnapshot,
  ): MenuEntry[] => {
    const shortcutMod = isMacOS() ? '⌘' : 'Ctrl+';

    if (snapshot.kind === 'control') {
      const control = snapshot.control;
      const isPassword = control instanceof HTMLInputElement && control.type.toLowerCase() === 'password';
      const isWritable = !control.disabled && !control.readOnly;
      const hasSelection = snapshot.supportsRange
        ? snapshot.end > snapshot.start
        : Boolean(window.getSelection()?.toString());
      const hasValue = control.value.length > 0;

      return [
        {
          type: 'action',
          id: 'ctx-cut',
          label: '剪切',
          icon: 'scissors',
          shortcut: `${shortcutMod}X`,
          disabled: !isWritable || !hasSelection || isPassword,
          onSelect: async () => {
            if (!isWritable || isPassword || !snapshot.supportsRange || snapshot.end <= snapshot.start) return;
            const current = control.value;
            const selected = current.slice(snapshot.start, snapshot.end);
            await copyWithFeedback(selected, '已剪切到剪贴板');
            control.focus({ preventScroll: true });
            control.value = current.slice(0, snapshot.start) + current.slice(snapshot.end);
            setInputSelectionRangeSafe(control, snapshot.start, snapshot.start);
            dispatchValueChanged(control, 'deleteByCut');
          },
        },
        {
          type: 'action',
          id: 'ctx-copy',
          label: '复制',
          icon: 'copy',
          shortcut: `${shortcutMod}C`,
          disabled: !hasSelection || isPassword,
          onSelect: async () => {
            if (isPassword) return;
            const selected = snapshot.supportsRange
              ? control.value.slice(snapshot.start, snapshot.end)
              : window.getSelection()?.toString() || '';
            if (selected) await copyWithFeedback(selected);
          },
        },
        {
          type: 'action',
          id: 'ctx-paste',
          label: '粘贴',
          icon: 'paste',
          shortcut: `${shortcutMod}V`,
          disabled: !isWritable,
          onSelect: async () => {
            if (!isWritable) return;
            let clipText = '';
            try {
              clipText = await readClipboard();
            } catch {
              notify('无法读取剪贴板内容');
              return;
            }
            if (!clipText) return;
            control.focus({ preventScroll: true });
            const current = control.value;
            const start = snapshot.supportsRange ? snapshot.start : current.length;
            const end = snapshot.supportsRange ? snapshot.end : current.length;
            const maxLen = control.maxLength;
            const replacedLen = current.length - (end - start);
            const insertText =
              maxLen > 0 ? clipText.slice(0, Math.max(0, maxLen - replacedLen)) : clipText;
            if (!insertText && maxLen > 0 && replacedLen >= maxLen) {
              notify('已达到最大输入长度');
              return;
            }
            control.value = current.slice(0, start) + insertText + current.slice(end);
            const nextCaret = start + insertText.length;
            setInputSelectionRangeSafe(control, nextCaret, nextCaret);
            dispatchValueChanged(control, 'insertFromPaste');
          },
        },
        { type: 'divider' },
        {
          type: 'action',
          id: 'ctx-select-all',
          label: '全选',
          icon: 'select-all',
          shortcut: `${shortcutMod}A`,
          disabled: !hasValue,
          onSelect: () => {
            control.focus({ preventScroll: true });
            try {
              control.select();
            } catch {
              setInputSelectionRangeSafe(control, 0, control.value.length);
            }
          },
        },
        {
          type: 'action',
          id: 'ctx-clear',
          label: '清空',
          icon: 'trash',
          disabled: !isWritable || !hasValue,
          onSelect: () => {
            if (!isWritable) return;
            control.focus({ preventScroll: true });
            control.value = '';
            setInputSelectionRangeSafe(control, 0, 0);
            dispatchValueChanged(control, 'deleteContent');
          },
        },
      ];
    }

    const host = editable;
    const isWritable = host.isContentEditable;
    const selectedText = snapshot.range?.toString() || window.getSelection()?.toString() || '';
    const hasSelection = selectedText.length > 0;
    const hasContent = (host.textContent || '').length > 0;

    return [
      {
        type: 'action',
        id: 'ctx-cut',
        label: '剪切',
        icon: 'scissors',
        shortcut: `${shortcutMod}X`,
        disabled: !isWritable || !hasSelection,
        onSelect: async () => {
          if (!isWritable || !snapshot.range || snapshot.range.collapsed) return;
          const text = snapshot.range.toString();
          await copyWithFeedback(text, '已剪切到剪贴板');
          host.focus({ preventScroll: true });
          snapshot.range.deleteContents();
          dispatchValueChanged(host, 'deleteByCut');
        },
      },
      {
        type: 'action',
        id: 'ctx-copy',
        label: '复制',
        icon: 'copy',
        shortcut: `${shortcutMod}C`,
        disabled: !hasSelection,
        onSelect: async () => {
          if (selectedText) await copyWithFeedback(selectedText);
        },
      },
      {
        type: 'action',
        id: 'ctx-paste',
        label: '粘贴',
        icon: 'paste',
        shortcut: `${shortcutMod}V`,
        disabled: !isWritable,
        onSelect: async () => {
          if (!isWritable) return;
          let clipText = '';
          try {
            clipText = await readClipboard();
          } catch {
            notify('无法读取剪贴板内容');
            return;
          }
          if (!clipText) return;
          host.focus({ preventScroll: true });
          const sel = window.getSelection();
          const range = snapshot.range ?? (sel && sel.rangeCount > 0 ? sel.getRangeAt(0) : null);
          if (range && host.contains(range.commonAncestorContainer)) {
            range.deleteContents();
            const textNode = document.createTextNode(clipText);
            range.insertNode(textNode);
            range.setStartAfter(textNode);
            range.collapse(true);
            sel?.removeAllRanges();
            sel?.addRange(range);
          } else {
            host.appendChild(document.createTextNode(clipText));
          }
          dispatchValueChanged(host, 'insertFromPaste');
        },
      },
      { type: 'divider' },
      {
        type: 'action',
        id: 'ctx-select-all',
        label: '全选',
        icon: 'select-all',
        shortcut: `${shortcutMod}A`,
        disabled: !hasContent,
        onSelect: () => {
          host.focus({ preventScroll: true });
          const range = document.createRange();
          range.selectNodeContents(host);
          const sel = window.getSelection();
          sel?.removeAllRanges();
          sel?.addRange(range);
        },
      },
      {
        type: 'action',
        id: 'ctx-clear',
        label: '清空',
        icon: 'trash',
        disabled: !isWritable || !hasContent,
        onSelect: () => {
          if (!isWritable) return;
          host.focus({ preventScroll: true });
          host.textContent = '';
          dispatchValueChanged(host, 'deleteContent');
        },
      },
    ];
  };

  const buildPageQuickMenu = (activePage: string): MenuEntry[] => {
    const entries: MenuEntry[] = [];
    const modalOpen = Boolean(document.querySelector('.modal-overlay:not(.hidden), dialog[open]'));
    if (modalOpen) {
      return entries;
    }

    if (options.triggerNetworkCheck) {
      entries.push({
        type: 'action',
        id: 'ctx-check-network',
        label: '立即检测网络',
        icon: 'refresh',
        onSelect: () => options.triggerNetworkCheck?.(),
      });
    }

    switch (activePage) {
      case 'dashboard': {
        const btnLogin = document.getElementById('btn-login') as HTMLButtonElement | null;
        if (options.runPrimaryPageAction && btnLogin && !btnLogin.disabled) {
          entries.push({
            type: 'action',
            id: 'ctx-dashboard-login',
            label: '立即认证',
            icon: 'activity',
            onSelect: () => options.runPrimaryPageAction?.('dashboard'),
          });
        }
        if (options.openBillingCenter || options.openPage) {
          entries.push({
            type: 'action',
            id: 'ctx-open-billing',
            label: '打开计费中心',
            icon: 'receipt',
            onSelect: () => {
              if (options.openBillingCenter) options.openBillingCenter();
              else options.openPage?.('billing-center');
            },
          });
        }
        break;
      }
      case 'billing-center': {
        if (options.runPrimaryPageAction) {
          entries.push({
            type: 'action',
            id: 'ctx-refresh-billing',
            label: '刷新计费数据',
            icon: 'refresh',
            onSelect: () => options.runPrimaryPageAction?.('billing-center'),
          });
        }
        if (options.openPage) {
          entries.push({
            type: 'action',
            id: 'ctx-back-dashboard',
            label: '返回控制台',
            icon: 'home',
            onSelect: () => options.openPage?.('dashboard'),
          });
        }
        break;
      }
      case 'diagnostics': {
        const hasReport = options.hasDiagnosticReport
          ? options.hasDiagnosticReport()
          : !document.querySelector<HTMLButtonElement>('#btn-copy-diagnostics')?.disabled;
        if (options.runDiagnostics || options.runPrimaryPageAction) {
          entries.push({
            type: 'action',
            id: 'ctx-run-diagnostics',
            label: '开始网络诊断',
            icon: 'activity',
            onSelect: () =>
              options.runDiagnostics
                ? options.runDiagnostics()
                : options.runPrimaryPageAction?.('diagnostics'),
          });
        }
        if (options.copyDiagnosticReport) {
          entries.push({
            type: 'action',
            id: 'ctx-copy-diagnostics',
            label: '复制诊断报告',
            icon: 'copy',
            disabled: !hasReport,
            onSelect: () => options.copyDiagnosticReport?.(),
          });
        }
        break;
      }
      case 'logs': {
        if (options.openSearchLogs) {
          entries.push({
            type: 'action',
            id: 'ctx-search-logs',
            label: '搜索日志',
            icon: 'search',
            onSelect: () => options.openSearchLogs?.(),
          });
        }
        if (options.exportLogs) {
          entries.push({
            type: 'action',
            id: 'ctx-export-logs',
            label: '导出日志',
            icon: 'download',
            onSelect: () => options.exportLogs?.(),
          });
        }
        if (options.copyAllFilteredLogs) {
          entries.push({
            type: 'action',
            id: 'ctx-copy-all-logs',
            label: '复制全部筛选日志',
            icon: 'copy',
            onSelect: () => options.copyAllFilteredLogs?.(),
          });
        }
        break;
      }
      case 'accounts': {
        if (options.openAddAccount || options.runPrimaryPageAction) {
          entries.push({
            type: 'action',
            id: 'ctx-add-account',
            label: '添加账号',
            icon: 'plus',
            onSelect: () =>
              options.openAddAccount
                ? options.openAddAccount()
                : options.runPrimaryPageAction?.('accounts'),
          });
        }
        break;
      }
      case 'settings': {
        if (options.openLogs || options.openPage) {
          entries.push({
            type: 'action',
            id: 'ctx-goto-logs',
            label: '前往运行日志',
            icon: 'logs',
            onSelect: () => {
              if (options.openLogs) options.openLogs();
              else options.openPage?.('logs');
            },
          });
        }
        break;
      }
      default:
        break;
    }

    if (activePage !== 'logs' && activePage !== 'settings' && (options.openLogs || options.openPage)) {
      if (entries.length) entries.push({ type: 'divider' });
      entries.push({
        type: 'action',
        id: 'ctx-open-logs',
        label: '查看运行日志',
        icon: 'logs',
        onSelect: () => {
          if (options.openLogs) options.openLogs();
          else options.openPage?.('logs');
        },
      });
    }

    return entries;
  };

  const buildMenuEntries = (target: EventTarget | null): MenuEntry[] => {
    const element = resolveElement(target);
    const shortcutMod = isMacOS() ? '⌘' : 'Ctrl+';

    // 1. Editable Input / Textarea / ContentEditable
    const editable = resolveEditableTarget(element);
    if (editable) {
      return buildEditableMenu(editable, captureEditableSnapshot(editable));
    }

    const sel = window.getSelection();
    const selectedText = sel && !sel.isCollapsed ? sel.toString() : '';

    // 2. Log Entry (.log-entry[data-log-index])
    const logEntryEl = element?.closest<HTMLElement>('.log-entry');
    if (logEntryEl) {
      const entries: MenuEntry[] = [];
      if (selectedText) {
        entries.push({
          type: 'action',
          id: 'ctx-copy-selection',
          label: '复制选中内容',
          icon: 'copy',
          shortcut: `${shortcutMod}C`,
          onSelect: () => copyWithFeedback(selectedText),
        });
      }
      const rawIndex = logEntryEl.dataset.logIndex;
      const parsedIndex = rawIndex !== undefined ? Number.parseInt(rawIndex, 10) : Number.NaN;
      const entryData = Number.isFinite(parsedIndex)
        ? options.getLogEntryByIndex?.(parsedIndex)
        : null;
      const lineText = entryData
        ? `[${entryData.time}] [${entryData.module}] ${entryData.message}`
        : logEntryEl.textContent?.trim() || '';
      if (lineText) {
        entries.push({
          type: 'action',
          id: 'ctx-copy-log-line',
          label: '复制此条日志',
          icon: 'logs',
          onSelect: () => copyWithFeedback(lineText, '已复制此条日志'),
        });
      }
      const moduleName =
        entryData?.module ||
        logEntryEl.dataset.logModule?.trim() ||
        extractBracketModule(logEntryEl.textContent);
      if (moduleName && options.filterLogsByModule) {
        entries.push({
          type: 'action',
          id: 'ctx-filter-log-module',
          label: `按「${formatShortLabel(moduleName)}」筛选`,
          icon: 'filter',
          onSelect: () => options.filterLogsByModule?.(moduleName),
        });
      }
      if (options.copyAllFilteredLogs) {
        entries.push({ type: 'divider' });
        entries.push({
          type: 'action',
          id: 'ctx-copy-filtered-logs',
          label: '复制全部筛选日志',
          icon: 'copy',
          onSelect: () => options.copyAllFilteredLogs?.(),
        });
      }
      return entries;
    }

    // 3. Text Selection (outside inputs & log entries)
    if (selectedText) {
      const entries: MenuEntry[] = [
        {
          type: 'action',
          id: 'ctx-copy-selection',
          label: '复制',
          icon: 'copy',
          shortcut: `${shortcutMod}C`,
          onSelect: () => copyWithFeedback(selectedText),
        },
      ];
      if (isTextSelectionAllowed(target)) {
        const container = resolveSelectableContainer(element);
        if (container && (container.textContent || '').trim()) {
          entries.push({
            type: 'action',
            id: 'ctx-select-region',
            label: '全选',
            icon: 'select-all',
            onSelect: () => {
              const range = document.createRange();
              range.selectNodeContents(container);
              const selection = window.getSelection();
              selection?.removeAllRanges();
              selection?.addRange(range);
            },
          });
        }
      }
      return entries;
    }

    // 4. Copyable Info Field / Card Value
    const copyable = extractCopyableField(element);
    if (copyable) {
      const entries: MenuEntry[] = [
        {
          type: 'action',
          id: 'ctx-copy-field',
          label: formatCopyMenuLabel(copyable.preview),
          icon: 'copy',
          onSelect: () => copyWithFeedback(copyable.text),
        },
      ];
      const stepEl = element?.closest<HTMLElement>('.diagnostic-step');
      const detailsBtn = stepEl?.querySelector<HTMLButtonElement>('.diagnostic-details-button');
      if (detailsBtn && !detailsBtn.disabled) {
        entries.push({
          type: 'action',
          id: 'ctx-step-details',
          label: '查看详细信息',
          icon: 'info',
          onSelect: () => detailsBtn.click(),
        });
      }
      return entries;
    }

    // 5. Blank / General Area: lightweight page-aware quick menu
    return buildPageQuickMenu(options.getActivePage());
  };

  const renderAndOpen = (entries: MenuEntry[], anchorX: number, anchorY: number) => {
    clearCloseTimer();
    menu.replaceChildren();
    actionButtons = [];
    activeButtonIndex = -1;

    for (const entry of entries) {
      if (entry.type === 'divider') {
        const sep = document.createElement('div');
        sep.className = 'desktop-context-menu-divider';
        sep.setAttribute('role', 'separator');
        menu.appendChild(sep);
        continue;
      }
      if (entry.type === 'label') {
        const lbl = document.createElement('div');
        lbl.className = 'desktop-context-menu-label';
        lbl.textContent = entry.text;
        menu.appendChild(lbl);
        continue;
      }

      const btn = document.createElement('button');
      btn.type = 'button';
      btn.id = entry.id;
      btn.className = 'desktop-context-menu-item';
      if (entry.danger) btn.classList.add('is-danger');
      btn.setAttribute('role', 'menuitem');
      if (entry.disabled) {
        btn.disabled = true;
        btn.setAttribute('aria-disabled', 'true');
      }

      const iconSvg = createIconSvg(entry.icon);
      const labelSpan = document.createElement('span');
      labelSpan.className = 'desktop-context-menu-item-label';
      labelSpan.textContent = entry.label;
      btn.append(iconSvg, labelSpan);

      if (entry.shortcut) {
        const shortcutSpan = document.createElement('span');
        shortcutSpan.className = 'desktop-context-menu-shortcut';
        shortcutSpan.textContent = entry.shortcut;
        btn.appendChild(shortcutSpan);
      }

      if (!entry.disabled) {
        const buttonIndex = actionButtons.length;
        actionButtons.push(btn);
        btn.addEventListener('pointerenter', () => {
          activeButtonIndex = buttonIndex;
        });
        btn.addEventListener('click', event => {
          event.stopPropagation();
          close(false);
          void entry.onSelect();
        });
      }

      menu.appendChild(btn);
    }

    returnFocusEl = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    menu.classList.remove('is-open', 'is-closing');
    menu.hidden = false;
    menu.inert = false;
    menu.style.visibility = 'hidden';

    if (supportsPopover()) {
      menu.classList.remove('is-fallback-layer');
      try {
        if (!menu.matches(':popover-open')) {
          (menu as HTMLElement & { showPopover: () => void }).showPopover();
        }
      } catch {
        menu.classList.add('is-fallback-layer');
      }
    } else {
      menu.classList.add('is-fallback-layer');
    }

    const vv = window.visualViewport;
    const vpLeft = vv?.offsetLeft ?? 0;
    const vpTop = vv?.offsetTop ?? 0;
    const vpWidth = vv?.width ?? document.documentElement.clientWidth;
    const vpHeight = vv?.height ?? window.innerHeight;
    const pad = 8;

    const rect = menu.getBoundingClientRect();
    const menuWidth = Math.ceil(rect.width || 200);
    const menuHeight = Math.ceil(rect.height || 160);

    const maxLeft = Math.max(vpLeft + pad, vpLeft + vpWidth - menuWidth - pad);
    const maxTop = Math.max(vpTop + pad, vpTop + vpHeight - menuHeight - pad);

    const flipX = anchorX + menuWidth + pad > vpLeft + vpWidth && anchorX - menuWidth >= vpLeft + pad;
    const flipY = anchorY + menuHeight + pad > vpTop + vpHeight && anchorY - menuHeight >= vpTop + pad;

    const rawLeft = flipX ? anchorX - menuWidth : anchorX;
    const rawTop = flipY ? anchorY - menuHeight : anchorY;

    const left = Math.round(Math.min(Math.max(rawLeft, vpLeft + pad), maxLeft));
    const top = Math.round(Math.min(Math.max(rawTop, vpTop + pad), maxTop));

    menu.style.left = `${left}px`;
    menu.style.top = `${top}px`;
    menu.style.transformOrigin = `${flipY ? 'bottom' : 'top'} ${flipX ? 'right' : 'left'}`;
    menu.style.visibility = '';

    isOpen = true;
    openedAt = performance.now();
    requestAnimationFrame(() => {
      if (!isOpen) return;
      menu.classList.add('is-open');
      menu.focus({ preventScroll: true });
    });
  };

  const onContextMenu = (event: MouseEvent) => {
    // Allow native browser Inspect Element in dev mode when Shift is held.
    if (isDevEnvironment() && event.shiftKey) {
      close(false);
      return;
    }

    event.preventDefault();

    const entries = buildMenuEntries(event.target);
    if (!entries.length) {
      close(false);
      return;
    }

    let x = event.clientX;
    let y = event.clientY;
    // Support keyboard invocation (ContextMenu key / Shift+F10) where coordinates are (0, 0).
    if (x === 0 && y === 0 && event.target instanceof Element) {
      const targetRect = event.target.getBoundingClientRect();
      if (targetRect.width > 0 || targetRect.height > 0) {
        x = targetRect.left + Math.min(24, targetRect.width / 2);
        y = targetRect.bottom + 4;
      }
    }

    renderAndOpen(entries, x, y);
  };

  const onPointerDown = (event: PointerEvent) => {
    if (!isOpen) return;
    if (event.target instanceof Node && menu.contains(event.target)) return;
    close(false);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (!isOpen) return;

    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        close(true);
        break;
      case 'ArrowDown':
        event.preventDefault();
        event.stopPropagation();
        moveHighlight(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        event.stopPropagation();
        moveHighlight(-1);
        break;
      case 'Home':
        event.preventDefault();
        event.stopPropagation();
        highlightIndex(0);
        break;
      case 'End':
        event.preventDefault();
        event.stopPropagation();
        highlightIndex(actionButtons.length - 1);
        break;
      case 'Enter':
      case ' ':
        if (activeButtonIndex >= 0 && actionButtons[activeButtonIndex]) {
          event.preventDefault();
          event.stopPropagation();
          actionButtons[activeButtonIndex].click();
        }
        break;
      case 'Tab':
        close(false);
        break;
      default:
        break;
    }
  };

  const onScroll = (event: Event) => {
    if (!isOpen) return;
    if (performance.now() - openedAt < 80) return;
    if (event.target instanceof Node && menu.contains(event.target)) return;
    close(false);
  };

  const onViewportChange = () => {
    if (isOpen) close(false);
  };

  document.addEventListener('contextmenu', onContextMenu);
  document.addEventListener('pointerdown', onPointerDown, true);
  document.addEventListener('keydown', onKeyDown, true);
  document.addEventListener('scroll', onScroll, true);
  window.addEventListener('resize', onViewportChange, { passive: true });
  window.addEventListener('blur', onViewportChange);
  window.visualViewport?.addEventListener('resize', onViewportChange);

  const handle: DesktopContextMenuHandle = {
    close: () => close(false),
    destroy: () => {
      document.removeEventListener('contextmenu', onContextMenu);
      document.removeEventListener('pointerdown', onPointerDown, true);
      document.removeEventListener('keydown', onKeyDown, true);
      document.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onViewportChange);
      window.removeEventListener('blur', onViewportChange);
      window.visualViewport?.removeEventListener('resize', onViewportChange);
      finishHide();
      menu.remove();
      if (activeContextMenuHandle === handle) {
        activeContextMenuHandle = null;
      }
    },
  };

  activeContextMenuHandle = handle;
  return handle;
}
