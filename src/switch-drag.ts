/** Enable native-feeling horizontal hold-and-drag toggling on `.switch` controls. */
export function setupSwitchDrag(): void {
  let installedOn = (document.documentElement as HTMLElement & { __switchDragInstalled?: boolean });
  if (installedOn.__switchDragInstalled) return;
  installedOn.__switchDragInstalled = true;

  interface DragState {
    switchEl: HTMLElement;
    input: HTMLInputElement;
    pointerId: number;
    startX: number;
    startY: number;
    initialChecked: boolean;
    maxTravel: number;
    currentRatio: number;
    engaged: boolean;
  }

  let drag: DragState | null = null;
  let suppressClickSwitch: HTMLElement | null = null;

  const measureMaxTravel = (switchEl: HTMLElement): number => {
    const slider = switchEl.querySelector<HTMLElement>('.slider');
    if (!slider) return 20;
    const beforeStyle = getComputedStyle(slider, '::before');
    const thumbWidth = Number.parseFloat(beforeStyle.width) || 18;
    const leftOffset = Number.parseFloat(beforeStyle.left) || 4;
    return Math.max(10, Math.round(slider.clientWidth - thumbWidth - leftOffset * 2));
  };

  const cleanup = (state: DragState) => {
    state.switchEl.classList.remove('is-dragging', 'is-drag-on');
    state.switchEl.style.removeProperty('--switch-drag-x');
    if (state.switchEl.hasPointerCapture?.(state.pointerId)) {
      try { state.switchEl.releasePointerCapture(state.pointerId); } catch {}
    }
  };

  const resetMainHorizontalScroll = () => {
    const main = document.getElementById('main-content');
    if (main && main.scrollLeft !== 0) main.scrollLeft = 0;
    if (document.documentElement.scrollLeft !== 0) document.documentElement.scrollLeft = 0;
    if (document.body.scrollLeft !== 0) document.body.scrollLeft = 0;
  };

  document.addEventListener('pointerdown', event => {
    if (event.button !== 0 && event.pointerType === 'mouse') return;
    const target = event.target instanceof Element ? event.target : null;
    const switchEl = target?.closest<HTMLElement>('.switch');
    const input = switchEl?.querySelector<HTMLInputElement>('input[type="checkbox"]');
    if (!switchEl || !input || input.disabled) return;
    window.getSelection()?.removeAllRanges();
    drag = {
      switchEl,
      input,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      initialChecked: input.checked,
      maxTravel: measureMaxTravel(switchEl),
      currentRatio: input.checked ? 1 : 0,
      engaged: false,
    };
  });

  document.addEventListener('selectstart', event => {
    if (drag) event.preventDefault();
  });

  document.addEventListener('dragstart', event => {
    if (drag) event.preventDefault();
  });

  document.addEventListener('pointermove', event => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const dx = event.clientX - drag.startX;
    const dy = event.clientY - drag.startY;
    if (!drag.engaged) {
      if (Math.abs(dy) > 10 && Math.abs(dy) > Math.abs(dx)) {
        drag = null;
        return;
      }
      if (Math.abs(dx) < 4) return;
      drag.engaged = true;
      drag.switchEl.classList.add('is-dragging');
      window.getSelection()?.removeAllRanges();
      try { drag.switchEl.setPointerCapture?.(drag.pointerId); } catch {}
    }
    event.preventDefault();
    resetMainHorizontalScroll();
    const base = drag.initialChecked ? drag.maxTravel : 0;
    const clamped = Math.max(0, Math.min(drag.maxTravel, base + dx));
    drag.currentRatio = drag.maxTravel > 0 ? clamped / drag.maxTravel : 0;
    drag.switchEl.classList.toggle('is-drag-on', drag.currentRatio >= 0.5);
    drag.switchEl.style.setProperty('--switch-drag-x', `${clamped.toFixed(1)}px`);
  });

  const finish = (event: PointerEvent, commit: boolean) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const state = drag;
    drag = null;
    const wasEngaged = state.engaged;
    cleanup(state);
    if (!wasEngaged) return;
    suppressClickSwitch = state.switchEl;
    window.setTimeout(() => {
      if (suppressClickSwitch === state.switchEl) suppressClickSwitch = null;
    }, 80);
    if (!commit) return;
    const nextChecked = state.currentRatio >= 0.5;
    if (nextChecked !== state.initialChecked && !state.input.disabled) {
      state.input.checked = nextChecked;
      state.input.dispatchEvent(new Event('change', { bubbles: true }));
    }
  };

  document.addEventListener('pointerup', event => finish(event, true));
  document.addEventListener('pointercancel', event => finish(event, false));
  document.addEventListener('click', event => {
    const target = event.target instanceof Element ? event.target : null;
    const switchEl = target?.closest<HTMLElement>('.switch');
    if (switchEl && switchEl === suppressClickSwitch) {
      suppressClickSwitch = null;
      event.preventDefault();
      event.stopPropagation();
    }
  }, true);
}
