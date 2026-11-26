const MAX_TOUCH_OFFSET = 128;
const MAX_WHEEL_OFFSET = 76;
const EDGE_EPS = 2.5;
const SPRING_OMEGA = 15.5;
const SPRING_ZETA = 0.97;

function rubberBand(pull: number, maxOffset: number, resistance = 1.32): number {
  const abs = Math.abs(pull);
  if (abs < 0.01) return 0;
  return Math.sign(pull) * (maxOffset * abs) / (maxOffset + abs * resistance);
}

function inverseRubberBand(offset: number, maxOffset: number, resistance = 1.32): number {
  const abs = Math.min(maxOffset - 0.5, Math.abs(offset));
  if (abs < 0.01) return 0;
  return Math.sign(offset) * (abs * maxOffset) / ((maxOffset - abs) * resistance);
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
    // Move banner / query-source rows and workbench content together, keeping
    // .page-header sticky and .billing-workbench untransformed so fixed
    // .billing-mobile-nav never becomes page-contained or disappears.
    const extras = Array.from(
      activePage.querySelectorAll<HTMLElement>(':scope > :not(.page-header):not(.billing-workbench)'),
    ).filter(el => !el.hidden);
    const content = activePage.querySelector<HTMLElement>(':scope > .billing-workbench > .billing-workbench-content');
    return content ? [...extras, content] : extras;
  }
  return Array.from(
    activePage.querySelectorAll<HTMLElement>(':scope > :not(.page-header)'),
  ).filter(el => !el.hidden);
}

function findScrollableChild(target: EventTarget | null, boundary: HTMLElement): HTMLElement | null {
  let node = target instanceof Element ? target as HTMLElement : null;
  while (node && node !== boundary) {
    if (node.scrollHeight > node.clientHeight + 2) {
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
  if (dy > 0) return scroller.scrollTop > EDGE_EPS;
  if (dy < 0) return scroller.scrollTop < maxScroll - EDGE_EPS;
  return false;
}

function shouldIgnoreTarget(target: EventTarget | null): boolean {
  if (document.querySelector('.custom-select.open')) {
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
  let touchLastAt = 0;
  let touchVelY = 0;
  let lastTouchEndAt = 0;
  let lastTouchEndVelY = 0;
  let directionLocked: 'x' | 'y' | null = null;
  let touchEngaged = false;
  let touchChildScroller: HTMLElement | null = null;

  // Scroll momentum tracking (inertial fling hitting top/bottom)
  let prevScrollTop = mainEl.scrollTop;
  let prevScrollAt = performance.now();
  let scrollVelPxPerSec = 0;
  let lastScrollDir = 0;
  let sameScrollDirFrames = 0;
  let scrollResetGuardUntil = 0;

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
    touchVelY = 0;
    lastTouchEndVelY = 0;
    scrollVelPxPerSec = 0;
    sameScrollDirFrames = 0;
    lastScrollDir = 0;
    scrollResetGuardUntil = performance.now() + 90;
    prevScrollTop = mainEl.scrollTop;
    prevScrollAt = performance.now();
    pull = 0;
    offset = 0;
    velocity = 0;
    clearTransforms();
  };

  const startReturnSpring = (initialVelocity?: number) => {
    stopSpring();
    pull = 0;
    if (typeof initialVelocity === 'number' && Number.isFinite(initialVelocity)) {
      velocity = initialVelocity;
    }
    if ((Math.abs(offset) < 0.25 && Math.abs(velocity) < 25) || isReducedMotion()) {
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
      if (Math.abs(offset) < 0.2 && Math.abs(velocity) < 4) {
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
    paintNow();
    springFrame = requestAnimationFrame(tick);
  };

  mainEl.addEventListener('scroll', () => {
    const now = performance.now();
    const curTop = mainEl.scrollTop;
    const dtMs = now - prevScrollAt;
    const delta = curTop - prevScrollTop;
    prevScrollTop = curTop;
    prevScrollAt = now;

    if (now < scrollResetGuardUntil || mainEl.classList.contains('is-page-sliding')) {
      scrollVelPxPerSec = 0;
      sameScrollDirFrames = 0;
      lastScrollDir = 0;
      return;
    }

    if (Math.abs(delta) < 0.1 || dtMs <= 0) return;

    const dir = delta > 0 ? 1 : -1;
    if (dir === lastScrollDir && dtMs <= 90) {
      sameScrollDirFrames += 1;
    } else {
      sameScrollDirFrames = 1;
      lastScrollDir = dir;
    }

    if (dtMs <= 90) {
      const instVel = (delta / Math.max(4, dtMs)) * 1000;
      scrollVelPxPerSec = Math.abs(scrollVelPxPerSec) > 10 && sameScrollDirFrames > 1
        ? scrollVelPxPerSec * 0.45 + instVel * 0.55
        : instVel;
    } else {
      scrollVelPxPerSec = 0;
    }

    // Trigger soft inertial impact bounce when a normal scroll fling arrives at top/bottom.
    if (
      activeTouchId === null
      && wheelSettleTimer === null
      && !touchEngaged
      && Math.abs(offset) < 0.5
      && !isReducedMotion()
    ) {
      const maxMainScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
      const recentTouchFling = now - lastTouchEndAt < 380;
      const qualifiedMomentum = sameScrollDirFrames >= 2 || recentTouchFling;
      if (!qualifiedMomentum) return;

      const rawImpactVel = recentTouchFling
        ? Math.max(Math.abs(scrollVelPxPerSec), Math.abs(lastTouchEndVelY))
        : Math.abs(scrollVelPxPerSec);

      if (dir < 0 && curTop <= EDGE_EPS && rawImpactVel >= 110) {
        lastTouchEndVelY = 0;
        offset = 0.4;
        const launchVel = Math.min(920, rawImpactVel * 0.42);
        startReturnSpring(launchVel);
      } else if (dir > 0 && maxMainScroll > EDGE_EPS && curTop >= maxMainScroll - EDGE_EPS && rawImpactVel >= 110) {
        lastTouchEndVelY = 0;
        offset = -0.4;
        const launchVel = -Math.min(920, rawImpactVel * 0.42);
        startReturnSpring(launchVel);
      }
    }
  }, { passive: true });

  mainEl.addEventListener('touchstart', event => {
    if (activeTouchId !== null || event.touches.length !== 1) return;
    if (mainEl.classList.contains('is-page-sliding')) return;
    if (shouldIgnoreTarget(event.target)) return;
    const touch = event.touches[0];
    activeTouchId = touch.identifier;
    touchStartX = touch.clientX;
    touchStartY = touch.clientY;
    touchLastY = touch.clientY;
    touchLastAt = performance.now();
    touchVelY = 0;
    lastTouchEndVelY = 0;
    directionLocked = null;
    touchEngaged = false;
    touchChildScroller = findScrollableChild(event.target, mainEl);
    stopSpring();
    if (Math.abs(offset) > 0.25) {
      // Catch mid-bounce release smoothly.
      pull = Math.max(-420, Math.min(420, inverseRubberBand(offset, MAX_TOUCH_OFFSET)));
      velocity = 0;
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
      directionLocked = Math.abs(totalDy) >= Math.abs(totalDx) ? 'y' : 'x';
    }
    if (directionLocked !== 'y') {
      activeTouchId = null;
      return;
    }

    const now = performance.now();
    const dy = touch.clientY - touchLastY;
    const dtMs = Math.max(2, now - touchLastAt);
    touchLastY = touch.clientY;
    touchLastAt = now;
    if (!dy) return;

    const instVelY = (dy / dtMs) * 1000;
    touchVelY = Math.abs(touchVelY) > 1 ? touchVelY * 0.45 + instVelY * 0.55 : instVelY;

    if (!touchEngaged && touchChildScroller && canConsumeVerticalScroll(touchChildScroller, dy)) {
      return;
    }

    const maxMainScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
    let activeDy = dy;

    if (!touchEngaged && Math.abs(offset) <= 0.1) {
      if (dy > 0 && mainEl.scrollTop > EDGE_EPS && dy > mainEl.scrollTop) {
        activeDy = dy - mainEl.scrollTop;
        mainEl.scrollTop = 0;
      } else if (dy < 0 && mainEl.scrollTop < maxMainScroll - EDGE_EPS && -dy > maxMainScroll - mainEl.scrollTop) {
        activeDy = dy + (maxMainScroll - mainEl.scrollTop);
        mainEl.scrollTop = maxMainScroll;
      }
    }

    const atTop = mainEl.scrollTop <= EDGE_EPS;
    const atBottom = maxMainScroll <= EDGE_EPS || mainEl.scrollTop >= maxMainScroll - EDGE_EPS;
    const pullingPastEdge = (atTop && activeDy > 0) || (atBottom && activeDy < 0);

    if (!touchEngaged && !pullingPastEdge && Math.abs(offset) <= 0.1) {
      return;
    }

    if (isReducedMotion()) return;

    const nextPull = pull + activeDy;
    if ((pull > 0 && nextPull <= 0) || (pull < 0 && nextPull >= 0)) {
      // Returned cleanly to scroll boundary -> hand control back to vertical scroll.
      pull = 0;
      offset = 0;
      velocity = 0;
      touchEngaged = false;
      paintNow();
      return;
    }

    touchEngaged = true;
    if (event.cancelable) event.preventDefault();
    pull = Math.max(-420, Math.min(420, nextPull));
    offset = rubberBand(pull, MAX_TOUCH_OFFSET);
    velocity = 0;
    schedulePaint();
  }, { passive: false });

  const endTouch = (event: TouchEvent) => {
    if (activeTouchId === null) return;
    for (let i = 0; i < event.changedTouches.length; i++) {
      if (event.changedTouches[i].identifier === activeTouchId) {
        const recent = performance.now() - touchLastAt < 85;
        const releaseVel = recent ? touchVelY : 0;
        activeTouchId = null;
        directionLocked = null;
        const wasEngaged = touchEngaged;
        touchEngaged = false;
        lastTouchEndAt = performance.now();
        lastTouchEndVelY = releaseVel;
        if (wasEngaged || Math.abs(offset) >= 0.25) {
          startReturnSpring(Math.max(-420, Math.min(420, releaseVel * 0.22)));
        }
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
    let dy = -deltaPx;
    if (!dy) return;

    const childScroller = findScrollableChild(event.target, mainEl);
    if (Math.abs(offset) <= 0.2 && childScroller && canConsumeVerticalScroll(childScroller, dy)) {
      return;
    }

    const maxMainScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
    if (Math.abs(offset) <= 0.2) {
      if (dy > 0 && mainEl.scrollTop > EDGE_EPS && dy > mainEl.scrollTop) {
        dy = dy - mainEl.scrollTop;
        mainEl.scrollTop = 0;
      } else if (dy < 0 && mainEl.scrollTop < maxMainScroll - EDGE_EPS && -dy > maxMainScroll - mainEl.scrollTop) {
        dy = dy + (maxMainScroll - mainEl.scrollTop);
        mainEl.scrollTop = maxMainScroll;
      }
    }

    const atTop = mainEl.scrollTop <= EDGE_EPS;
    const atBottom = maxMainScroll <= EDGE_EPS || mainEl.scrollTop >= maxMainScroll - EDGE_EPS;
    const pushingPastEdge = (atTop && dy > 0) || (atBottom && dy < 0);

    if (!pushingPastEdge && Math.abs(offset) <= 0.2) return;
    if (isReducedMotion()) return;

    const nextPull = pull + dy * 0.56;
    if ((pull > 0 && nextPull <= 0) || (pull < 0 && nextPull >= 0)) {
      pull = 0;
      offset = 0;
      velocity = 0;
      if (wheelSettleTimer !== null) {
        window.clearTimeout(wheelSettleTimer);
        wheelSettleTimer = null;
      }
      paintNow();
      return;
    }

    if (event.cancelable) event.preventDefault();
    stopSpring();
    pull = Math.max(-280, Math.min(280, nextPull));
    offset = rubberBand(pull, MAX_WHEEL_OFFSET, 1.42);
    velocity = 0;
    schedulePaint();

    if (wheelSettleTimer !== null) window.clearTimeout(wheelSettleTimer);
    wheelSettleTimer = window.setTimeout(() => {
      wheelSettleTimer = null;
      startReturnSpring();
    }, 110);
  }, { passive: false });

  return { reset };
}
