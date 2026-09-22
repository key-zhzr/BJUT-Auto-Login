/** Keep the Android scroll area above the keyboard, without a floating nav bar. */
export function setupAndroidKeyboard() {
  if (!document.body.classList.contains('is-android')) return;
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
