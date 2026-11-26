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

function readSafeAreaPx(name: string): number {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name);
  const parsed = Number.parseFloat(raw);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

export function setupOverscrollBounce(): { reset: () => void } {
  if (!document.body.classList.contains('is-android')) {
    return { reset: () => {} };
  }
  const mainEl = document.getElementById('main-content');
  if (!mainEl) return { reset: () => {} };

  let pull = 0;
  let offset = 0;
  let velocity = 0;
  let springFrame: number | null = null;
  let lastSpringTime = 0;
  let paintFrame: number | null = null;
  let manualMomentumFrame: number | null = null;
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
  let touchPreventedNativeScroll = false;
  let touchManualScrolled = false;
  let touchChildScroller: HTMLElement | null = null;

  // Scroll momentum tracking (inertial fling hitting top/bottom)
  let prevScrollTop = mainEl.scrollTop;
  let prevScrollAt = performance.now();
  let scrollVelPxPerSec = 0;
  let lastScrollDir = 0;
  let sameScrollDirFrames = 0;
  let sameScrollDirTravelPx = 0;
  let scrollResetGuardUntil = 0;

  // Android GPU-composited edge scrollbar indicator (shrinks during overscroll)
  const indicator = document.createElement('div');
  indicator.className = 'android-scroll-indicator';
  indicator.setAttribute('aria-hidden', 'true');
  const thumb = document.createElement('div');
  thumb.className = 'android-scroll-thumb';
  indicator.append(thumb);
  document.body.append(indicator);

  let indicatorHideTimer: number | null = null;
  let indicatorVisible = false;
  let cachedTrackTop = 0;
  let cachedTrackHeight = 0;
  let lastTrackTopStr = '';
  let lastTrackHeightStr = '';
  let lastThumbHeightStr = '';
  let lastThumbTransform = '';
  let lastThumbScale = '';

  const measureIndicatorTrack = () => {
    const rect = mainEl.getBoundingClientRect();
    const mainPadTop = Number.parseFloat(getComputedStyle(mainEl).paddingTop) || 0;
    const fallbackSafeTop = Math.max(0, mainPadTop - (window.innerWidth > 768 ? 22 : 16));
    const safeTop = Math.max(readSafeAreaPx('--android-inset-top'), fallbackSafeTop);
    const safeBottom = Math.max(readSafeAreaPx('--android-inset-bottom'), 0);
    const trackTop = rect.top + Math.max(6, safeTop + 6);
    const trackBottom = rect.bottom - Math.max(10, safeBottom + 8);
    cachedTrackTop = trackTop;
    cachedTrackHeight = Math.max(40, trackBottom - trackTop);
  };
  measureIndicatorTrack();

  const hideIndicatorNow = () => {
    if (indicatorHideTimer !== null) {
      window.clearTimeout(indicatorHideTimer);
      indicatorHideTimer = null;
    }
    if (indicatorVisible) {
      indicatorVisible = false;
      indicator.classList.remove('is-visible');
    }
  };

  const updateScrollIndicator = (activate = false) => {
    const clientHeight = mainEl.clientHeight;
    const scrollHeight = mainEl.scrollHeight;
    const maxScroll = Math.max(0, scrollHeight - clientHeight);
    if (maxScroll <= EDGE_EPS || clientHeight <= 0 || mainEl.classList.contains('is-page-sliding')) {
      hideIndicatorNow();
      return;
    }
    if (cachedTrackHeight <= 0) measureIndicatorTrack();
    const trackH = cachedTrackHeight;
    if (trackH <= 40) {
      hideIndicatorNow();
      return;
    }

    const trackTopStr = `${cachedTrackTop.toFixed(1)}px`;
    if (trackTopStr !== lastTrackTopStr) {
      lastTrackTopStr = trackTopStr;
      indicator.style.top = trackTopStr;
    }
    const trackHStr = `${trackH.toFixed(1)}px`;
    if (trackHStr !== lastTrackHeightStr) {
      lastTrackHeightStr = trackHStr;
      indicator.style.height = trackHStr;
    }

    const baseThumbH = Math.max(28, Math.min(trackH * 0.82, (clientHeight / scrollHeight) * trackH));
    const thumbHStr = `${baseThumbH.toFixed(1)}px`;
    if (thumbHStr !== lastThumbHeightStr) {
      lastThumbHeightStr = thumbHStr;
      thumb.style.height = thumbHStr;
    }

    const travel = Math.max(0, trackH - baseThumbH);
    const absOffset = Math.abs(offset);
    let scaleY = 1;
    let thumbY: number;
    if (absOffset > 0.1) {
      const compress = 1 / (1 + (absOffset / Math.max(96, clientHeight * 0.32)) * 1.75);
      scaleY = Math.max(Math.min(1, 14 / baseThumbH), Math.min(1, compress));
      thumbY = offset > 0 ? 0 : Math.max(0, trackH - baseThumbH * scaleY);
    } else {
      const progress = Math.max(0, Math.min(1, mainEl.scrollTop / maxScroll));
      thumbY = progress * travel;
    }

    const nextTransform = `translate3d(0, ${thumbY.toFixed(2)}px, 0) scaleY(${scaleY.toFixed(4)})`;
    if (nextTransform !== lastThumbTransform) {
      lastThumbTransform = nextTransform;
      thumb.style.transform = nextTransform;
    }
    const nextScaleAttr = scaleY.toFixed(3);
    if (nextScaleAttr !== lastThumbScale) {
      lastThumbScale = nextScaleAttr;
      thumb.dataset.thumbScale = nextScaleAttr;
    }

    const shouldShow = activate || absOffset > 0.1;
    if (shouldShow) {
      if (!indicatorVisible) {
        indicatorVisible = true;
        indicator.classList.add('is-visible');
      }
      if (indicatorHideTimer !== null) {
        window.clearTimeout(indicatorHideTimer);
        indicatorHideTimer = null;
      }
      if (activeTouchId === null && absOffset <= 0.1 && manualMomentumFrame === null) {
        indicatorHideTimer = window.setTimeout(() => {
          indicatorHideTimer = null;
          if (activeTouchId === null && Math.abs(offset) <= 0.1 && manualMomentumFrame === null) {
            indicatorVisible = false;
            indicator.classList.remove('is-visible');
          }
        }, 750);
      }
    }
  };

  window.addEventListener('resize', () => {
    measureIndicatorTrack();
    updateScrollIndicator(false);
  }, { passive: true });
  window.addEventListener('bjut-safe-area-change', () => {
    measureIndicatorTrack();
    updateScrollIndicator(false);
  });

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
      updateScrollIndicator(false);
      return;
    }
    const transform = `translate3d(0, ${offset.toFixed(2)}px, 0)`;
    for (const el of targets) {
      el.style.willChange = 'transform';
      el.style.transform = transform;
    }
    paintedTargets = targets;
    updateScrollIndicator(true);
  };

  const schedulePaint = () => {
    if (paintFrame === null) paintFrame = requestAnimationFrame(paintNow);
  };

  const stopManualMomentum = () => {
    if (manualMomentumFrame !== null) {
      cancelAnimationFrame(manualMomentumFrame);
      manualMomentumFrame = null;
    }
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
    stopManualMomentum();
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
    touchPreventedNativeScroll = false;
    touchManualScrolled = false;
    touchVelY = 0;
    lastTouchEndVelY = 0;
    scrollVelPxPerSec = 0;
    sameScrollDirFrames = 0;
    sameScrollDirTravelPx = 0;
    lastScrollDir = 0;
    scrollResetGuardUntil = performance.now() + 90;
    prevScrollTop = mainEl.scrollTop;
    prevScrollAt = performance.now();
    pull = 0;
    offset = 0;
    velocity = 0;
    clearTransforms();
    hideIndicatorNow();
    measureIndicatorTrack();
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

  const startManualMomentum = (initialScrollVel: number) => {
    stopManualMomentum();
    let scrollVel = initialScrollVel; // positive = scrollTop increasing (moving down page)
    let lastTime = 0;
    const step = (now: number) => {
      if (mainEl.classList.contains('is-page-sliding')) {
        manualMomentumFrame = null;
        return;
      }
      const dt = lastTime ? Math.min((now - lastTime) / 1000, 0.04) : 1 / 60;
      lastTime = now;
      const maxMainScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
      const nextTop = mainEl.scrollTop + scrollVel * dt;
      scrollVel *= Math.exp(-5.0 * dt);
      scrollResetGuardUntil = now + 90;

      if (nextTop <= 0 && scrollVel < 0) {
        mainEl.scrollTop = 0;
        prevScrollTop = 0;
        prevScrollAt = now;
        manualMomentumFrame = null;
        if (Math.abs(scrollVel) >= 110 && !isReducedMotion()) {
          offset = 0.4;
          startReturnSpring(Math.min(720, Math.abs(scrollVel) * 0.36));
        } else {
          updateScrollIndicator(true);
        }
        return;
      }
      if (nextTop >= maxMainScroll && scrollVel > 0) {
        mainEl.scrollTop = maxMainScroll;
        prevScrollTop = maxMainScroll;
        prevScrollAt = now;
        manualMomentumFrame = null;
        if (maxMainScroll > EDGE_EPS && Math.abs(scrollVel) >= 110 && !isReducedMotion()) {
          offset = -0.4;
          startReturnSpring(-Math.min(720, Math.abs(scrollVel) * 0.36));
        } else {
          updateScrollIndicator(true);
        }
        return;
      }

      mainEl.scrollTop = Math.max(0, Math.min(maxMainScroll, nextTop));
      prevScrollTop = mainEl.scrollTop;
      prevScrollAt = now;
      updateScrollIndicator(true);

      if (Math.abs(scrollVel) < 40) {
        manualMomentumFrame = null;
        return;
      }
      manualMomentumFrame = requestAnimationFrame(step);
    };
    manualMomentumFrame = requestAnimationFrame(step);
  };

  mainEl.addEventListener('scroll', () => {
    const now = performance.now();
    const curTop = mainEl.scrollTop;
    const dtMs = now - prevScrollAt;
    const delta = curTop - prevScrollTop;
    prevScrollTop = curTop;
    prevScrollAt = now;

    updateScrollIndicator(true);

    if (now < scrollResetGuardUntil || mainEl.classList.contains('is-page-sliding')) {
      scrollVelPxPerSec = 0;
      sameScrollDirFrames = 0;
      sameScrollDirTravelPx = 0;
      lastScrollDir = 0;
      return;
    }

    if (Math.abs(delta) < 0.1 || dtMs <= 0) return;

    const dir = delta > 0 ? 1 : -1;
    if (dir === lastScrollDir && dtMs <= 90) {
      sameScrollDirFrames += 1;
      sameScrollDirTravelPx += Math.abs(delta);
    } else {
      sameScrollDirFrames = 1;
      sameScrollDirTravelPx = Math.abs(delta);
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

    // Trigger soft inertial impact bounce only when travel/fling direction matches the edge.
    if (
      activeTouchId === null
      && wheelSettleTimer === null
      && manualMomentumFrame === null
      && !touchEngaged
      && Math.abs(offset) < 0.5
      && !isReducedMotion()
    ) {
      const maxMainScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
      const recentTouchFling = now - lastTouchEndAt < 380;
      // finger dragging down (touchVelY > 0) scrolls up (dir < 0); dragging up (touchVelY < 0) scrolls down (dir > 0)
      const matchingTouchFling = recentTouchFling
        && ((dir < 0 && lastTouchEndVelY > 60) || (dir > 0 && lastTouchEndVelY < -60));
      const qualifiedMomentum = sameScrollDirTravelPx >= 10 && (sameScrollDirFrames >= 2 || matchingTouchFling);
      if (!qualifiedMomentum) return;

      const rawImpactVel = matchingTouchFling
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
    touchPreventedNativeScroll = false;
    touchManualScrolled = false;
    touchChildScroller = findScrollableChild(event.target, mainEl);
    stopSpring();
    stopManualMomentum();
    if (Math.abs(offset) > 0.05) {
      // Catch mid-bounce release smoothly without resetting pull to 0.
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

    if (!touchEngaged && !touchPreventedNativeScroll && touchChildScroller && canConsumeVerticalScroll(touchChildScroller, dy)) {
      return;
    }

    const maxMainScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
    let activeDy = dy;

    if (!touchEngaged && Math.abs(offset) <= 0.1) {
      if (touchPreventedNativeScroll) {
        // Native WebView scrolling is locked out for this pointer after an earlier
        // preventDefault(); continue scrolling mainEl.scrollTop manually instead.
        const nextTop = mainEl.scrollTop - dy;
        if (nextTop >= 0 && nextTop <= maxMainScroll) {
          if (event.cancelable) event.preventDefault();
          touchManualScrolled = true;
          scrollResetGuardUntil = now + 90;
          mainEl.scrollTop = nextTop;
          prevScrollTop = mainEl.scrollTop;
          prevScrollAt = now;
          updateScrollIndicator(true);
          return;
        }
        if (event.cancelable) event.preventDefault();
        scrollResetGuardUntil = now + 90;
        if (nextTop < 0) {
          mainEl.scrollTop = 0;
          prevScrollTop = 0;
          activeDy = -nextTop;
        } else {
          mainEl.scrollTop = maxMainScroll;
          prevScrollTop = maxMainScroll;
          activeDy = maxMainScroll - nextTop;
        }
        prevScrollAt = now;
      } else {
        if (dy > 0 && mainEl.scrollTop > EDGE_EPS && dy > mainEl.scrollTop) {
          activeDy = dy - mainEl.scrollTop;
          mainEl.scrollTop = 0;
        } else if (dy < 0 && mainEl.scrollTop < maxMainScroll - EDGE_EPS && -dy > maxMainScroll - mainEl.scrollTop) {
          activeDy = dy + (maxMainScroll - mainEl.scrollTop);
          mainEl.scrollTop = maxMainScroll;
        }
      }
    } else if (Math.abs(offset) > 0.05 && (pull === 0 || Math.sign(pull) !== Math.sign(offset))) {
      pull = Math.max(-420, Math.min(420, inverseRubberBand(offset, MAX_TOUCH_OFFSET)));
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
      const leftoverDy = nextPull;
      pull = 0;
      offset = 0;
      velocity = 0;
      touchEngaged = false;
      if (maxMainScroll > EDGE_EPS) {
        if (event.cancelable) {
          event.preventDefault();
          touchPreventedNativeScroll = true;
        }
        touchManualScrolled = true;
        scrollResetGuardUntil = now + 90;
        mainEl.scrollTop = Math.max(0, Math.min(maxMainScroll, mainEl.scrollTop - leftoverDy));
        prevScrollTop = mainEl.scrollTop;
        prevScrollAt = now;
      }
      paintNow();
      return;
    }

    touchEngaged = true;
    if (event.cancelable) {
      event.preventDefault();
      touchPreventedNativeScroll = true;
    }
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
        const wasManualScroll = touchManualScrolled;
        touchEngaged = false;
        touchPreventedNativeScroll = false;
        touchManualScrolled = false;
        lastTouchEndAt = performance.now();
        lastTouchEndVelY = releaseVel;
        if (wasEngaged || Math.abs(offset) >= 0.25) {
          startReturnSpring(Math.max(-420, Math.min(420, releaseVel * 0.22)));
        } else if (wasManualScroll && Math.abs(releaseVel) > 65 && !isReducedMotion()) {
          startManualMomentum(-releaseVel);
        } else {
          updateScrollIndicator(false);
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
    stopManualMomentum();

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
    } else if (pull === 0 || Math.sign(pull) !== Math.sign(offset)) {
      // Reconstruct active pull from current offset so return-spring reset never zeroes pull mid-bounce.
      pull = Math.max(-280, Math.min(280, inverseRubberBand(offset, MAX_WHEEL_OFFSET, 1.42)));
    }

    const atTop = mainEl.scrollTop <= EDGE_EPS;
    const atBottom = maxMainScroll <= EDGE_EPS || mainEl.scrollTop >= maxMainScroll - EDGE_EPS;
    const pushingPastEdge = (atTop && dy > 0) || (atBottom && dy < 0);

    if (!pushingPastEdge && Math.abs(offset) <= 0.2) return;
    if (isReducedMotion()) return;

    const nextPull = pull + dy * 0.56;
    if ((pull > 0 && nextPull <= 0) || (pull < 0 && nextPull >= 0)) {
      const leftoverScrollPx = -nextPull / 0.56;
      pull = 0;
      offset = 0;
      velocity = 0;
      stopSpring();
      if (wheelSettleTimer !== null) {
        window.clearTimeout(wheelSettleTimer);
        wheelSettleTimer = null;
      }
      if (maxMainScroll > EDGE_EPS) {
        if (event.cancelable) event.preventDefault();
        scrollResetGuardUntil = performance.now() + 90;
        mainEl.scrollTop = Math.max(0, Math.min(maxMainScroll, mainEl.scrollTop + leftoverScrollPx));
        prevScrollTop = mainEl.scrollTop;
        prevScrollAt = performance.now();
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
