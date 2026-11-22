import { AnimatedVisibility } from './animated-visibility';

const visibility = new WeakMap<HTMLElement, AnimatedVisibility>();
const resizing = new WeakMap<HTMLElement, { animation: Animation; cleanup: () => void }>();
const disclosureAnims = new WeakMap<
  HTMLDetailsElement,
  {
    animation: Animation;
    childAnims: Animation[];
    siblingAnims: Animation[];
    fromHeight: number;
    toHeight: number;
    cleanup: () => void;
  }
>();
const disclosures = new WeakMap<HTMLDetailsElement, { desired: boolean; ignore: boolean | null; height: number }>();
const reduced = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

export function collectFlowSiblings(element: HTMLElement, direction: 'next' | 'prev'): HTMLElement[] {
  const stopRoot = element.closest<HTMLElement>('.page, .modal-content') || element.parentElement;
  const collected: HTMLElement[] = [];
  const seen = new Set<HTMLElement>();
  let current: HTMLElement | null = element;
  while (current && current !== stopRoot) {
    let sibling = direction === 'next' ? current.nextElementSibling : current.previousElementSibling;
    while (sibling) {
      if (sibling instanceof HTMLElement && !sibling.hidden && !seen.has(sibling)) {
        const style = getComputedStyle(sibling);
        if (
          style.display !== 'none'
          && style.position !== 'fixed'
          && style.position !== 'absolute'
          && style.position !== 'sticky'
        ) {
          // Never transform a wrapper that contains a viewport-fixed mobile nav bar.
          const fixedDescendant = sibling.querySelector<HTMLElement>('.billing-mobile-nav, #nav');
          if (fixedDescendant) {
            for (const child of Array.from(sibling.children)) {
              if (
                child instanceof HTMLElement
                && !child.hidden
                && child !== fixedDescendant
                && !child.contains(fixedDescendant)
                && !seen.has(child)
              ) {
                const cs = getComputedStyle(child);
                if (cs.display !== 'none' && cs.position !== 'fixed' && cs.position !== 'absolute' && cs.position !== 'sticky') {
                  seen.add(child);
                  collected.push(child);
                }
              }
            }
          } else {
            seen.add(sibling);
            collected.push(sibling);
          }
        }
      }
      sibling = direction === 'next' ? sibling.nextElementSibling : sibling.previousElementSibling;
    }
    current = current.parentElement;
  }
  return collected;
}

export function setVisible(element: HTMLElement, visible: boolean, options?: { autoAnchor?: boolean }) {
  let controller = visibility.get(element);
  if (!controller) { controller = new AnimatedVisibility(element); visibility.set(element, controller); }
  controller.setVisible(visible, options);
}

function stop(element: HTMLElement) {
  const previous = resizing.get(element);
  if (!previous) return;
  previous.animation.onfinish = null;
  previous.animation.cancel();
  previous.cleanup();
  resizing.delete(element);
}

export interface ScrollAnchorHandle {
  sync: () => void;
  startLoop: () => void;
  stop: () => void;
  release?: () => void;
}

const NOOP_ANCHOR: ScrollAnchorHandle = { sync: () => {}, startLoop: () => {}, stop: () => {}, release: () => {} };

/** Anchor content below `element` when `element` sits in the upper half of `#main-content`
 * and an automatic update asks to squeeze content above upward instead of jerking the page. */
export function createUpperHalfScrollAnchor(element: HTMLElement, enabled = false): ScrollAnchorHandle {
  if (!enabled) return NOOP_ANCHOR;
  const scroller = element.closest<HTMLElement>('#main-content');
  const page = element.closest<HTMLElement>('.page.active');
  if (!scroller || !page) return NOOP_ANCHOR;

  let anchor: HTMLElement | null = null;
  let current: Element | null = element;
  while (current && current !== page && !anchor) {
    let sibling = current.nextElementSibling;
    while (sibling) {
      if (sibling instanceof HTMLElement && !sibling.hidden && sibling.getBoundingClientRect().height > 0) {
        anchor = sibling;
        break;
      }
      sibling = sibling.nextElementSibling;
    }
    current = current.parentElement;
  }
  if (!anchor) return NOOP_ANCHOR;

  const scrollerRect = scroller.getBoundingClientRect();
  const anchorRectBefore = anchor.getBoundingClientRect();
  const elementRectBefore = element.getBoundingClientRect();
  const referenceY = !element.hidden && elementRectBefore.height > 0
    ? elementRectBefore.top + elementRectBefore.height * 0.5
    : anchorRectBefore.top;
  if (referenceY >= scrollerRect.top + scrollerRect.height * 0.5) return NOOP_ANCHOR;

  const previousOverflowAnchor = scroller.style.overflowAnchor;
  scroller.style.overflowAnchor = 'none';
  let targetTop = anchorRectBefore.top;
  let expectedScrollTop = scroller.scrollTop;
  let previousMaxScrollTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
  let rafId: number | null = null;
  let released = false;

  const sync = () => {
    if (released || !anchor?.isConnected || !page.classList.contains('active')) return false;
    const currentScrollTop = scroller.scrollTop;
    const currentMaxScrollTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    const wasClampedAtBottom = Math.abs(expectedScrollTop - previousMaxScrollTop) <= 1.5
      && Math.abs(currentScrollTop - currentMaxScrollTop) <= 1.5;
    if (!wasClampedAtBottom && Math.abs(currentScrollTop - expectedScrollTop) > 1.5) {
      targetTop -= currentScrollTop - expectedScrollTop;
      expectedScrollTop = currentScrollTop;
    }
    const currentTop = anchor.getBoundingClientRect().top;
    const diff = currentTop - targetTop;
    if (Math.abs(diff) >= 0.5) {
      scroller.scrollTop = Math.max(0, currentScrollTop + diff);
    }
    expectedScrollTop = scroller.scrollTop;
    previousMaxScrollTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    return true;
  };

  const loop = () => {
    rafId = sync() ? requestAnimationFrame(loop) : null;
  };

  return {
    sync,
    startLoop: () => {
      if (rafId === null && !released) rafId = requestAnimationFrame(loop);
    },
    stop: () => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
    },
    release: () => {
      if (released) return;
      released = true;
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      scroller.style.overflowAnchor = previousOverflowAnchor;
    },
  };
}

function heightTransition(
  element: HTMLElement,
  from: number,
  to: number,
  finished: () => void = () => {},
  anchor: ScrollAnchorHandle = createUpperHalfScrollAnchor(element, false),
) {
  if (Math.abs(from - to) < 1 || from <= 0 || to <= 0 || reduced() || !element.animate) {
    finished();
    anchor.sync();
    anchor.release?.();
    return;
  }
  const overflow = element.style.overflow, sizing = element.style.boxSizing;
  element.style.overflow = 'clip'; element.style.boxSizing = 'border-box';
  const cleanup = () => {
    element.style.overflow = overflow;
    element.style.boxSizing = sizing;
    anchor.stop();
    anchor.release?.();
  };
  const animation = element.animate([{ height: `${from}px` }, { height: `${to}px` }], {
    duration: to > from ? 220 : 180, easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)', fill: 'both',
  });
  anchor.sync();
  anchor.startLoop();
  resizing.set(element, { animation, cleanup });
  animation.onfinish = () => {
    if (resizing.get(element)?.animation !== animation) return;
    finished(); animation.cancel(); cleanup(); resizing.delete(element);
  };
}

/** Preserve the interpolated height when a live list changes during a transition. */
export function animateResize(element: HTMLElement, update: () => void, autoAnchor = false) {
  const anchor = createUpperHalfScrollAnchor(element, autoAnchor);
  const before = element.offsetHeight;
  stop(element); update();
  heightTransition(element, before, element.offsetHeight, () => {}, anchor);
}

function disclosureState(details: HTMLDetailsElement) {
  let state = disclosures.get(details);
  if (!state) {
    state = { desired: details.open, ignore: null, height: details.offsetHeight };
    disclosures.set(details, state);
  }
  return state;
}

export function setExpanded(details: HTMLDetailsElement, expanded: boolean, previousHeight?: number) {
  const state = disclosureState(details);
  const existing = disclosureAnims.get(details);
  let currentHeight = previousHeight ?? details.offsetHeight;
  if (existing) {
    const rawProgress = existing.animation.effect?.getComputedTiming().progress;
    const progress = typeof rawProgress === 'number' && Number.isFinite(rawProgress)
      ? Math.max(0, Math.min(1, rawProgress))
      : 1;
    currentHeight = existing.fromHeight + (existing.toHeight - existing.fromHeight) * progress;
    existing.animation.onfinish = null;
    existing.cleanup();
  }
  stop(details);
  state.desired = expanded;

  const nativeOpen = (value: boolean) => { state.ignore = value; details.open = value; };
  const siblings = collectFlowSiblings(details, 'next');
  const scroller = details.closest<HTMLElement>('#main-content');
  const savedScroll = scroller?.scrollTop ?? 0;

  nativeOpen(false);
  const collapsed = details.offsetHeight;
  const beforeTops = siblings.map(s => s.getBoundingClientRect().top);

  nativeOpen(true);
  if (scroller && scroller.scrollTop !== savedScroll) scroller.scrollTop = savedScroll;
  const opened = details.offsetHeight;
  const afterTops = siblings.map(s => s.getBoundingClientRect().top);

  const delta = opened - collapsed;
  if (delta < 1 || reduced() || !details.animate) {
    nativeOpen(expanded);
    state.height = details.offsetHeight;
    return;
  }

  const startH = Math.max(collapsed, Math.min(opened, currentHeight));
  const targetH = expanded ? opened : collapsed;
  if (Math.abs(targetH - startH) < 1) {
    nativeOpen(expanded);
    state.height = details.offsetHeight;
    return;
  }

  const fromFrac = (startH - collapsed) / delta;
  const toFrac = expanded ? 1 : 0;
  const fromClip = Math.max(0, opened - startH);
  const toClip = Math.max(0, opened - targetH);
  const radius = getComputedStyle(details).borderTopLeftRadius || '0px';
  const timing: KeyframeAnimationOptions = {
    duration: expanded ? 220 : 180,
    easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)',
    fill: 'both',
  };

  const animation = details.animate(
    [
      { clipPath: `inset(0px 0px ${fromClip.toFixed(1)}px 0px round ${radius})` },
      { clipPath: `inset(0px 0px ${toClip.toFixed(1)}px 0px round ${radius})` },
    ],
    timing,
  );

  const childAnims: Animation[] = [];
  for (const child of Array.from(details.children)) {
    if (!(child instanceof HTMLElement) || child.tagName === 'SUMMARY' || !child.animate) continue;
    childAnims.push(child.animate(
      [
        { opacity: fromFrac, transform: `translate3d(0, ${((1 - fromFrac) * -6).toFixed(2)}px, 0)` },
        { opacity: toFrac, transform: `translate3d(0, ${((1 - toFrac) * -6).toFixed(2)}px, 0)` },
      ],
      timing,
    ));
  }

  const siblingAnims: Animation[] = [];
  for (let i = 0; i < siblings.length; i++) {
    const dy = afterTops[i] - beforeTops[i];
    if (Math.abs(dy) < 1 || !siblings[i].animate) continue;
    siblingAnims.push(siblings[i].animate(
      [
        { transform: `translate3d(0, ${(-(1 - fromFrac) * dy).toFixed(2)}px, 0)` },
        { transform: `translate3d(0, ${(-(1 - toFrac) * dy).toFixed(2)}px, 0)` },
      ],
      timing,
    ));
  }

  const cleanup = () => {
    animation.cancel();
    childAnims.forEach(a => a.cancel());
    siblingAnims.forEach(a => a.cancel());
    disclosureAnims.delete(details);
  };
  disclosureAnims.set(details, {
    animation,
    childAnims,
    siblingAnims,
    fromHeight: startH,
    toHeight: targetH,
    cleanup,
  });
  animation.onfinish = () => {
    if (disclosureAnims.get(details)?.animation !== animation) return;
    nativeOpen(state.desired);
    cleanup();
    state.height = details.offsetHeight;
  };
}

export function toggleExpanded(details: HTMLDetailsElement) {
  setExpanded(details, !disclosureState(details).desired);
}

/** Native details keep their markup, keyboard behavior and focus semantics. */
export function setupDisclosureMotion() {
  const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(entries => {
    for (const entry of entries) {
      const details = entry.target as HTMLDetailsElement;
      const state = disclosureState(details);
      if (state.desired === details.open && !resizing.has(details) && !disclosureAnims.has(details)) {
        state.height = details.getBoundingClientRect().height;
      }
    }
  });
  const prepare = (details: HTMLDetailsElement) => {
    disclosureState(details); observer?.observe(details);
  };
  document.querySelectorAll('details').forEach(prepare);
  document.addEventListener('click', event => {
    const target = event.target instanceof Element ? event.target : null;
    const summary = target?.closest('summary');
    const details = summary?.parentElement;
    if (!(details instanceof HTMLDetailsElement) || event.defaultPrevented || target?.closest('a,button,input,select,textarea')) return;
    event.preventDefault(); prepare(details); toggleExpanded(details);
  });
  document.addEventListener('toggle', event => {
    if (!(event.target instanceof HTMLDetailsElement)) return;
    const details = event.target, state = disclosureState(details);
    if (state.ignore === details.open) { state.ignore = null; return; }
    if (state.desired !== details.open) setExpanded(details, details.open, state.height);
  }, true);
  new MutationObserver(records => {
    for (const record of records) {
      for(const node of record.removedNodes) {
        if (!(node instanceof Element) || node.isConnected) continue;
        if (node instanceof HTMLDetailsElement) observer?.unobserve(node);
        node.querySelectorAll("details").forEach(details=>observer?.unobserve(details));
      }
      for (const node of record.addedNodes) {
      if (!(node instanceof Element)) continue;
      if (node instanceof HTMLDetailsElement) prepare(node);
      node.querySelectorAll('details').forEach(prepare);
      }
    }
  }).observe(document.body, { childList: true, subtree: true });
}
