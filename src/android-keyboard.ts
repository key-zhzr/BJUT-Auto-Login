/** Keep the Android scroll area edge-to-edge with native system-bar insets and
 * stay above the keyboard, without a floating nav bar. */
export function setupAndroidKeyboard() {
  if (!document.body.classList.contains('is-android')) return;
  let lastInsetsKey = '';
  const applySafeAreaInsets = (top: number, right: number, bottom: number, left: number) => {
    const clean = (v: unknown) => {
      const n = Number(v);
      return Number.isFinite(n) && n > 0 ? +n.toFixed(2) : 0;
    };
    const t = clean(top);
    const r = clean(right);
    const b = clean(bottom);
    const l = clean(left);
    const key = `${t},${r},${b},${l}`;
    if (key === lastInsetsKey) return;
    lastInsetsKey = key;
    const style = document.documentElement.style;
    style.setProperty('--android-inset-top', `${t}px`);
    style.setProperty('--android-inset-right', `${r}px`);
    style.setProperty('--android-inset-bottom', `${b}px`);
    style.setProperty('--android-inset-left', `${l}px`);
    window.dispatchEvent(new CustomEvent('bjut-safe-area-change'));
  };

  window.__nativeSafeAreaInsets = applySafeAreaInsets;
  const rawInsets = window.AndroidBridge?.getSafeAreaInsets?.();
  if (rawInsets) {
    try {
      const parsed = JSON.parse(rawInsets) as Partial<Record<'top' | 'right' | 'bottom' | 'left', number>>;
      applySafeAreaInsets(parsed.top ?? 0, parsed.right ?? 0, parsed.bottom ?? 0, parsed.left ?? 0);
    } catch {
      // Ignore malformed native inset payloads.
    }
  }

  let nativeVisible: boolean | null = null;
  let nativeHeight = 0;
  let baseline = window.innerHeight;
  let frame: number | null = null;
  const update = () => {
    frame = null;
    const viewport = window.visualViewport;
    const height = Math.min(window.innerHeight, viewport?.height || window.innerHeight);
    const input = document.activeElement;
    const editing = input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement
      && !['checkbox', 'radio', 'button', 'submit', 'file', 'range'].includes(input.type);
    if (nativeVisible !== true && !editing) baseline = window.innerHeight;
    const visible = nativeVisible ?? (editing && baseline - height > 120);
    document.documentElement.classList.toggle('keyboard-open', visible);
    document.body.classList.toggle('keyboard-open', visible);
    if (visible) {
      const available = nativeVisible && nativeHeight > 0 ? Math.min(height, nativeHeight) : height;
      document.documentElement.style.setProperty('--app-viewport-height', `${Math.max(1, available)}px`);
      // The page itself never pans into the hidden navigation reserve; main and
      // modal content remain the only scroll containers while editing.
      if (window.scrollY) window.scrollTo(0, 0);
    } else {
      document.documentElement.style.removeProperty('--app-viewport-height');
      baseline = window.innerHeight;
    }
  };
  const schedule = () => { if (frame === null) frame = requestAnimationFrame(update); };
  window.__nativeKeyboardChanged = (visible, availableHeight) => {
    nativeVisible = visible; nativeHeight = availableHeight; schedule();
  };
  window.visualViewport?.addEventListener('resize', schedule);
  window.visualViewport?.addEventListener('scroll', schedule);
  window.addEventListener('resize', schedule);
  document.addEventListener('focusin', schedule);
  document.addEventListener('focusout', schedule);
  schedule();
}
