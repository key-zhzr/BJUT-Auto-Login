import { AnimatedVisibility } from './animated-visibility';

const visibility = new WeakMap<HTMLElement, AnimatedVisibility>();
const resizing = new WeakMap<HTMLElement, { animation: Animation; cleanup: () => void }>();
const disclosures = new WeakMap<HTMLDetailsElement, { desired: boolean; ignore: boolean | null; height: number }>();
const reduced = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

export function setVisible(element: HTMLElement, visible: boolean) {
  let controller = visibility.get(element);
  if (!controller) { controller = new AnimatedVisibility(element); visibility.set(element, controller); }
  controller.setVisible(visible);
}

function stop(element: HTMLElement) {
  const previous = resizing.get(element);
  if (!previous) return;
  previous.animation.onfinish = null;
  previous.animation.cancel();
  previous.cleanup();
  resizing.delete(element);
}

function heightTransition(element: HTMLElement, from: number, to: number, finished: () => void = () => {}) {
  if (Math.abs(from - to) < 1 || from < 0 || to < 0 || reduced() || !element.animate) { finished(); return; }
  const overflow = element.style.overflow, sizing = element.style.boxSizing;
  element.style.overflow = 'clip'; element.style.boxSizing = 'border-box';
  const cleanup = () => { element.style.overflow = overflow; element.style.boxSizing = sizing; };
  const animation = element.animate([{ height: `${from}px` }, { height: `${to}px` }], {
    duration: to > from ? 220 : 180, easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)', fill: 'both',
  });
  resizing.set(element, { animation, cleanup });
  animation.onfinish = () => {
    if (resizing.get(element)?.animation !== animation) return;
    finished(); animation.cancel(); cleanup(); resizing.delete(element);
  };
}

/** Preserve the interpolated height when a live list changes during a transition. */
export function animateResize(element: HTMLElement, update: () => void) {
  const before = element.getBoundingClientRect().height;
  stop(element); update();
  heightTransition(element, before, element.getBoundingClientRect().height);
}

function disclosureState(details: HTMLDetailsElement) {
  let state = disclosures.get(details);
  if (!state) {
    state = { desired: details.open, ignore: null, height: details.getBoundingClientRect().height };
    disclosures.set(details, state);
  }
  return state;
}

export function setExpanded(details: HTMLDetailsElement, expanded: boolean, previousHeight?: number) {
  const state = disclosureState(details);
  const before = previousHeight ?? details.getBoundingClientRect().height;
  state.desired = expanded;
  stop(details);
  const nativeOpen = (value: boolean) => { state.ignore = value; details.open = value; };
  nativeOpen(false);
  const collapsed = details.getBoundingClientRect().height;
  nativeOpen(true);
  const opened = details.getBoundingClientRect().height;
  heightTransition(details, before, expanded ? opened : collapsed, () => {
    nativeOpen(state.desired); state.height = details.getBoundingClientRect().height;
  });
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
