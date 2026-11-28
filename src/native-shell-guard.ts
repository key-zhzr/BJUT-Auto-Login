import { IS_ANDROID } from './platform';

interface NativeGuardAndroidBridge {
  setLongPressAllowed?: (allowed: boolean) => void;
}

export interface NativeShellGuardOptions {
  /** Optional handler for Ctrl/Cmd+F on desktop; return true if handled by app UI. */
  onFindShortcut?: () => boolean;
}

export interface NativeShellGuardHandle {
  destroy: () => void;
}

const NON_TEXT_INPUT_TYPES = new Set([
  'checkbox',
  'radio',
  'button',
  'submit',
  'reset',
  'file',
  'range',
  'color',
  'hidden',
  'image',
]);

const NON_SELECTABLE_CONTROL_SELECTOR = [
  'button',
  '[role="button"]',
  '.btn',
  'a.btn',
  '.nav-item',
  '.nav-tree-link',
  '.billing-section-nav-button',
  '.switch',
  'summary',
  '.custom-select-trigger',
  '.diagnostic-details-button',
  '.diagnostic-step-marker',
  '.titlebar',
  '.drag-handle',
  '.badge',
  '.status-badge',
  '.health-badge',
  '.account-health-badge',
  '.billing-state-badge',
  '.billing-count-badge',
  'svg',
].join(', ');

const EXPLICIT_SELECTABLE_SELECTOR = [
  'code',
  'pre',
  '#logs-content',
  '.logs-container',
  '.log-entry',
  '.router-log-content',
  '.diagnostic-card-message',
  '.diagnostic-advice',
  '.diagnostic-step-info',
  '.diagnostic-panel-item-message',
  '.diagnostic-panel-item-detail',
  '.diagnostic-details-body',
  '.trace-hop',
  '#network-events-list',
  '.status-value',
  '.account-username',
  '.account-meta',
  '.account-user h4',
  '.account-health-reason',
  '.billing-summary-value',
  '.billing-detail-value',
  '.billing-summary-card strong',
  '.billing-detail-item strong',
  '.billing-record-row',
  '.billing-record-meta',
  '#billing-center td',
  '#billing-center th',
  '.security-info-value',
  '.security-info-box',
  '.network-info-value',
  '.dashboard-network-info',
  '.info-content p',
  '.error-message',
  '[role="alert"]',
  '.tos-content',
  '.tos-modal-body',
  '#update-modal-meta',
  '#update-release-notes',
  '.router-uplink-card p',
  '#router-message',
  '.router-port-account',
  '.router-port-traffic',
  '[data-selectable-text="true"]',
].join(', ');

const DRAG_ALLOWED_SELECTOR = [
  '[draggable="true"]',
  '.sortable-chosen',
  '.dragging',
  '.dragging-fallback',
  'input',
  'textarea',
].join(', ');

function isDevEnvironment(): boolean {
  return Boolean((import.meta as ImportMeta & { env?: { DEV?: boolean } }).env?.DEV);
}

function shouldBypassProductionShortcuts(): boolean {
  return isDevEnvironment() && !navigator.webdriver;
}

function resolveElement(target: EventTarget | null): Element | null {
  if (target instanceof Element) return target;
  if (target instanceof Node) return target.parentElement;
  return null;
}

/** Returns true if `el` (or its active editable host) is a text-editable form field or contenteditable. */
export function isEditableElement(el: Element | null): boolean {
  if (!el) return false;
  if (el instanceof HTMLTextAreaElement) return true;
  if (el instanceof HTMLInputElement) {
    const type = (el.type || 'text').toLowerCase();
    return !NON_TEXT_INPUT_TYPES.has(type);
  }
  const ceHost = el.closest('[contenteditable]');
  if (ceHost) {
    const attr = ceHost.getAttribute('contenteditable')?.toLowerCase();
    if (attr === '' || attr === 'true' || attr === 'plaintext-only') {
      return true;
    }
  }
  return el instanceof HTMLElement && el.isContentEditable;
}

/**
 * Returns true if `target` should allow text selection (and on Android, native long-press ActionMode).
 * Interactive controls (buttons, switches, nav tabs, triggers, SVGs) always return false even when
 * placed inside a selectable parent container.
 */
export function isTextSelectionAllowed(target: EventTarget | null): boolean {
  const element = resolveElement(target);
  if (!element) return false;

  if (isEditableElement(element)) return true;

  if (element.closest(NON_SELECTABLE_CONTROL_SELECTOR)) {
    return false;
  }

  if (element.closest(EXPLICIT_SELECTABLE_SELECTOR)) {
    if (element.matches('.logs-container, #logs-content') && !element.querySelector('.log-entry')) {
      return false;
    }
    return true;
  }

  const style = getComputedStyle(element) as CSSStyleDeclaration & { webkitUserSelect?: string };
  const userSelect = style.userSelect || style.webkitUserSelect || '';
  return userSelect === 'text';
}

function activeBridge(): NativeGuardAndroidBridge | undefined {
  return window.AndroidBridge as (typeof window.AndroidBridge & NativeGuardAndroidBridge) | undefined;
}

let activeGuardHandle: NativeShellGuardHandle | null = null;

/**
 * Install cross-platform WebView hardening:
 * - Android: syncs `AndroidBridge.setLongPressAllowed(allowed)` per touch/pointer and suppresses
 *   browser callouts/text selection outside inputs & explicitly selectable containers.
 * - Desktop & All platforms: suppresses pinch-zoom, Ctrl/Cmd+wheel zoom, non-intentional drag
 *   ghosts, non-text `selectstart`, and browser navigation/reload/print/save shortcuts in production.
 */
export function setupNativeShellGuard(options: NativeShellGuardOptions = {}): NativeShellGuardHandle {
  if (activeGuardHandle) {
    activeGuardHandle.destroy();
  }

  const cleanups: Array<() => void> = [];
  const addListener = <K extends keyof DocumentEventMap>(
    target: Document | Window,
    type: K | string,
    listener: EventListener,
    listenerOptions?: AddEventListenerOptions | boolean,
  ) => {
    target.addEventListener(type, listener, listenerOptions);
    cleanups.push(() => target.removeEventListener(type, listener, listenerOptions));
  };

  let multiTouchMoveAttached = false;
  const onMultiTouchMove = (event: Event) => {
    const touchEvent = event as TouchEvent;
    if (touchEvent.touches && touchEvent.touches.length > 1 && touchEvent.cancelable) {
      touchEvent.preventDefault();
    }
  };
  const detachMultiTouchMove = () => {
    if (!multiTouchMoveAttached) return;
    multiTouchMoveAttached = false;
    document.removeEventListener('touchmove', onMultiTouchMove, true);
  };
  cleanups.push(detachMultiTouchMove);

  const preventIfCancelable = (event: Event) => {
    if (event.cancelable) event.preventDefault();
  };

  // Multi-touch pinch zoom protection: only attach non-passive touchmove while 2+ fingers are down
  // so single-finger scroll performance remains completely unaffected.
  addListener(
    document,
    'touchstart',
    event => {
      const touchEvent = event as TouchEvent;
      if (touchEvent.touches.length > 1 && !multiTouchMoveAttached) {
        multiTouchMoveAttached = true;
        document.addEventListener('touchmove', onMultiTouchMove, { capture: true, passive: false });
      }
      if (IS_ANDROID) {
        activeBridge()?.setLongPressAllowed?.(isTextSelectionAllowed(event.target));
      }
    },
    { capture: true, passive: true },
  );

  addListener(
    document,
    'touchend',
    event => {
      const touchEvent = event as TouchEvent;
      if (touchEvent.touches.length <= 1) {
        detachMultiTouchMove();
      }
    },
    { capture: true, passive: true },
  );

  addListener(
    document,
    'touchcancel',
    event => {
      const touchEvent = event as TouchEvent;
      if (touchEvent.touches.length <= 1) {
        detachMultiTouchMove();
      }
      if (IS_ANDROID && !isTextSelectionAllowed(event.target) && !window.getSelection()?.toString()) {
        activeBridge()?.setLongPressAllowed?.(false);
      }
    },
    { capture: true, passive: true },
  );

  if (IS_ANDROID) {
    document.documentElement.style.setProperty('-webkit-tap-highlight-color', 'transparent');
    document.documentElement.style.setProperty('-webkit-touch-callout', 'none');

    addListener(
      document,
      'pointerdown',
      event => {
        activeBridge()?.setLongPressAllowed?.(isTextSelectionAllowed(event.target));
      },
      { capture: true, passive: true },
    );

    addListener(
      document,
      'pointercancel',
      event => {
        if (!isTextSelectionAllowed(event.target) && !window.getSelection()?.toString()) {
          activeBridge()?.setLongPressAllowed?.(false);
        }
      },
      { capture: true, passive: true },
    );

    addListener(
      document,
      'contextmenu',
      event => {
        const element = resolveElement(event.target);
        if (!isEditableElement(element) && !isTextSelectionAllowed(event.target)) {
          event.preventDefault();
        }
      },
      true,
    );
  }

  // Safari / WKWebView pinch-zoom gesture events.
  addListener(document, 'gesturestart', preventIfCancelable, { capture: true, passive: false });
  addListener(document, 'gesturechange', preventIfCancelable, { capture: true, passive: false });

  // Desktop Ctrl/Meta + wheel zoom (including trackpad pinch-zoom).
  addListener(
    document,
    'wheel',
    event => {
      const wheelEvent = event as WheelEvent;
      if ((wheelEvent.ctrlKey || wheelEvent.metaKey) && wheelEvent.cancelable) {
        wheelEvent.preventDefault();
      }
    },
    { capture: true, passive: false },
  );

  // Prevent browser drag ghosts on icons, SVG glyphs, links, and cards while preserving SortableJS & explicit drags.
  addListener(
    document,
    'dragstart',
    event => {
      const target = resolveElement(event.target);
      if (target && target.closest(DRAG_ALLOWED_SELECTOR)) {
        return;
      }
      event.preventDefault();
    },
    true,
  );

  // Suppress accidental selection outside inputs & explicitly selectable text containers.
  addListener(
    document,
    'selectstart',
    event => {
      if (!isTextSelectionAllowed(event.target)) {
        event.preventDefault();
      }
    },
    true,
  );

  // Suppress browser-like shortcuts in production builds (and automated test sessions) while preserving editing & accessibility keys.
  addListener(
    document,
    'keydown',
    event => {
      const keyEvent = event as KeyboardEvent;
      const key = keyEvent.key;
      const lowerKey = key.toLowerCase();
      const mod = keyEvent.ctrlKey || keyEvent.metaKey;

      if (shouldBypassProductionShortcuts()) {
        if (mod && !keyEvent.altKey && lowerKey === 'f' && options.onFindShortcut?.()) {
          keyEvent.preventDefault();
        }
        return;
      }

      // Block DevTools inspection shortcuts in production before standard clipboard allowlist
      // so Ctrl+Shift+C does not pass through as copy.
      if (
        (keyEvent.ctrlKey && keyEvent.shiftKey && (lowerKey === 'i' || lowerKey === 'j' || lowerKey === 'c')) ||
        (keyEvent.metaKey && keyEvent.altKey && (lowerKey === 'i' || lowerKey === 'j' || lowerKey === 'c'))
      ) {
        keyEvent.preventDefault();
        return;
      }

      // Always preserve standard text editing & clipboard shortcuts.
      if (mod && !keyEvent.altKey && (lowerKey === 'c' || lowerKey === 'v' || lowerKey === 'x' || lowerKey === 'a' || lowerKey === 'z' || lowerKey === 'y')) {
        return;
      }

      if (key === 'F5' || key === 'F7' || key === 'F3' || key === 'F12') {
        keyEvent.preventDefault();
        return;
      }

      if (!mod && !keyEvent.ctrlKey && !keyEvent.metaKey && keyEvent.altKey && (key === 'ArrowLeft' || key === 'ArrowRight')) {
        // Keep Option+Arrow word cursor navigation inside editable fields on macOS.
        if (!isEditableElement(resolveElement(document.activeElement))) {
          keyEvent.preventDefault();
        }
        return;
      }

      if (!mod) return;

      if (lowerKey === 'f') {
        if (options.onFindShortcut?.()) {
          keyEvent.preventDefault();
          return;
        }
        keyEvent.preventDefault();
        return;
      }

      if (
        lowerKey === 'r' ||
        lowerKey === 'p' ||
        lowerKey === 's' ||
        lowerKey === 'u' ||
        lowerKey === 'g' ||
        lowerKey === 'w' ||
        key === '=' ||
        key === '+' ||
        key === '-' ||
        key === '_' ||
        key === '0' ||
        keyEvent.code === 'Equal' ||
        keyEvent.code === 'Minus' ||
        keyEvent.code === 'Digit0' ||
        keyEvent.code === 'NumpadAdd' ||
        keyEvent.code === 'NumpadSubtract' ||
        keyEvent.code === 'Numpad0'
      ) {
        keyEvent.preventDefault();
      }
    },
    true,
  );

  const handle: NativeShellGuardHandle = {
    destroy: () => {
      while (cleanups.length) {
        const cleanup = cleanups.pop();
        cleanup?.();
      }
      if (activeGuardHandle === handle) {
        activeGuardHandle = null;
      }
    },
  };

  activeGuardHandle = handle;
  return handle;
}
