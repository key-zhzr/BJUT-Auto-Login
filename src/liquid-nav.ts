import { updateDockGlass } from './liquid-glass';
import {
  flushMacosLiquidGlass,
  getMacosNativeGlassEngine,
  initMacosLiquidGlass,
  isMacosNativeGlassActive,
  setMacosGlassSlot,
} from './macos-liquid-glass';

const mobileWidth = window.matchMedia('(max-width: 768px)');
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
const reducedTransparency = window.matchMedia('(prefers-reduced-transparency: reduce)');
const LENS_WIDTH_GAIN = 28;
const LENS_HEIGHT_GAIN = 22;
const OVERSCROLL_LIMIT_X = 14;
const PULL_LIMIT_X = 6;
const PULL_LIMIT_Y = 3.2;
const EDGE_CLEARANCE = 2;
const DEFORM_OMEGA = 22;
const DEFORM_ZETA = 0.45;
const DEFORM_LIMIT = 1;
const LIFT_OPEN_OMEGA = 19;
const LIFT_OPEN_ZETA = 0.68;
const LIFT_CLOSE_OMEGA = 15;
const LIFT_CLOSE_ZETA = 0.92;
const BAR_OPEN_OMEGA = 20;
const BAR_OPEN_ZETA = 0.62;
const BAR_CLOSE_OMEGA = 20;
const BAR_CLOSE_ZETA = 0.42;
const PULL_RETURN_OMEGA = 19;
const PULL_RETURN_ZETA = 0.54;
const ITEM_MAG_GAIN = 0.11;
const BAR_PRESS_SCALE_MAX = 0.032;

interface TabLayoutBox {
  element: HTMLElement;
  mirror: HTMLElement;
  left: number;
  top: number;
  width: number;
  height: number;
  inkHalfW: number;
  inkHalfH: number;
  lastTransform: string;
  lastMirrorTransform: string;
  lastClipPath: string;
  lastOpacity: string;
}

interface PillState {
  container: HTMLElement;
  host: HTMLElement;
  barSurface: HTMLElement;
  barGlow: HTMLElement;
  barRecess: HTMLElement;
  barRecessSurface: HTMLElement;
  barRecessGlow: HTMLElement;
  pill: HTMLElement;
  pillInner: HTMLElement;
  accentLayer: HTMLElement;
  accentSignature: string;
  accentTabs: TabLayoutBox[];
  droplet: HTMLElement;
  lastKey: string;
  pressed: boolean;
  dragging: boolean;
  glideUntil: number;
  cancelPress?: (ignoreClick?: boolean, preserveGlide?: boolean) => void;
  frame: number;
  lastFrame: number;
  x: number;
  width: number;
  targetX: number;
  targetWidth: number;
  velocity: number;
  lift: number;
  liftVel: number;
  barScale: number;
  barScaleVel: number;
  overscrollX: number;
  pullX: number;
  pullY: number;
  pullVelX: number;
  pullVelY: number;
  targetPullX: number;
  targetPullY: number;
  deform: number;
  deformVel: number;
  lastBarX: number;
  lastBarY: number;
  lastPillW: number;
  lastPillH: number;
  lastPillOpacity: string;
  lastDropletActive: boolean;
  lastBarAnimating: boolean;
  lastGlowOpacity: string;
  lastBarTransform: string;
  lastBarCutout: string;
  lastRecessClip: string;
  lastRecessTransform: string;
  lastAccentClip: string;
  lastDropletOpacity: string;
  lastDropletTransform: string;
  containerLeftInHost: number;
  containerTopInHost: number;
  top: number;
  height: number;
  dockWidth: number;
  dockHeight: number;
  leftRoom: number;
  rightEdge: number;
  bottomEdge: number;
  barBox: DOMRect | null;
  lensWidth: number;
  lensHeight: number;
}

function stepSpring(
  value: number,
  velocity: number,
  target: number,
  omega: number,
  zeta: number,
  dt: number,
): [number, number] {
  const alpha = zeta * omega;
  const delta = value - target;
  const decay = Math.exp(-alpha * dt);
  if (zeta < 0.999) {
    const omegaD = omega * Math.sqrt(1 - zeta * zeta);
    const cos = Math.cos(omegaD * dt);
    const sin = Math.sin(omegaD * dt);
    const b = (velocity + alpha * delta) / omegaD;
    const nextValue = target + (delta * cos + b * sin) * decay;
    const nextVelocity = (
      velocity * cos
      - ((alpha * velocity + omega * omega * delta) / omegaD) * sin
    ) * decay;
    return [nextValue, nextVelocity];
  }
  const k = velocity + omega * delta;
  const exp = Math.exp(-omega * dt);
  return [
    target + (delta + k * dt) * exp,
    (velocity - omega * k * dt) * exp,
  ];
}

const controllers: PillState[] = [];
let initialized = false;

function isHorizontalMobileDock() {
  return mobileWidth.matches && !document.body.classList.contains('is-desktop');
}

function getUntransformedRect(element: HTMLElement): DOMRect {
  const r = element.getBoundingClientRect();
  const tr = getComputedStyle(element).transform;
  if (!tr || tr === 'none') return r;
  try {
    const m = new DOMMatrixReadOnly(tr);
    const sx = Math.hypot(m.a, m.b) || 1;
    const sy = Math.hypot(m.c, m.d) || 1;
    const width = r.width / sx;
    const height = r.height / sy;
    const cx = r.left + r.width / 2 - m.m41;
    const cy = r.top + r.height / 2 - m.m42;
    return new DOMRect(cx - width / 2, cy - height / 2, width, height);
  } catch {
    return r;
  }
}

function getNetTransformOffset(element: HTMLElement): { dx: number; dy: number } {
  let dx = 0;
  let dy = 0;
  let cur: HTMLElement | null = element;
  while (cur && cur !== document.body) {
    const tr = getComputedStyle(cur).transform;
    if (tr && tr !== 'none') {
      try {
        const m = new DOMMatrixReadOnly(tr);
        dx += m.m41;
        dy += m.m42;
      } catch {
        // Ignore unparseable transforms.
      }
    }
    cur = cur.parentElement;
  }
  return { dx, dy };
}

function isBoxVisible(el: HTMLElement | null): el is HTMLElement {
  if (!el || !el.isConnected || el.hidden) return false;
  const style = getComputedStyle(el);
  if (style.display === 'none' || style.visibility === 'hidden') return false;
  const rect = el.getBoundingClientRect();
  return rect.width > 1 && rect.height > 1;
}

function resolvePrimaryNavActiveTarget(container: HTMLElement): {
  target: HTMLElement | null;
  key: string;
  isParentPill: boolean;
} {
  const nav = document.getElementById('nav');
  if (!nav) return { target: null, key: '', isParentPill: false };

  if (isHorizontalMobileDock()) {
    const item = container.querySelector<HTMLElement>('.nav-item.active');
    if (!isBoxVisible(item)) return { target: null, key: '', isParentPill: false };
    return {
      target: item,
      key: `h:${item.dataset.target || 'active'}`,
      isParentPill: false,
    };
  }

  const billingActive = nav.classList.contains('billing-active');
  const dashboardExpanded = nav.classList.contains('dashboard-expanded');
  const childrenEl = document.getElementById('nav-dashboard-children');
  const billingTreeEl = document.getElementById('nav-billing-tree');
  const childrenVisible = dashboardExpanded
    && childrenEl
    && getComputedStyle(childrenEl).display !== 'none';
  const billingTreeVisible = childrenVisible
    && billingActive
    && billingTreeEl
    && getComputedStyle(billingTreeEl).display !== 'none';

  if (billingTreeVisible) {
    const billingLink = billingTreeEl.querySelector<HTMLElement>('.nav-billing-link.active:not([hidden])');
    if (isBoxVisible(billingLink)) {
      return {
        target: billingLink,
        key: `v:billing:${billingLink.dataset.billingSectionTarget || ''}`,
        isParentPill: false,
      };
    }
  }

  if (childrenVisible) {
    const pageLink = childrenEl.querySelector<HTMLElement>('.nav-page-link.active');
    const parentActive = nav.querySelector<HTMLElement>('.nav-dashboard-parent.active');
    if (parentActive && isBoxVisible(pageLink)) {
      return {
        target: pageLink,
        key: `v:page:${pageLink.dataset.navPageTarget || ''}`,
        isParentPill: false,
      };
    }
  }

  const activeItem = container.querySelector<HTMLElement>('.nav-item.active');
  if (!isBoxVisible(activeItem)) return { target: null, key: '', isParentPill: false };
  const isParent = activeItem.classList.contains('nav-dashboard-parent') && dashboardExpanded;
  return {
    target: activeItem,
    key: `v:top:${activeItem.dataset.target || ''}:${childrenVisible ? 'open' : 'rail'}`,
    isParentPill: isParent,
  };
}

function resolveBillingDockActiveTarget(container: HTMLElement): {
  target: HTMLElement | null;
  key: string;
  isParentPill: boolean;
} {
  if (!isHorizontalMobileDock() || !document.body.classList.contains('billing-center-active')) {
    return { target: null, key: '', isParentPill: false };
  }
  const activeBtn = container.querySelector<HTMLElement>('.billing-section-nav-button.active:not([hidden])');
  if (!isBoxVisible(activeBtn)) return { target: null, key: '', isParentPill: false };
  return {
    target: activeBtn,
    key: `bill:${activeBtn.dataset.billingSectionTarget || ''}`,
    isParentPill: false,
  };
}

function getHorizontalTabs(container: HTMLElement) {
  return Array.from(container.querySelectorAll<HTMLElement>('.nav-item, .billing-section-nav-button'))
    .filter(tab => isBoxVisible(tab));
}

function buildStadiumHolePath(
  outerLeft: number,
  outerTop: number,
  outerRight: number,
  outerBottom: number,
  rx: number,
  ry: number,
  rw: number,
  rh: number,
  radiusX: number,
  radiusY: number,
) {
  const rX = Math.max(0.01, Math.min(rw / 2, radiusX));
  const rY = Math.max(0.01, Math.min(rh / 2, radiusY));
  const x0 = +rx.toFixed(2);
  const y0 = +ry.toFixed(2);
  const x1 = +(rx + rw).toFixed(2);
  const y1 = +(ry + rh).toFixed(2);
  const xr0 = +(rx + rX).toFixed(2);
  const xr1 = +(rx + rw - rX).toFixed(2);
  const yr0 = +(ry + rY).toFixed(2);
  const yr1 = +(ry + rh - rY).toFixed(2);
  const rxFmt = +rX.toFixed(2);
  const ryFmt = +rY.toFixed(2);
  const oL = +outerLeft.toFixed(2);
  const oT = +outerTop.toFixed(2);
  const oR = +outerRight.toFixed(2);
  const oB = +outerBottom.toFixed(2);
  const outer = `M${oL} ${oT} H${oR} V${oB} H${oL} Z`;
  const hole = `M${xr0} ${y0} H${xr1} A${rxFmt} ${ryFmt} 0 0 1 ${x1} ${yr0} V${yr1} A${rxFmt} ${ryFmt} 0 0 1 ${xr1} ${y1} H${xr0} A${rxFmt} ${ryFmt} 0 0 1 ${x0} ${yr1} V${yr0} A${rxFmt} ${ryFmt} 0 0 1 ${xr0} ${y0} Z`;
  return `path(evenodd, "${outer} ${hole}")`;
}

function setCachedStyle(
  element: HTMLElement,
  prop: 'transform' | 'clipPath' | 'opacity',
  previous: string,
  next: string,
): string {
  if (previous === next) return previous;
  element.style[prop] = next;
  return next;
}

function isRectInsideStadium(
  rectLeft: number,
  rectTop: number,
  rectRight: number,
  rectBottom: number,
  cx: number,
  cy: number,
  halfW: number,
  halfH: number,
  radius: number,
  margin: number,
): boolean {
  const safeR = radius - margin;
  if (safeR <= 1) return false;
  const straightX = Math.max(0, halfW - radius);
  const straightY = Math.max(0, halfH - radius);
  const dx = Math.max(0, Math.max(Math.abs(rectLeft - cx), Math.abs(rectRight - cx)) - straightX);
  const dy = Math.max(0, Math.max(Math.abs(rectTop - cy), Math.abs(rectBottom - cy)) - straightY);
  return dx * dx + dy * dy <= safeR * safeR;
}

function clearTabCutouts(state: PillState) {
  for (const entry of state.accentTabs) {
    entry.lastClipPath = setCachedStyle(entry.element, 'clipPath', entry.lastClipPath, '');
    entry.lastOpacity = setCachedStyle(entry.element, 'opacity', entry.lastOpacity, '');
  }
}

function clearTabVisuals(state: PillState) {
  clearTabCutouts(state);
  for (const entry of state.accentTabs) {
    entry.lastTransform = setCachedStyle(entry.element, 'transform', entry.lastTransform, '');
    entry.lastMirrorTransform = setCachedStyle(entry.mirror, 'transform', entry.lastMirrorTransform, '');
  }
  state.lastRecessClip = setCachedStyle(state.barRecess, 'clipPath', state.lastRecessClip, '');
  state.lastRecessTransform = setCachedStyle(state.barRecessSurface, 'transform', state.lastRecessTransform, '');
  state.lastBarCutout = setCachedStyle(state.barSurface, 'clipPath', state.lastBarCutout, '');
  state.lastBarTransform = setCachedStyle(state.barSurface, 'transform', state.lastBarTransform, '');
  state.lastAccentClip = setCachedStyle(state.accentLayer, 'clipPath', state.lastAccentClip, '');
  state.lastDropletTransform = setCachedStyle(state.droplet, 'transform', state.lastDropletTransform, '');
  state.lastDropletOpacity = setCachedStyle(state.droplet, 'opacity', state.lastDropletOpacity, '');
  state.lastGlowOpacity = setCachedStyle(state.barGlow, 'opacity', state.lastGlowOpacity, '');
  state.barRecessGlow.style.opacity = '';
}

function syncAccentLayer(state: PillState) {
  const {container, accentLayer} = state;
  const tabs = getHorizontalTabs(container);
  if (!tabs.length) {
    clearTabVisuals(state);
    state.accentTabs = [];
    if (state.accentSignature) {
      state.accentSignature = '';
      accentLayer.replaceChildren();
    }
    return;
  }

  const bounds = container.getBoundingClientRect();
  const entries = tabs.map(tab => {
    const rect = getUntransformedRect(tab);
    const left = rect.left - bounds.left + container.scrollLeft;
    const top = rect.top - bounds.top + container.scrollTop;
    const iconSvg = tab.querySelector<SVGElement>(':scope > svg:not(.nav-tree-chevron)');
    const labelSpan = tab.querySelector<HTMLElement>(':scope > span');
    // Measure actual icon + label ink bounds once on layout sync so fully-covered tabs skip path() cutouts.
    const tabCx = rect.left + rect.width / 2;
    const tabCy = rect.top + rect.height / 2;
    let inkHalfW = rect.width / 2;
    let inkHalfH = rect.height / 2;
    if (iconSvg || labelSpan) {
      let maxDx = 0;
      let maxDy = 0;
      for (const child of [iconSvg, labelSpan]) {
        if (!child) continue;
        const cb = child.getBoundingClientRect();
        maxDx = Math.max(maxDx, Math.abs(cb.left - tabCx), Math.abs(cb.right - tabCx));
        maxDy = Math.max(maxDy, Math.abs(cb.top - tabCy), Math.abs(cb.bottom - tabCy));
      }
      inkHalfW = Math.min(rect.width / 2, maxDx + 1.5);
      inkHalfH = Math.min(rect.height / 2, maxDy + 1.5);
    }
    return {
      tab,
      left,
      top,
      width: rect.width,
      height: rect.height,
      inkHalfW,
      inkHalfH,
      sig: `${Math.round(left)}:${Math.round(top)}:${Math.round(rect.width)}:${Math.round(rect.height)}:${iconSvg ? 's' : 'n'}:${labelSpan?.textContent || ''}`,
    };
  });

  const signature = `${Math.round(bounds.width)}x${Math.round(bounds.height)}|${entries.map(e => e.sig).join(';')}`;
  if (signature === state.accentSignature && state.accentTabs.length === entries.length) {
    state.accentTabs = entries.map((entry, index) => ({
      ...state.accentTabs[index],
      element: entry.tab,
      left: entry.left,
      top: entry.top,
      width: entry.width,
      height: entry.height,
      inkHalfW: entry.inkHalfW,
      inkHalfH: entry.inkHalfH,
    }));
    return;
  }
  clearTabVisuals(state);
  state.accentSignature = signature;

  const mirrors: HTMLElement[] = [];
  const nextTabs: TabLayoutBox[] = [];
  for (const entry of entries) {
    const mirror = document.createElement('span');
    mirror.className = 'nav-liquid-accent-item';
    mirror.style.left = `${entry.left}px`;
    mirror.style.top = `${entry.top}px`;
    mirror.style.width = `${entry.width}px`;
    mirror.style.height = `${entry.height}px`;
    for (const child of Array.from(entry.tab.children)) {
      const tag = child.tagName.toLowerCase();
      if ((tag === 'svg' && !child.classList.contains('nav-tree-chevron')) || tag === 'span') {
        mirror.append(child.cloneNode(true));
      }
    }
    mirrors.push(mirror);
    nextTabs.push({
      element: entry.tab,
      mirror,
      left: entry.left,
      top: entry.top,
      width: entry.width,
      height: entry.height,
      inkHalfW: entry.inkHalfW,
      inkHalfH: entry.inkHalfH,
      lastTransform: '',
      lastMirrorTransform: '',
      lastClipPath: '',
      lastOpacity: '',
    });
  }
  state.accentTabs = nextTabs;
  accentLayer.replaceChildren(...mirrors);
}

function rubberBand(distance: number, limit: number) {
  return limit * distance / (limit + Math.abs(distance));
}

function syncNativeDropletSlot(host: HTMLElement, droplet: HTMLElement, visible: boolean) {
  if (!isMacosNativeGlassActive() || getMacosNativeGlassEngine() !== 'ns-glass-effect') return;
  if (!isBoxVisible(host)) return;
  if (!visible) {
    setMacosGlassSlot('dock-droplet', {
      id: 'dock-droplet',
      placement: 'overlay',
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      cornerRadius: 0,
      style: 'clear',
      visible: false,
    }, true);
    return;
  }
  const rect = droplet.getBoundingClientRect();
  setMacosGlassSlot('dock-droplet', {
    id: 'dock-droplet',
    placement: 'overlay',
    x: rect.left,
    y: rect.top,
    width: rect.width,
    height: rect.height,
    cornerRadius: rect.height / 2,
    style: 'clear',
    visible: rect.width > 1 && rect.height > 1,
    interactive: true,
  });
}

function paintDockSurface(
  state: PillState,
  pullX: number,
  pullY: number,
): { barX: number; barY: number; pressScale: number; barScaleX: number; barScaleY: number } {
  const {host, barSurface, barGlow, barRecessGlow, barBox, lift} = state;
  const clampedLift = Math.max(0, lift);
  const dropletActive = clampedLift > 0;
  if (dropletActive !== state.lastDropletActive) {
    state.lastDropletActive = dropletActive;
    host.classList.toggle('is-droplet-active', dropletActive);
  }
  const rawGlow = Math.max(0, Math.min(1, state.barScale));
  const nextGlow = rawGlow > 0.002 ? rawGlow.toFixed(3) : '';
  if (nextGlow !== state.lastGlowOpacity) {
    state.lastGlowOpacity = nextGlow;
    barGlow.style.opacity = nextGlow;
    barRecessGlow.style.opacity = nextGlow;
  }
  const pullMag = Math.hypot(pullX, pullY);
  const hasPull = pullMag >= 0.02;
  const hasScale = Math.abs(state.barScale) >= 0.002;
  const barAnimating = dropletActive || hasPull || hasScale;
  if (barAnimating !== state.lastBarAnimating) {
    state.lastBarAnimating = barAnimating;
    host.classList.toggle('is-bar-animating', barAnimating);
  }
  if (!barBox || (!hasPull && !hasScale)) {
    state.lastBarTransform = setCachedStyle(barSurface, 'transform', state.lastBarTransform, '');
    state.lastBarCutout = setCachedStyle(barSurface, 'clipPath', state.lastBarCutout, '');
    state.lastBarX = 0;
    state.lastBarY = 0;
    return { barX: 0, barY: 0, pressScale: 1, barScaleX: 1, barScaleY: 1 };
  }
  const minBarX = -Math.max(0, barBox.left - EDGE_CLEARANCE);
  const maxBarX = Math.max(0, window.innerWidth - barBox.right - EDGE_CLEARANCE);
  const barX = hasPull ? Math.max(minBarX, Math.min(maxBarX, pullX)) : 0;
  const maxBarY = Math.max(0, window.innerHeight - barBox.bottom - EDGE_CLEARANCE);
  const barY = hasPull ? Math.max(-PULL_LIMIT_Y, Math.min(maxBarY, pullY)) : 0;
  const maxPressScaleX = Math.max(
    0,
    ((Math.min(barBox.left, window.innerWidth - barBox.right) - EDGE_CLEARANCE) * 2) / barBox.width,
  );
  const maxPressScaleY = Math.max(
    0,
    ((window.innerHeight - barBox.bottom - EDGE_CLEARANCE) * 2) / barBox.height,
  );
  const maxPressScale = Math.min(BAR_PRESS_SCALE_MAX, maxPressScaleX, maxPressScaleY);
  const pressScale = 1 + maxPressScale * state.barScale;
  const stretchX = hasPull ? Math.min(0.016, (Math.abs(barX) / barBox.width) * 0.75) : 0;
  const barScaleX = pressScale + stretchX;
  const barScaleY = pressScale - stretchX * 0.35;
  const barTransform = `translate3d(${barX.toFixed(2)}px, ${barY.toFixed(2)}px, 0) scale(${barScaleX.toFixed(4)}, ${barScaleY.toFixed(4)})`;
  state.lastBarTransform = setCachedStyle(barSurface, 'transform', state.lastBarTransform, barTransform);
  state.lastBarX = barX;
  state.lastBarY = barY;
  return { barX, barY, pressScale, barScaleX, barScaleY };
}

function paintHorizontalPill(state: PillState) {
  const {host, barSurface, pill, accentLayer, droplet, barRecess, barRecessSurface, x, width, top, height} = state;
  const lift = Math.max(0, state.lift);
  const pullWeight = state.pressed ? lift : Math.min(1, lift + Math.hypot(state.pullX, state.pullY) * 0.08);
  const pullX = state.pullX * pullWeight;
  const pullY = state.pullY * pullWeight;
  const { barX, barY, pressScale, barScaleX, barScaleY } = paintDockSurface(state, pullX, pullY);

  const dockCenterX = state.dockWidth / 2;
  const dockCenterY = state.dockHeight / 2;
  const pillRadX = (x + width / 2 - dockCenterX) * (pressScale - 1);
  const pillRadY = (top + height / 2 - dockCenterY) * (pressScale - 1);
  if (Math.abs(width - state.lastPillW) > 0.05) {
    state.lastPillW = width;
    pill.style.width = `${width}px`;
  }
  if (Math.abs(height - state.lastPillH) > 0.05) {
    state.lastPillH = height;
    pill.style.height = `${height}px`;
  }
  const nextPillOpacity = lift > 0 ? Math.max(0, 1 - lift * 2.2).toFixed(3) : '';
  state.lastPillOpacity = setCachedStyle(pill, 'opacity', state.lastPillOpacity, nextPillOpacity);
  pill.style.transform = `translate3d(${(x + barX + pillRadX).toFixed(2)}px, ${(top + barY + pillRadY).toFixed(2)}px, 0) scale(${pressScale.toFixed(4)})`;

  const itemTransforms = state.accentTabs.map(entry => {
    const radX = (entry.left + entry.width / 2 - dockCenterX) * (pressScale - 1);
    const radY = (entry.top + entry.height / 2 - dockCenterY) * (pressScale - 1);
    const itemTx = barX + radX;
    const itemTy = barY + radY;
    const nextTransform = Math.abs(itemTx) > 0.01 || Math.abs(itemTy) > 0.01 || Math.abs(pressScale - 1) > 0.0002
      ? `translate3d(${itemTx.toFixed(2)}px, ${itemTy.toFixed(2)}px, 0) scale(${pressScale.toFixed(4)})`
      : '';
    entry.lastTransform = setCachedStyle(entry.element, 'transform', entry.lastTransform, nextTransform);
    return { itemTx, itemTy };
  });

  if (!lift) {
    state.lastDropletTransform = setCachedStyle(droplet, 'transform', state.lastDropletTransform, '');
    state.lastDropletOpacity = setCachedStyle(droplet, 'opacity', state.lastDropletOpacity, '');
    state.lastAccentClip = setCachedStyle(accentLayer, 'clipPath', state.lastAccentClip, '');
    state.lastRecessClip = setCachedStyle(barRecess, 'clipPath', state.lastRecessClip, '');
    state.lastRecessTransform = setCachedStyle(barRecessSurface, 'transform', state.lastRecessTransform, '');
    state.lastBarCutout = setCachedStyle(barSurface, 'clipPath', state.lastBarCutout, '');
    clearTabCutouts(state);
    for (const entry of state.accentTabs) {
      entry.lastMirrorTransform = setCachedStyle(entry.mirror, 'transform', entry.lastMirrorTransform, '');
    }
    syncNativeDropletSlot(host, droplet, false);
    return;
  }

  const visualLift = Math.min(1.06, lift);
  const deform = Math.max(-0.35, Math.min(DEFORM_LIMIT, state.deform));
  const maxSpan = state.leftRoom + state.rightEdge;
  const baseLensW = width + LENS_WIDTH_GAIN * visualLift;
  const baseLensH = height + LENS_HEIGHT_GAIN * visualLift;
  const minFastH = state.barBox
    ? Math.max(height + 1, Math.min(baseLensH, state.barBox.height - 6))
    : Math.min(baseLensH, height + 2);
  const lensWidthRaw = deform >= 0
    ? baseLensW * (1 + 0.28 * deform)
    : baseLensW * (1 + 0.16 * deform);
  const lensHeightRaw = deform >= 0
    ? baseLensH - (baseLensH - minFastH) * deform
    : baseLensH * (1 - 0.10 * deform);
  const lensWidth = Math.max(1, Math.min(maxSpan, lensWidthRaw));
  const halfLens = lensWidth / 2;
  const middleRaw = x + width / 2 + barX;
  const middle = Math.max(-state.leftRoom + halfLens, Math.min(state.rightEdge - halfLens, middleRaw));
  const barCenterY = state.barBox
    ? state.barBox.height / 2 - state.containerTopInHost
    : top + height / 2;
  const middleY = barCenterY + barY + 0.12 * pullY;
  const lensHeight = Math.max(1, Math.min(lensHeightRaw, 2 * (state.bottomEdge - middleY)));
  const left = middle - state.lensWidth / 2;
  const y = middleY - state.lensHeight / 2;
  const nextDropletTransform = `translate3d(${left.toFixed(2)}px, ${y.toFixed(2)}px, 0) scale(${(lensWidth / state.lensWidth).toFixed(4)}, ${(lensHeight / state.lensHeight).toFixed(4)})`;
  state.lastDropletTransform = setCachedStyle(droplet, 'transform', state.lastDropletTransform, nextDropletTransform);
  const nextDropletOpacity = Math.max(0, Math.min(1, lift * 2.4)).toFixed(3);
  state.lastDropletOpacity = setCachedStyle(droplet, 'opacity', state.lastDropletOpacity, nextDropletOpacity);

  const clipTop = middleY - lensHeight / 2;
  const clipLeft = middle - lensWidth / 2;
  const clipRight = state.dockWidth - (middle + lensWidth / 2);
  const clipBottom = state.dockHeight - (middleY + lensHeight / 2);
  const clipRadius = Math.min(lensWidth / 2, lensHeight / 2);
  const nextAccentClip = `inset(${clipTop.toFixed(2)}px ${clipRight.toFixed(2)}px ${clipBottom.toFixed(2)}px ${clipLeft.toFixed(2)}px round ${clipRadius.toFixed(2)}px)`;
  state.lastAccentClip = setCachedStyle(accentLayer, 'clipPath', state.lastAccentClip, nextAccentClip);

  if (state.barBox) {
    const barW = state.barBox.width;
    const barH = state.barBox.height;
    const hLeft = clipLeft + state.containerLeftInHost;
    const hTop = clipTop + state.containerTopInHost;
    const hRight = barW - (hLeft + lensWidth);
    const hBottom = barH - (hTop + lensHeight);
    const nextRecessClip = `inset(${hTop.toFixed(2)}px ${hRight.toFixed(2)}px ${hBottom.toFixed(2)}px ${hLeft.toFixed(2)}px round ${clipRadius.toFixed(2)}px)`;
    state.lastRecessClip = setCachedStyle(barRecess, 'clipPath', state.lastRecessClip, nextRecessClip);

    const recessFactor = visualLift * (1 - Math.max(0, deform));
    const recessScaleY = 1 - 0.176 * recessFactor;
    const recessScaleX = 1 - 0.0204 * recessFactor;
    const nextRecessTransform = `translate3d(${barX.toFixed(2)}px, ${barY.toFixed(2)}px, 0) scale(${(barScaleX * recessScaleX).toFixed(4)}, ${(barScaleY * recessScaleY).toFixed(4)})`;
    state.lastRecessTransform = setCachedStyle(barRecessSurface, 'transform', state.lastRecessTransform, nextRecessTransform);

    const hcX = hLeft + lensWidth / 2;
    const hcY = hTop + lensHeight / 2;
    const localCx = barW / 2 + (hcX - barX - barW / 2) / barScaleX;
    const localCy = barH / 2 + (hcY - barY - barH / 2) / barScaleY;
    const localW = lensWidth / barScaleX;
    const localH = lensHeight / barScaleY;
    const nextCutout = buildStadiumHolePath(
      -32,
      -32,
      barW + 32,
      barH + 32,
      localCx - localW / 2,
      localCy - localH / 2,
      localW,
      localH,
      clipRadius / barScaleX,
      clipRadius / barScaleY,
    );
    state.lastBarCutout = setCachedStyle(barSurface, 'clipPath', state.lastBarCutout, nextCutout);
  }

  const itemMag = 1 + ITEM_MAG_GAIN * visualLift;
  const mirrorScale = (pressScale * itemMag).toFixed(4);
  state.accentTabs.forEach((entry, index) => {
    const { itemTx, itemTy } = itemTransforms[index];
    const nextMirrorTransform = `translate3d(${itemTx.toFixed(2)}px, ${itemTy.toFixed(2)}px, 0) scale(${mirrorScale})`;
    entry.lastMirrorTransform = setCachedStyle(entry.mirror, 'transform', entry.lastMirrorTransform, nextMirrorTransform);

    const eCx = entry.left + entry.width / 2 + itemTx;
    const eCy = entry.top + entry.height / 2 + itemTy;
    const tabVisLeft = eCx - (entry.width * pressScale) / 2;
    const tabVisRight = tabVisLeft + entry.width * pressScale;
    if (clipLeft + lensWidth <= tabVisLeft - 4 || clipLeft >= tabVisRight + 4) {
      entry.lastOpacity = setCachedStyle(entry.element, 'opacity', entry.lastOpacity, '');
      entry.lastClipPath = setCachedStyle(entry.element, 'clipPath', entry.lastClipPath, '');
      return;
    }

    const inkHalfW = entry.inkHalfW * pressScale;
    const inkHalfH = entry.inkHalfH * pressScale;
    if (isRectInsideStadium(
      eCx - inkHalfW,
      eCy - inkHalfH,
      eCx + inkHalfW,
      eCy + inkHalfH,
      middle,
      middleY,
      lensWidth / 2,
      lensHeight / 2,
      clipRadius,
      1,
    )) {
      entry.lastOpacity = setCachedStyle(entry.element, 'opacity', entry.lastOpacity, '0');
      entry.lastClipPath = setCachedStyle(entry.element, 'clipPath', entry.lastClipPath, '');
      return;
    }

    entry.lastOpacity = setCachedStyle(entry.element, 'opacity', entry.lastOpacity, '');
    const localCx = entry.width / 2 + (middle - eCx) / pressScale;
    const localCy = entry.height / 2 + (middleY - eCy) / pressScale;
    const localW = lensWidth / pressScale;
    const localH = lensHeight / pressScale;
    const localR = clipRadius / pressScale;
    const nextClip = buildStadiumHolePath(
      -32,
      -32,
      entry.width + 32,
      entry.height + 32,
      localCx - localW / 2,
      localCy - localH / 2,
      localW,
      localH,
      localR,
      localR,
    );
    entry.lastClipPath = setCachedStyle(entry.element, 'clipPath', entry.lastClipPath, nextClip);
  });

  syncNativeDropletSlot(host, droplet, lift > 0.02);
}

function stopHorizontalMotion(state: PillState) {
  cancelAnimationFrame(state.frame);
  state.frame = 0;
  state.lastFrame = 0;
}

function snapHorizontalPill(state: PillState) {
  stopHorizontalMotion(state);
  state.x = state.targetX;
  state.width = state.targetWidth;
  state.velocity = 0;
  state.lift = 0;
  state.liftVel = 0;
  state.barScale = 0;
  state.barScaleVel = 0;
  state.overscrollX = 0;
  state.glideUntil = 0;
  state.pullX = 0;
  state.pullY = 0;
  state.pullVelX = 0;
  state.pullVelY = 0;
  state.deform = 0;
  state.deformVel = 0;
  clearTabVisuals(state);
  paintHorizontalPill(state);
}

function moveHorizontalPill(state: PillState) {
  if (reducedMotion.matches) {
    snapHorizontalPill(state);
    return;
  }
  if (state.frame) return;
  const step = (time: number) => {
    const dt = state.lastFrame ? Math.min((time - state.lastFrame) / 1000, 0.05) : 1 / 60;
    state.lastFrame = time;

    // A time-based, damped spring keeps the same feel at 60 Hz and 120 Hz.
    const rate = state.dragging ? 38 : 24;
    const delta = state.x - state.targetX;
    const pull = state.velocity + rate * delta;
    const decay = Math.exp(-rate * dt);
    state.x = state.targetX + (delta + pull * dt) * decay;
    state.velocity = (state.velocity - rate * pull * dt) * decay;
    state.width += (state.targetWidth - state.width) * (1 - Math.exp(-30 * dt));
    const glideLift = state.glideUntil > time && Math.abs(state.targetX - state.x) > 4;
    if (!glideLift && state.glideUntil && time >= state.glideUntil) {
      state.glideUntil = 0;
    }

    const liftTarget = !reducedTransparency.matches && (state.pressed || glideLift) ? 1 : 0;
    [state.lift, state.liftVel] = stepSpring(
      state.lift,
      state.liftVel,
      liftTarget,
      liftTarget > state.lift ? LIFT_OPEN_OMEGA : LIFT_CLOSE_OMEGA,
      liftTarget > state.lift ? LIFT_OPEN_ZETA : LIFT_CLOSE_ZETA,
      dt,
    );
    if (liftTarget === 0 && state.lift < 0.008 && Math.abs(state.liftVel) < 0.12) {
      state.lift = 0;
      state.liftVel = 0;
    }

    const barScaleTarget = !reducedTransparency.matches && state.pressed ? 1 : 0;
    [state.barScale, state.barScaleVel] = stepSpring(
      state.barScale,
      state.barScaleVel,
      barScaleTarget,
      barScaleTarget > 0 ? BAR_OPEN_OMEGA : BAR_CLOSE_OMEGA,
      barScaleTarget > 0 ? BAR_OPEN_ZETA : BAR_CLOSE_ZETA,
      dt,
    );

    const pullOmega = state.pressed ? 24 : PULL_RETURN_OMEGA;
    const pullZeta = state.pressed ? 1 : PULL_RETURN_ZETA;
    [state.pullX, state.pullVelX] = stepSpring(state.pullX, state.pullVelX, state.targetPullX, pullOmega, pullZeta, dt);
    [state.pullY, state.pullVelY] = stepSpring(state.pullY, state.pullVelY, state.targetPullY, pullOmega, pullZeta, dt);
    if (!state.dragging) {
      state.overscrollX += (0 - state.overscrollX) * (1 - Math.exp(-19 * dt));
    }

    const edgeCalm = Math.max(0, 1 - Math.abs(state.overscrollX) / 5);
    const effectiveSpeed = Math.max(0, Math.abs(state.velocity) - 20);
    const targetDeform = state.lift > 0.05
      ? rubberBand(effectiveSpeed / 460, DEFORM_LIMIT) * Math.min(1, state.lift) * edgeCalm
      : 0;
    [state.deform, state.deformVel] = stepSpring(
      state.deform,
      state.deformVel,
      targetDeform,
      DEFORM_OMEGA,
      DEFORM_ZETA,
      dt,
    );

    if (Math.abs(state.targetX - state.x) < 0.35
      && Math.abs(state.velocity) < 5
      && Math.abs(state.targetWidth - state.width) < 0.25
      && Math.abs(state.targetPullX - state.pullX) < 0.12
      && Math.abs(state.targetPullY - state.pullY) < 0.12
      && Math.abs(state.pullVelX) < 0.5
      && Math.abs(state.pullVelY) < 0.5
      && Math.abs(targetDeform - state.deform) < 0.002
      && Math.abs(state.deformVel) < 0.02
      && Math.abs(barScaleTarget - state.barScale) < 0.004
      && Math.abs(state.barScaleVel) < 0.04
      && Math.abs(liftTarget - state.lift) < 0.008
      && Math.abs(state.liftVel) < 0.08) {
      state.frame = 0;
      state.lastFrame = 0;
      state.x = state.targetX;
      state.width = state.targetWidth;
      state.velocity = 0;
      state.lift = liftTarget;
      state.liftVel = 0;
      state.barScale = barScaleTarget;
      state.barScaleVel = 0;
      if (!state.dragging) state.overscrollX = 0;
      state.glideUntil = 0;
      state.pullX = state.targetPullX;
      state.pullY = state.targetPullY;
      state.pullVelX = 0;
      state.pullVelY = 0;
      state.deform = targetDeform;
      state.deformVel = 0;
    } else {
      state.frame = requestAnimationFrame(step);
    }
    paintHorizontalPill(state);
  };
  state.frame = requestAnimationFrame(step);
}

function positionPill(state: PillState, animate: boolean) {
  const {container, host, pill, pillInner} = state;
  const isPrimary = host.id === 'nav';
  const resolved = isPrimary ? resolvePrimaryNavActiveTarget(container) : resolveBillingDockActiveTarget(container);
  if (!resolved.target) {
    state.cancelPress?.();
    stopHorizontalMotion(state);
    state.lastKey = '';
    state.dockWidth = 0;
    state.dockHeight = 0;
    state.lift = 0;
    state.liftVel = 0;
    state.barScale = 0;
    state.barScaleVel = 0;
    state.overscrollX = 0;
    state.glideUntil = 0;
    state.pullVelX = 0;
    state.pullVelY = 0;
    state.deform = 0;
    state.deformVel = 0;
    clearTabVisuals(state);
    paintDockSurface(state, 0, 0);
    state.lastPillOpacity = setCachedStyle(pill, 'opacity', state.lastPillOpacity, '0');
    host.classList.remove('has-liquid-pill');
    return;
  }

  const horizontal = isHorizontalMobileDock();
  const bounds = container.getBoundingClientRect();
  const target = horizontal ? getUntransformedRect(resolved.target) : resolved.target.getBoundingClientRect();
  const changed = state.lastKey !== resolved.key;
  let left = target.left - bounds.left + container.scrollLeft;
  let top = target.top - bounds.top + container.scrollTop;
  if (!horizontal) {
    const cOffset = getNetTransformOffset(container);
    const tOffset = getNetTransformOffset(resolved.target);
    left += cOffset.dx - tOffset.dx;
    top += cOffset.dy - tOffset.dy;
  }

  pill.classList.toggle('is-parent-pill', resolved.isParentPill);
  state.lastPillOpacity = setCachedStyle(pill, 'opacity', state.lastPillOpacity, '');
  host.classList.add('has-liquid-pill');

  if (horizontal) {
    const dockResized = Math.abs(bounds.width - state.dockWidth) > 2
      || Math.abs(bounds.height - state.dockHeight) > 2;
    const sizeChanged = dockResized
      || Math.abs(bounds.width - state.dockWidth) > 0.25
      || Math.abs(bounds.height - state.dockHeight) > 0.25
      || Math.abs(target.width - state.targetWidth) > 0.25
      || Math.abs(target.height - state.height) > 0.25;
    if (state.pressed && !dockResized && !changed) return;
    if (state.pressed) state.cancelPress?.();

    const wasHorizontal = state.dockWidth > 0;
    state.lastKey = resolved.key;
    state.dockWidth = bounds.width;
    state.dockHeight = bounds.height;
    state.leftRoom = bounds.left - EDGE_CLEARANCE;
    state.rightEdge = window.innerWidth - bounds.left - EDGE_CLEARANCE;
    state.bottomEdge = window.innerHeight - bounds.top - EDGE_CLEARANCE;
    state.barBox = host.getBoundingClientRect();
    state.containerLeftInHost = bounds.left - state.barBox.left;
    state.containerTopInHost = bounds.top - state.barBox.top;
    state.targetX = left;
    state.targetWidth = target.width;
    state.top = top;
    state.height = target.height;
    state.overscrollX = 0;
    syncAccentLayer(state);
    if (sizeChanged || !wasHorizontal || !host.style.getPropertyValue('--dock-backdrop')) {
      state.lensWidth = target.width + LENS_WIDTH_GAIN;
      state.lensHeight = target.height + LENS_HEIGHT_GAIN;
      state.droplet.style.width = `${state.lensWidth}px`;
      state.droplet.style.height = `${state.lensHeight}px`;
      if (!reducedTransparency.matches) {
        updateDockGlass(host, state.barBox, state.lensWidth, state.lensHeight);
      }
    }

    const needsSettle = changed
      || state.lift > 0
      || Math.abs(state.barScale) > 0.002
      || Math.hypot(state.pullX, state.pullY) > 0.02
      || Math.abs(state.x - left) > 0.35;
    if (wasHorizontal && !sizeChanged && (state.frame || (animate && needsSettle))) {
      moveHorizontalPill(state);
    } else {
      snapHorizontalPill(state);
    }
    return;
  }

  state.cancelPress?.();
  stopHorizontalMotion(state);
  state.dockWidth = 0;
  state.dockHeight = 0;
  state.lift = 0;
  state.liftVel = 0;
  state.barScale = 0;
  state.barScaleVel = 0;
  state.overscrollX = 0;
  state.glideUntil = 0;
  state.pullVelX = 0;
  state.pullVelY = 0;
  state.deform = 0;
  state.deformVel = 0;
  clearTabVisuals(state);
  paintDockSurface(state, 0, 0);
  if (isPrimary) {
    syncNativeDropletSlot(host, state.droplet, false);
  }

  // Preserve the sliding desktop and tablet sidebar indicator.
  const transition = animate && state.lastKey.startsWith('v:') && !reducedMotion.matches;
  pill.classList.toggle('no-pill-transition', !transition);
  state.lastPillW = target.width;
  state.lastPillH = target.height;
  pill.style.width = `${target.width}px`;
  pill.style.height = `${target.height}px`;
  pill.style.transform = `translate3d(${left}px, ${top}px, 0)`;
  if (!transition) {
    void pill.offsetWidth;
    requestAnimationFrame(() => pill.classList.remove('no-pill-transition'));
  } else if (changed && typeof pillInner.animate === 'function') {
    pillInner.getAnimations().forEach(animation => animation.cancel());
    pillInner.animate(
      [{transform: 'scale(1)'}, {transform: 'scale(0.95, 1.1)'}, {transform: 'scale(1.01, 0.98)'}, {transform: 'scale(1)'}],
      {duration: 300, easing: 'cubic-bezier(0.22, 1.22, 0.36, 1)'},
    );
  }
  state.lastKey = resolved.key;
}

function attachDragScrub(state: PillState) {
  const {container, host} = state;
  let pointerId: number | null = null;
  let ignoredClick: number | null = null;
  let startX = 0;
  let startY = 0;
  let grabOffsetX = 0;
  let dockLeft = 0;
  let pressedTabEl: HTMLElement | null = null;
  let tabs: {element: HTMLElement; box: DOMRect}[] = [];
  let preview: HTMLElement | null = null;

  const previewTab = (tab: HTMLElement | null) => {
    if (preview === tab) return;
    if (preview) delete preview.dataset.liquidHover;
    preview = tab;
    if (tab) tab.dataset.liquidHover = 'true';
    host.classList.toggle('is-liquid-previewing', Boolean(tab));
  };

  const nearestTabIn = (x: number, candidates: {element: HTMLElement; box: DOMRect}[]) => {
    let nearest: HTMLElement | null = null;
    let distance = Infinity;
    for (const {element, box} of candidates) {
      const offset = Math.abs(x - box.left - box.width / 2);
      if (offset < distance) {
        nearest = element;
        distance = offset;
      }
    }
    return nearest;
  };

  const cancel = (ignoreClick = true, preserveGlide = false) => {
    if (pointerId === null) return;
    const id = pointerId;
    if (ignoreClick) {
      ignoredClick = id;
      requestAnimationFrame(() => {
        if (ignoredClick === id) ignoredClick = null;
      });
    }
    pointerId = null;
    pressedTabEl = null;
    state.pressed = false;
    state.dragging = false;
    state.targetPullX = 0;
    state.targetPullY = 0;
    if (!preserveGlide) state.glideUntil = 0;
    previewTab(null);
    host.classList.remove('is-liquid-dragging');
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp, true);
    window.removeEventListener('pointercancel', onCancel);
    try {
      if (host.hasPointerCapture(id)) host.releasePointerCapture(id);
    } catch {
      // Ignore capture-release failures on detached elements.
    }
  };
  state.cancelPress = cancel;

  const onMove = (event: PointerEvent) => {
    if (event.pointerId !== pointerId) return;
    if (event.pointerType === 'mouse' && !(event.buttons & 1)) {
      onUp(event);
      return;
    }
    const dx = event.clientX - startX;
    const dy = event.clientY - startY;
    const hoveredTab = nearestTabIn(event.clientX, tabs);
    if (!state.dragging) {
      if (
        event.pointerType !== 'mouse'
        && Math.abs(dy) > 28
        && Math.abs(dy) > Math.abs(dx) * 2.5
        && hoveredTab === pressedTabEl
      ) {
        cancel();
        positionPill(state, true);
        return;
      }
      if (Math.abs(dx) < 5 && (!hoveredTab || hoveredTab === pressedTabEl)) return;
      state.dragging = true;
      host.classList.add('is-liquid-dragging');
      if (event.isTrusted) {
        try {
          host.setPointerCapture(event.pointerId);
        } catch {
          // Ignore synthetic or non-capturable pointers.
        }
      }
    }

    if (event.cancelable) event.preventDefault();

    const offsetWeight = Math.max(0, 1 - Math.hypot(dx, dy) / 56);
    const first = tabs[0].box;
    const last = tabs[tabs.length - 1].box;
    const minX = first.left;
    const maxX = last.right - state.targetWidth;
    const rawX = event.clientX - grabOffsetX * offsetWeight - state.targetWidth / 2;
    const excess = rawX < minX ? rawX - minX : rawX > maxX ? rawX - maxX : 0;
    const overscroll = excess ? rubberBand(excess, OVERSCROLL_LIMIT_X) : 0;
    state.targetX = (Math.max(minX, Math.min(maxX, rawX)) + overscroll) - dockLeft;
    state.overscrollX = overscroll;

    const barBox = state.barBox;
    if (barBox) {
      const marginX = Math.min(barBox.left, window.innerWidth - barBox.right);
      const deadzoneX = Math.min(18, Math.max(4, marginX * 0.4));
      const deadzoneTop = 20;
      const deadzoneBottom = Math.min(16, Math.max(4, (window.innerHeight - barBox.bottom) * 0.4));
      const outsideX = event.clientX < barBox.left - deadzoneX
        ? event.clientX - (barBox.left - deadzoneX)
        : event.clientX > barBox.right + deadzoneX
          ? event.clientX - (barBox.right + deadzoneX)
          : 0;
      const outsideY = event.clientY < barBox.top - deadzoneTop
        ? event.clientY - (barBox.top - deadzoneTop)
        : event.clientY > barBox.bottom + deadzoneBottom
          ? event.clientY - (barBox.bottom + deadzoneBottom)
          : 0;
      state.targetPullX = outsideX ? rubberBand(outsideX * 0.32, PULL_LIMIT_X) : 0;
      state.targetPullY = outsideY ? rubberBand(outsideY * 0.18, PULL_LIMIT_Y) : 0;
    } else {
      state.targetPullX = 0;
      state.targetPullY = 0;
    }

    previewTab(hoveredTab);
    moveHorizontalPill(state);
  };

  const onUp = (event: PointerEvent) => {
    if (event.pointerId !== pointerId) return;
    // The browser may coalesce the last move into the release. Hit-test the
    // release even if the animation and the move handler have not run yet.
    const releaseTab = nearestTabIn(event.clientX, tabs);
    const movedToOtherTab = Boolean(releaseTab && releaseTab !== pressedTabEl);
    const dragged = state.dragging || Math.abs(event.clientX - startX) >= 5 || movedToOtherTab;
    const targetTab = dragged ? releaseTab : pressedTabEl;
    const shouldSwitch = Boolean(targetTab && !targetTab.classList.contains('active') && isBoxVisible(targetTab));
    const switchingOnTap = !dragged && shouldSwitch;
    cancel(dragged || shouldSwitch, switchingOnTap);
    if (shouldSwitch && targetTab) {
      if (switchingOnTap) state.glideUntil = performance.now() + 260;
      targetTab.click();
    }
    positionPill(state, true);
    if (state.dockWidth) moveHorizontalPill(state);
  };

  const onCancel = (event: PointerEvent) => {
    if (event.pointerId !== pointerId) return;
    cancel();
    positionPill(state, true);
  };
  host.addEventListener('dragstart', event => event.preventDefault());

  host.addEventListener('pointerdown', event => {
    if (pointerId !== null || !event.isPrimary || event.button !== 0 || !isHorizontalMobileDock()) return;
    ignoredClick = null;
    if (!state.dockWidth) return;
    syncAccentLayer(state);
    tabs = getHorizontalTabs(container).map(element => ({
      element,
      box: getUntransformedRect(element),
    }));
    const directTarget = event.target instanceof Element
      ? event.target.closest<HTMLElement>('.nav-item, .billing-section-nav-button')
      : null;
    const fallbackEl = nearestTabIn(event.clientX, tabs);
    const pressedTab = tabs.find(tab => tab.element === (directTarget || fallbackEl));
    if (tabs.length < 2 || !pressedTab) return;

    pointerId = event.pointerId;
    startX = event.clientX;
    startY = event.clientY;
    grabOffsetX = event.clientX - (pressedTab.box.left + pressedTab.box.width / 2);
    pressedTabEl = pressedTab.element;
    state.glideUntil = 0;
    state.overscrollX = 0;
    state.pressed = true;
    dockLeft = container.getBoundingClientRect().left;
    state.targetX = pressedTab.box.left - dockLeft;
    previewTab(pressedTab.element);
    moveHorizontalPill(state);
    window.addEventListener('pointermove', onMove, {passive: false});
    window.addEventListener('pointerup', onUp, true);
    window.addEventListener('pointercancel', onCancel);
  });

  host.addEventListener('click', event => {
    if (ignoredClick === null || !event.isTrusted || event.detail === 0
      || (event instanceof PointerEvent && event.pointerId > 0 && event.pointerId !== ignoredClick)) return;
    ignoredClick = null;
    event.preventDefault();
    event.stopPropagation();
  }, true);
}

function registerPillHost(container: HTMLElement, host: HTMLElement, tag: 'li' | 'div') {
  const barSurface = document.createElement('div');
  barSurface.className = 'nav-liquid-bar-surface';
  barSurface.setAttribute('aria-hidden', 'true');
  const barGlow = document.createElement('span');
  barGlow.className = 'nav-liquid-bar-glow';
  barSurface.append(barGlow);

  const barRecess = document.createElement('div');
  barRecess.className = 'nav-liquid-bar-recess';
  barRecess.setAttribute('aria-hidden', 'true');
  const barRecessSurface = document.createElement('span');
  barRecessSurface.className = 'nav-liquid-bar-recess-surface';
  const barRecessGlow = document.createElement('span');
  barRecessGlow.className = 'nav-liquid-bar-recess-glow';
  barRecessSurface.append(barRecessGlow);
  barRecess.append(barRecessSurface);
  host.prepend(barSurface, barRecess);

  const pill = document.createElement(tag);
  pill.className = 'nav-liquid-pill';
  pill.setAttribute('aria-hidden', 'true');
  const pillInner = document.createElement('span');
  pillInner.className = 'nav-liquid-pill-inner';
  pill.append(pillInner);

  const accentLayer = document.createElement(tag);
  accentLayer.className = 'nav-liquid-accent-layer';
  accentLayer.setAttribute('aria-hidden', 'true');

  const droplet = document.createElement(tag);
  droplet.className = 'nav-liquid-droplet';
  droplet.setAttribute('aria-hidden', 'true');
  container.prepend(pill, accentLayer, droplet);

  const state: PillState = {
    container, host, barSurface, barGlow, barRecess, barRecessSurface, barRecessGlow, pill, pillInner, accentLayer, accentSignature: '', accentTabs: [], droplet,
    lastKey: '', pressed: false, dragging: false, glideUntil: 0, frame: 0, lastFrame: 0,
    x: 0, width: 0, targetX: 0, targetWidth: 0, velocity: 0, lift: 0, liftVel: 0,
    barScale: 0, barScaleVel: 0,
    overscrollX: 0, pullX: 0, pullY: 0, pullVelX: 0, pullVelY: 0, targetPullX: 0, targetPullY: 0,
    deform: 0, deformVel: 0, lastBarX: 0, lastBarY: 0,
    lastPillW: 0, lastPillH: 0, lastPillOpacity: '', lastDropletActive: false, lastBarAnimating: false,
    lastGlowOpacity: '', lastBarTransform: '', lastBarCutout: '', lastRecessClip: '',
    lastRecessTransform: '', lastAccentClip: '', lastDropletOpacity: '', lastDropletTransform: '',
    containerLeftInHost: 0, containerTopInHost: 0,
    top: 0, height: 0, dockWidth: 0, dockHeight: 0, leftRoom: 0, rightEdge: 0, bottomEdge: 0,
    barBox: null, lensWidth: 0, lensHeight: 0,
  };
  controllers.push(state);
  attachDragScrub(state);
}

export function syncLiquidNav(options: {animate?: boolean} = {}) {
  for (const state of controllers) positionPill(state, options.animate !== false);
}

export function setupLiquidNav() {
  if (initialized) return;
  if ((window as Window & { __SKIP_LIQUID_NAV__?: boolean }).__SKIP_LIQUID_NAV__) return;
  initialized = true;
  const nav = document.getElementById('nav');
  const items = nav?.querySelector<HTMLElement>('.nav-items');
  const billing = document.querySelector<HTMLElement>('.billing-mobile-nav');
  if (nav && items) registerPillHost(items, nav, 'li');
  if (billing) registerPillHost(billing, billing, 'div');

  const syncInstant = () => syncLiquidNav({animate: false});
  const cancelAndSync = () => {
    for (const state of controllers) {
      state.cancelPress?.();
      stopHorizontalMotion(state);
    }
    syncInstant();
  };
  window.addEventListener('resize', cancelAndSync);
  window.addEventListener('blur', cancelAndSync);
  reducedMotion.addEventListener('change', cancelAndSync);
  reducedTransparency.addEventListener('change', cancelAndSync);
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) cancelAndSync();
  });

  const trees = [document.getElementById('nav-dashboard-children'), document.getElementById('nav-billing-tree')];
  const resize = new ResizeObserver(syncInstant);
  for (const element of [nav, items, billing, ...trees]) {
    if (element) resize.observe(element);
  }
  for (const tree of trees) {
    tree?.addEventListener('transitionend', event => {
      if (event.target === tree) syncInstant();
    });
  }
  const observeLayout = new MutationObserver(syncInstant);
  observeLayout.observe(document.documentElement, {attributes: true, attributeFilter: ['data-theme']});
  observeLayout.observe(document.body, {attributes: true, attributeFilter: ['class']});

  void initMacosLiquidGlass().then(() => {
    syncInstant();
    if (isMacosNativeGlassActive()) {
      const observeSurfaceOverlays = new MutationObserver(() => {
        flushMacosLiquidGlass();
      });
      observeSurfaceOverlays.observe(document.body, {
        subtree: true,
        attributes: true,
        attributeFilter: ['class', 'hidden'],
      });
    }
  });
  syncInstant();
}
