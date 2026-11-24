const MAX_TOUCH_OFFSET = 92;
const MAX_WHEEL_OFFSET = 58;
const SPRING_OMEGA = 24;
const SPRING_ZETA = 0.92;

function rubberBand(pull: number, maxOffset: number, resistance = 1.85): number {
  const abs = Math.abs(pull);
  if (abs < 0.01) return 0;
  return Math.sign(pull) * (maxOffset * abs) / (maxOffset + abs * resistance);
}

function normalizeWheelDelta(event: WheelEvent, pageHeight: number): number {
  if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) return event.deltaY * 20;
  if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) return event.deltaY * Math.max(200, pageHeight);
  return event.deltaY;
}

function getBounceTargets(mainEl: HTMLElement): HTMLElement[] {
  const activePage = mainEl.querySelector<HTMLElement>(':scope > .page.active');
  if (!activePage) return [];
  if (activePage.id === 'billing-center') {
    return Array.from(
      activePage.querySelectorAll<HTMLElement>(':scope > .page-header, :scope > .billing-workbench'),
    );
  }
  return [activePage];
}

function findScrollableChild(target: EventTarget | null, boundary: HTMLElement): HTMLElement | null {
  let node = target instanceof Element ? target as HTMLElement : null;
  while (node && node !== boundary) {
    if (node.scrollHeight > node.clientHeight + 1) {
      const style = getComputedStyle(node);
      if (style.overflowY === 'auto' || style.overflowY === 'scroll' || style.overflowY === 'overlay') {
        return node;
      }
    }
    node = node.parentElement;
  }
  return null;
}

function canConsumeVerticalScroll(scroller: HTMLElement, dy: number): boolean {
  const maxScroll = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
  if (dy > 0) return scroller.scrollTop > 0.5;
  if (dy < 0) return scroller.scrollTop < maxScroll - 0.5;
  return false;
}

function shouldIgnoreTarget(target: EventTarget | null): boolean {
  if (document.querySelector('.custom-select.open, .custom-select.closing')) {
    return true;
  }
  if (!(target instanceof Element)) return false;
  if (target.closest('.switch, .drag-handle, .sortable-chosen, input[type="range"], canvas, .custom-select-options, .modal-overlay:not(.hidden), .billing-mobile-nav, #nav')) {
    return true;
  }
  return false;
}

export function setupOverscrollBounce(): { reset: () => void } {
  const mainEl = document.getElementById('main-content');
  if (!mainEl) return { reset: () => {} };

  let pull = 0;
  let offset = 0;
  let velocity = 0;
  let springFrame: number | null = null;
  let lastSpringTime = 0;
  let paintFrame: number | null = null;
  let wheelSettleTimer: number | null = null;
  let paintedTargets: HTMLElement[] = [];

  // Touch tracking
  let activeTouchId: number | null = null;
  let touchStartX = 0;
  let touchStartY = 0;
  let touchLastY = 0;
  let directionLocked: 'x' | 'y' | null = null;
  let touchEngaged = false;
  let touchChildScroller: HTMLElement | null = null;

  const isReducedMotion = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  const clearTransforms = () => {
    for (const el of paintedTargets) {
      el.style.transform = '';
      el.style.willChange = '';
    }
    paintedTargets = [];
  };

  const paintNow = () => {
    paintFrame = null;
    const targets = getBounceTargets(mainEl);
    for (const el of paintedTargets) {
      if (!targets.includes(el)) {
        el.style.transform = '';
        el.style.willChange = '';
      }
    }
    if (Math.abs(offset) < 0.05) {
      for (const el of targets) {
        el.style.transform = '';
        el.style.willChange = '';
      }
      paintedTargets = [];
      return;
    }
    const transform = `translate3d(0, ${offset.toFixed(2)}px, 0)`;
    for (const el of targets) {
      el.style.willChange = 'transform';
      el.style.transform = transform;
    }
    paintedTargets = targets;
  };

  const schedulePaint = () => {
    if (paintFrame === null) paintFrame = requestAnimationFrame(paintNow);
  };

  const stopSpring = () => {
    if (springFrame !== null) {
      cancelAnimationFrame(springFrame);
      springFrame = null;
    }
    lastSpringTime = 0;
  };

  const reset = () => {
    stopSpring();
    if (paintFrame !== null) {
      cancelAnimationFrame(paintFrame);
      paintFrame = null;
    }
    if (wheelSettleTimer !== null) {
      window.clearTimeout(wheelSettleTimer);
      wheelSettleTimer = null;
    }
    activeTouchId = null;
    directionLocked = null;
    touchEngaged = false;
    pull = 0;
    offset = 0;
    velocity = 0;
    clearTransforms();
  };

  const startReturnSpring = () => {
    stopSpring();
    pull = 0;
    if (Math.abs(offset) < 0.25 || isReducedMotion()) {
      offset = 0;
      velocity = 0;
      paintNow();
      return;
    }
    const tick = (now: number) => {
      const dt = lastSpringTime ? Math.min((now - lastSpringTime) / 1000, 0.04) : 1 / 60;
      lastSpringTime = now;
      const alpha = SPRING_ZETA * SPRING_OMEGA;
      const omegaD = SPRING_OMEGA * Math.sqrt(1 - SPRING_ZETA * SPRING_ZETA);
      const decay = Math.exp(-alpha * dt);
      const cos = Math.cos(omegaD * dt);
      const sin = Math.sin(omegaD * dt);
      const b = (velocity + alpha * offset) / omegaD;
      const nextOffset = (offset * cos + b * sin) * decay;
      const nextVel = (velocity * cos - ((alpha * velocity + SPRING_OMEGA * SPRING_OMEGA * offset) / omegaD) * sin) * decay;
      offset = nextOffset;
      velocity = nextVel;
      if (Math.abs(offset) < 0.25 && Math.abs(velocity) < 2) {
        springFrame = null;
        lastSpringTime = 0;
        offset = 0;
        velocity = 0;
        paintNow();
        return;
      }
      paintNow();
      springFrame = requestAnimationFrame(tick);
    };
    springFrame = requestAnimationFrame(tick);
  };

  mainEl.addEventListener('touchstart', event => {
    if (activeTouchId !== null || event.touches.length !== 1) return;
    if (mainEl.classList.contains('is-page-sliding')) return;
    if (shouldIgnoreTarget(event.target)) return;
    const touch = event.touches[0];
    activeTouchId = touch.identifier;
    touchStartX = touch.clientX;
    touchStartY = touch.clientY;
    touchLastY = touch.clientY;
    directionLocked = null;
    touchEngaged = false;
    touchChildScroller = findScrollableChild(event.target, mainEl);
    stopSpring();
    if (Math.abs(offset) > 0.25) {
      // Catch mid-bounce release smoothly.
      pull = offset > 0 ? Math.min(240, offset * 2.1) : Math.max(-240, offset * 2.1);
    } else {
      pull = 0;
    }
  }, { passive: true });

  mainEl.addEventListener('touchmove', event => {
    if (activeTouchId === null) return;
    let touch: Touch | null = null;
    for (let i = 0; i < event.touches.length; i++) {
      if (event.touches[i].identifier === activeTouchId) {
        touch = event.touches[i];
        break;
      }
    }
    if (!touch) return;
    if (mainEl.classList.contains('is-page-sliding')) {
      reset();
      return;
    }

    const totalDx = touch.clientX - touchStartX;
    const totalDy = touch.clientY - touchStartY;
    if (!directionLocked) {
      if (Math.hypot(totalDx, totalDy) < 4) return;
      directionLocked = Math.abs(totalDy) > Math.abs(totalDx) ? 'y' : 'x';
    }
    if (directionLocked !== 'y') {
      activeTouchId = null;
      return;
    }

    const dy = touch.clientY - touchLastY;
    touchLastY = touch.clientY;
    if (!dy) return;

    if (!touchEngaged && touchChildScroller && canConsumeVerticalScroll(touchChildScroller, dy)) {
      return;
    }

    const maxMainScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
    const atTop = mainEl.scrollTop <= 0.5;
    const atBottom = mainEl.scrollTop >= maxMainScroll - 0.5;
    const pullingPastEdge = (atTop && dy > 0) || (atBottom && dy < 0);

    if (!touchEngaged && !pullingPastEdge && Math.abs(offset) <= 0.1) {
      return;
    }

    if (isReducedMotion()) return;

    const nextPull = pull + dy;
    if ((pull > 0 && nextPull <= 0) || (pull < 0 && nextPull >= 0)) {
      // Returned cleanly to scroll boundary -> hand control back to vertical scroll.
      pull = 0;
      offset = 0;
      touchEngaged = false;
      paintNow();
      return;
    }

    touchEngaged = true;
    if (event.cancelable) event.preventDefault();
    pull = Math.max(-280, Math.min(280, nextPull));
    offset = rubberBand(pull, MAX_TOUCH_OFFSET);
    velocity = 0;
    schedulePaint();
  }, { passive: false });

  const endTouch = (event: TouchEvent) => {
    if (activeTouchId === null) return;
    for (let i = 0; i < event.changedTouches.length; i++) {
      if (event.changedTouches[i].identifier === activeTouchId) {
        activeTouchId = null;
        directionLocked = null;
        const wasEngaged = touchEngaged;
        touchEngaged = false;
        if (wasEngaged || Math.abs(offset) >= 0.25) startReturnSpring();
        return;
      }
    }
  };
  mainEl.addEventListener('touchend', endTouch, { passive: true });
  mainEl.addEventListener('touchcancel', endTouch, { passive: true });

  mainEl.addEventListener('wheel', event => {
    if (event.defaultPrevented || event.ctrlKey || mainEl.classList.contains('is-page-sliding')) return;
    if (shouldIgnoreTarget(event.target)) return;
    if (!event.deltaY || Math.abs(event.deltaY) < Math.abs(event.deltaX)) return;

    const deltaPx = normalizeWheelDelta(event, mainEl.clientHeight);
    const dy = -deltaPx;
    if (!dy) return;

    const childScroller = findScrollableChild(event.target, mainEl);
    if (Math.abs(offset) <= 0.2 && childScroller && canConsumeVerticalScroll(childScroller, dy)) {
      return;
    }

    const maxMainScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
    const atTop = mainEl.scrollTop <= 0.5;
    const atBottom = mainEl.scrollTop >= maxMainScroll - 0.5;
    const pushingPastEdge = (atTop && dy > 0) || (atBottom && dy < 0);

    if (!pushingPastEdge && Math.abs(offset) <= 0.2) return;
    if (isReducedMotion()) return;

    const nextPull = pull + dy * 0.65;
    if ((pull > 0 && nextPull <= 0) || (pull < 0 && nextPull >= 0)) {
      pull = 0;
      offset = 0;
      if (wheelSettleTimer !== null) {
        window.clearTimeout(wheelSettleTimer);
        wheelSettleTimer = null;
      }
      paintNow();
      return;
    }

    if (event.cancelable) event.preventDefault();
    stopSpring();
    pull = Math.max(-220, Math.min(220, nextPull));
    offset = rubberBand(pull, MAX_WHEEL_OFFSET);
    velocity = 0;
    schedulePaint();

    if (wheelSettleTimer !== null) window.clearTimeout(wheelSettleTimer);
    wheelSettleTimer = window.setTimeout(() => {
      wheelSettleTimer = null;
      startReturnSpring();
    }, 95);
  }, { passive: false });

  return { reset };
}
