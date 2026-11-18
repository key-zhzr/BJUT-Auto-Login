import { AnimatedVisibility } from './animated-visibility';

const visibility = new WeakMap<HTMLElement, AnimatedVisibility>();
const resizing = new WeakMap<HTMLElement, { animation: Animation; cleanup: () => void }>();
const disclosures = new WeakMap<HTMLDetailsElement, { desired: boolean; ignore: boolean | null; height: number }>();
const reduced = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

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
  const anchor = createUpperHalfScrollAnchor(details, false);
  const state = disclosureState(details);
  const before = previousHeight ?? details.offsetHeight;
  state.desired = expanded;
  stop(details);
  const nativeOpen = (value: boolean) => { state.ignore = value; details.open = value; };
  nativeOpen(false);
  const collapsed = details.offsetHeight;
  nativeOpen(true);
  const opened = details.offsetHeight;
  heightTransition(details, before, expanded ? opened : collapsed, () => {
    nativeOpen(state.desired); state.height = details.offsetHeight;
  }, anchor);
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
      if (state.desired === details.open && !resizing.has(details)) state.height = details.getBoundingClientRect().height;
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
