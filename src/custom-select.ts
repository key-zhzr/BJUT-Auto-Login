interface LayoutMetrics {
  cardLeft: number;
  cardTop: number;
  cardWidth: number;
  cardHeight: number;
  triggerLeft: number;
  triggerTop: number;
  triggerWidth: number;
  triggerHeight: number;
  triggerRadius: number;
  boundLeft: number;
  boundRight: number;
  boundTop: number;
  boundBottom: number;
  usePopover: boolean;
}

interface MorphSnapshot {
  top: number;
  height: number;
  radius: number;
  scaleX: number;
  opacity: number;
  reveal: number;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** Evaluates a normalized underdamped unit-mass spring (0 -> 1, v0 = 0). */
function evalSpring(t: number, stiffness: number, ratio: number): number {
  if (t <= 0) return 0;
  const w0 = Math.sqrt(stiffness);
  const zeta = clamp(ratio, 0.05, 0.999);
  const wd = w0 * Math.sqrt(1 - zeta * zeta);
  const env = Math.exp(-zeta * w0 * t);
  return 1 - env * (Math.cos(wd * t) + ((zeta * w0) / wd) * Math.sin(wd * t));
}

/** Matches FlClash prism_popup riseAndFall pulse curve for dip/lift/blur bumps. */
function riseAndFall(t: number, peak: number, shape: number): number {
  if (t <= 0 || peak <= 0) return 0;
  const x = t / peak;
  return Math.pow(x * Math.exp(1 - x), shape);
}

function rubberBand(delta: number, limit: number): number {
  if (Math.abs(delta) < 0.01) return 0;
  return Math.sign(delta) * limit * (1 - Math.exp(-Math.abs(delta) / (limit * 2.4)));
}

function isBasicTheme(): boolean {
  return document.documentElement.dataset.theme === 'basic';
}

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === 'function'
    && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

function readViewportBounds(element: HTMLElement, triggerRect: DOMRect) {
  const viewport = window.visualViewport;
  const left = viewport?.offsetLeft ?? 0;
  const width = viewport?.width ?? document.documentElement.clientWidth;
  const right = left + width;
  const viewportTop = viewport?.offsetTop ?? 0;
  const scroller = element.closest<HTMLElement>('main, .modal-content');
  const bounds = scroller?.getBoundingClientRect();
  let top = Math.max(bounds?.top ?? 0, viewportTop);
  let bottom = Math.min(bounds?.bottom ?? window.innerHeight, viewportTop + (viewport?.height ?? window.innerHeight));
  if (scroller?.tagName === 'MAIN') {
    const header = element.closest('.page')?.querySelector('.page-header');
    if (header && !header.contains(element)) top = Math.max(top, header.getBoundingClientRect().bottom);
    const dock = element.closest('#billing-center')?.querySelector('.billing-mobile-nav');
    for (const nav of [document.querySelector('body:not(.is-desktop) #nav'), dock]) {
      if (!nav) continue;
      const style = getComputedStyle(nav);
      const rect = nav.getBoundingClientRect();
      // A sidebar can retain its full height while animating into a bottom dock.
      if (style.position === 'fixed' && style.visibility === 'visible' && rect.height > 0 && rect.width > rect.height
        && rect.right > triggerRect.left && rect.left < triggerRect.right) {
        bottom = Math.min(bottom, rect.top);
      }
    }
  }
  return { left, right, width, top, bottom };
}

function bakeSpringTransformKeyframes(
  steps: number,
  fromDy: number,
  toDy: number,
  fromSX: number,
  toSX: number,
  fromSY: number,
  toSY: number,
  fromOp: number,
  toOp: number,
  stiffness: number,
  ratio: number,
  springSpanSec: number,
  dipSign: number,
  dipAmp: number,
  mode: 'open' | 'close',
): Keyframe[] {
  const frames: Keyframe[] = [];
  for (let i = 0; i <= steps; i++) {
    const f = i / steps;
    const tSec = f * springSpanSec;
    const rawProgress = i === steps ? 1 : evalSpring(tSec, stiffness, ratio);
    const ringing = mode === 'close' ? 1 - Math.pow(clamp((f - 0.78) / 0.22, 0, 1), 2) : 1;
    const progress = mode === 'close' ? 1 + (rawProgress - 1) * ringing : rawProgress;
    const peak = mode === 'open' ? 0.098 : 0.072;
    const shape = mode === 'open' ? 3.3 : 2.4;
    const dip = dipSign * dipAmp * riseAndFall(tSec, peak, shape) * ringing;
    const dy = lerp(fromDy, toDy, progress) + dip;
    const sx = mode === 'open'
      ? lerp(fromSX, toSX, clamp(progress * 1.25, 0, 1))
      : lerp(fromSX, toSX, clamp(progress, 0, 1));
    const sy = Math.max(0.25, lerp(fromSY, toSY, progress));
    let op = 1;
    if (mode === 'open') {
      op = lerp(fromOp, toOp, clamp(f / 0.14, 0, 1));
    } else {
      const fadeT = clamp((f - 0.56) / 0.44, 0, 1);
      op = lerp(fromOp, toOp, fadeT * fadeT * (3 - 2 * fadeT));
    }
    frames.push({
      offset: f,
      transform: `translate3d(0, ${dy.toFixed(2)}px, 0) scale(${sx.toFixed(4)}, ${sy.toFixed(4)})`,
      opacity: op.toFixed(3),
    });
  }
  return frames;
}

export class CustomSelect {
  private static openInstance: CustomSelect | null = null;
  private static outsideHandlerInstalled = false;
  private static layoutObserver: ResizeObserver | null = null;
  private static placementFrame: number | null = null;
  private static ignoreOutsideClickUntil = 0;
  element: HTMLElement;
  trigger: HTMLElement;
  triggerSpan: HTMLSpanElement;
  optionsContainer: HTMLElement;
  private _value = '';
  private activeIndex = 0;
  private disabled = false;
  private allOptions: HTMLElement[] = [];
  private optionsByValue = new Map<string, HTMLElement[]>();
  private selectedOptions: HTMLElement[] = [];
  private highlightedOption: HTMLElement | null = null;
  private pressHoveredOption: HTMLElement | null = null;
  private scrollFrame: number | null = null;
  private closeTimer: number | null = null;
  private morphFrame: number | null = null;
  private morphMode: 'open' | 'close' | null = null;
  private morphState: MorphSnapshot | null = null;
  private openAnimations: Animation[] = [];
  private closeAnimations: Animation[] = [];
  private androidMorphStartedAt = 0;
  private androidMorphFrom = { dy: 0, sx: 1, sy: 1, op: 1 };
  private layoutMetrics: LayoutMetrics | null = null;
  private elasticFrame: number | null = null;
  private elastic = { x: 0, y: 0, sx: 1, sy: 1 };
  private elasticAnchorY: number | null = null;
  private elasticBoundaryMode: 'top' | 'bottom' | 'edge' | null = null;
  private lastPointerY = 0;
  private touchElasticId: number | null = null;
  private touchStartX = 0;
  private touchStartY = 0;
  private touchLastY = 0;
  private touchAnchorY: number | null = null;
  private touchElasticActive = false;
  private touchBoundaryMode: 'top' | 'bottom' | 'edge' | null = null;
  private isScrollableList = false;
  private wheelPullY = 0;
  private wheelBounceTimer: number | null = null;
  private triggerHoldTimer: number | null = null;
  private ignoreTriggerClickUntil = 0;
  private openedInBasic = false;
  private hasPaintedValue = false;
  private scrollPositions = new WeakMap<Element, {top: number; left: number}>();
  onChangeCallbacks: ((value: string) => void)[] = [];

  private options() { return this.allOptions.filter(option => option.getAttribute('aria-disabled') !== 'true'); }
  private usePopover() {
    return typeof (this.optionsContainer as HTMLElement & { showPopover?: () => void }).showPopover === 'function';
  }
  private setPressHover(option: HTMLElement | null) {
    if (this.pressHoveredOption === option) return;
    if (this.pressHoveredOption) delete this.pressHoveredOption.dataset.pressHover;
    this.pressHoveredOption = option;
    if (option) option.dataset.pressHover = 'true';
  }
  private syncScrollMask() {
    const el = this.optionsContainer;
    const top = el.scrollTop;
    const max = Math.max(0, el.scrollHeight - el.clientHeight);
    el.classList.toggle('can-scroll-up', top > 2);
    el.classList.toggle('can-scroll-down', max - top > 2);
  }
  private clearTriggerInlineStyles() {
    this.trigger.style.opacity = '';
    this.trigger.style.filter = '';
    this.trigger.style.transform = '';
    this.trigger.style.pointerEvents = '';
  }
  private cancelAnimationsList(list: Animation[]) {
    for (const anim of list) {
      anim.onfinish = null;
      anim.cancel();
    }
    list.length = 0;
  }
  private clearElasticState() {
    if (this.elasticFrame !== null) {
      cancelAnimationFrame(this.elasticFrame);
      this.elasticFrame = null;
    }
    if (this.wheelBounceTimer !== null) {
      window.clearTimeout(this.wheelBounceTimer);
      this.wheelBounceTimer = null;
    }
    this.wheelPullY = 0;
    this.elasticAnchorY = null;
    this.elasticBoundaryMode = null;
    this.touchElasticId = null;
    this.touchAnchorY = null;
    this.touchBoundaryMode = null;
    this.touchElasticActive = false;
    this.elastic = { x: 0, y: 0, sx: 1, sy: 1 };
    if (this.morphMode === null) this.optionsContainer.style.transform = '';
  }
  private applyElasticTransform(x: number, y: number, sx: number, sy: number) {
    this.elastic = { x, y, sx, sy };
    if (this.morphMode !== null) return;
    this.optionsContainer.style.transform =
      Math.abs(x) < 0.05 && Math.abs(y) < 0.05 && Math.abs(sx - 1) < 0.0005 && Math.abs(sy - 1) < 0.0005
        ? ''
        : `translate3d(${x.toFixed(2)}px, ${y.toFixed(2)}px, 0) scale(${sx.toFixed(4)}, ${sy.toFixed(4)})`;
  }
  private springElasticBack() {
    this.elasticAnchorY = null;
    this.elasticBoundaryMode = null;
    if (this.elasticFrame !== null) cancelAnimationFrame(this.elasticFrame);
    const from = { ...this.elastic };
    if (Math.abs(from.x) < 0.2 && Math.abs(from.y) < 0.2 && Math.abs(from.sx - 1) < 0.002 && Math.abs(from.sy - 1) < 0.002) {
      this.clearElasticState();
      return;
    }
    const started = performance.now();
    const step = (now: number) => {
      if (this.morphMode !== null || !this.element.classList.contains('open')) {
        this.elasticFrame = null;
        return;
      }
      const elapsedSec = (now - started) / 1000;
      const t = elapsedSec >= 0.28 ? 1 : evalSpring(elapsedSec, 380, 0.72);
      this.applyElasticTransform(lerp(from.x, 0, t), lerp(from.y, 0, t), lerp(from.sx, 1, t), lerp(from.sy, 1, t));
      if (elapsedSec >= 0.28) {
        this.elasticFrame = null;
        this.applyElasticTransform(0, 0, 1, 1);
        return;
      }
      this.elasticFrame = requestAnimationFrame(step);
    };
    this.elasticFrame = requestAnimationFrame(step);
  }
  private markMorphVisibleOptions(cardHeight: number) {
    const scrollTop = this.optionsContainer.scrollTop;
    const minTop = scrollTop - 48;
    const maxBottom = scrollTop + cardHeight + 48;
    const visibleFlags = this.allOptions.map((option) => {
      const top = option.offsetTop;
      const bottom = top + option.offsetHeight;
      return bottom >= minTop && top <= maxBottom;
    });
    for (let i = 0; i < this.allOptions.length; i += 1) {
      this.allOptions[i].classList.toggle('morph-visible', visibleFlags[i]);
    }
  }
  private clearMorphContainerStyles() {
    if (this.morphFrame !== null) {
      cancelAnimationFrame(this.morphFrame);
      this.morphFrame = null;
    }
    this.cancelAnimationsList(this.openAnimations);
    this.cancelAnimationsList(this.closeAnimations);
    this.morphMode = null;
    this.morphState = null;
    this.element.classList.remove('is-morphing');
    const style = this.optionsContainer.style;
    style.height = '';
    style.borderRadius = '';
    style.transform = '';
    style.opacity = '';
    style.removeProperty('--opt-opacity');
    style.removeProperty('--opt-shift-y');
    style.removeProperty('--opt-filter');
    for (const option of this.allOptions) option.classList.remove('morph-visible');
  }
  private cancelMorph(snapOpen = false) {
    const wasMode = this.morphMode;
    this.clearMorphContainerStyles();
    if (snapOpen && wasMode === 'open' && this.element.classList.contains('open')) {
      this.clearTriggerInlineStyles();
      if (isBasicTheme()) this.element.classList.add('is-trigger-hidden');
    } else if (!this.element.classList.contains('open')) {
      this.element.classList.remove('is-trigger-hidden');
      this.clearTriggerInlineStyles();
    }
  }
  private hideOptions() {
    if (this.triggerHoldTimer !== null) {
      window.clearTimeout(this.triggerHoldTimer);
      this.triggerHoldTimer = null;
    }
    this.clearMorphContainerStyles();
    this.clearElasticState();
    this.clearTriggerInlineStyles();
    this.element.classList.remove('closing', 'is-trigger-hidden', 'covers-anchor', 'opens-up');
    this.setPressHover(null);
    const popover = this.optionsContainer as HTMLElement & { hidePopover?: () => void };
    if (typeof popover.hidePopover === 'function' && this.optionsContainer.matches(':popover-open')) {
      try { popover.hidePopover(); } catch {}
    }
    this.optionsContainer.hidden = true;
    this.optionsContainer.inert = true;
    this.optionsContainer.classList.remove('can-scroll-up', 'can-scroll-down');
    const style = this.optionsContainer.style;
    style.top = '';
    style.bottom = '';
    style.left = '';
    style.width = '';
    style.maxHeight = '';
    style.overflowY = '';
    style.touchAction = '';
    this.isScrollableList = false;
    this.layoutMetrics = null;
    this.openedInBasic = false;
  }
  private positionMenu(): boolean {
    const trigger = this.trigger.getBoundingClientRect();
    const { left, right, width, top, bottom } = readViewportBounds(this.element, trigger);
    if (!trigger.width || !trigger.height || !width || trigger.right <= left || trigger.left >= right
      || trigger.bottom <= top || trigger.top >= bottom) return false;

    const { style } = this.optionsContainer;
    if (this.morphMode === null) style.height = '';
    const isBasic = isBasicTheme();

    if (isBasic) {
      const pad = 8;
      const minTop = top + pad;
      const maxBottom = bottom - pad;
      const available = Math.max(0, maxBottom - minTop);
      if (available < 24) return false;

      const isAndroid = document.body.classList.contains('is-android');
      const itemH = this.allOptions[0]?.offsetHeight || (isAndroid ? 44 : 40);
      const optionCount = this.optionsContainer.children.length || this.allOptions.length;
      const cs = getComputedStyle(this.optionsContainer);
      const triggerRadius = Number.parseFloat(getComputedStyle(this.trigger).borderTopLeftRadius) || 12;
      const borderY = (Number.parseFloat(cs.borderTopWidth) || 0) + (Number.parseFloat(cs.borderBottomWidth) || 0);
      const contentScrollH = this.optionsContainer.scrollHeight || itemH + 16;
      const fullBorderBoxH = Math.ceil(contentScrollH + borderY);
      // Account for border-box borders so non-scrollable menus never show a 2px phantom scrollbar.
      const flClashMax = optionCount > 7
        ? Math.round(8 + 7.5 * itemH + borderY)
        : fullBorderBoxH;
      const effectiveMaxH = Math.min(flClashMax, available);
      style.maxHeight = `${effectiveMaxH}px`;

      const menuH = Math.min(fullBorderBoxH, effectiveMaxH);
      const isScrollable = fullBorderBoxH > effectiveMaxH + 1;
      this.isScrollableList = isScrollable;
      style.overflowY = isScrollable ? 'auto' : 'hidden';

      const maxWidth = Math.max(120, width - pad * 2);
      const menuWidth = Math.min(trigger.width, maxWidth);
      const menuLeft = Math.round(clamp(trigger.left, left + pad, Math.max(left + pad, right - pad - menuWidth)));
      const triggerCenterY = trigger.top + trigger.height / 2;
      const maxTop = Math.max(minTop, maxBottom - menuH);
      const menuTop = Math.round(clamp(triggerCenterY - menuH / 2, minTop, maxTop));
      const popover = this.usePopover();

      this.element.classList.remove('opens-up');
      this.element.classList.add('covers-anchor');
      if (popover) {
        style.left = `${menuLeft}px`;
        style.width = `${menuWidth}px`;
        style.top = `${menuTop}px`;
        style.bottom = 'auto';
      } else {
        style.left = `${Math.round(menuLeft - trigger.left)}px`;
        style.width = `${menuWidth}px`;
        style.top = `${Math.round(menuTop - trigger.top)}px`;
        style.bottom = 'auto';
      }
      this.layoutMetrics = {
        cardLeft: menuLeft,
        cardTop: menuTop,
        cardWidth: menuWidth,
        cardHeight: menuH,
        triggerLeft: trigger.left,
        triggerTop: trigger.top,
        triggerWidth: trigger.width,
        triggerHeight: trigger.height,
        triggerRadius,
        boundLeft: left,
        boundRight: right,
        boundTop: top,
        boundBottom: bottom,
        usePopover: popover,
      };
      return true;
    }

    this.element.classList.remove('covers-anchor');
    style.overflowY = '';
    this.isScrollableList = false;
    const maxMenuHeight = 220;
    const below = Math.max(0, bottom - trigger.bottom - 12);
    const above = Math.max(0, trigger.top - top - 12);
    const desired = Math.min(maxMenuHeight, this.optionsContainer.scrollHeight || maxMenuHeight);
    const opensUp = below < desired && above > below;
    const available = opensUp ? above : below;
    if (!available) return false;
    this.element.classList.toggle('opens-up', opensUp);
    style.maxHeight = `${Math.min(maxMenuHeight, available)}px`;
    if (this.usePopover()) {
      const menuWidth = Math.min(trigger.width, width);
      const gap = document.documentElement.dataset.theme === 'apple27' ? 6 : document.documentElement.dataset.theme === 'winui' ? 4 : 5;
      style.left = `${Math.max(left, Math.min(trigger.left, right - menuWidth))}px`;
      style.width = `${menuWidth}px`;
      style.top = opensUp ? 'auto' : `${trigger.bottom + gap}px`;
      style.bottom = opensUp ? `${window.innerHeight - trigger.top + gap}px` : 'auto';
    }
    return true;
  }
  private startAndroidBasicOpenMorph(fromInterrupted: { dy: number; sx: number; sy: number; op: number } | null) {
    const m = this.layoutMetrics;
    if (!m || typeof this.optionsContainer.animate !== 'function') {
      this.element.classList.add('is-trigger-hidden');
      return;
    }
    this.clearMorphContainerStyles();
    this.clearElasticState();
    this.morphMode = 'open';
    this.androidMorphStartedAt = performance.now();
    this.element.classList.add('is-morphing');
    this.element.classList.remove('is-trigger-hidden');

    const triggerCenterY = m.triggerTop + m.triggerHeight / 2;
    const cardCenterY = m.cardTop + m.cardHeight / 2;
    const toCard = cardCenterY - triggerCenterY;
    const dipSign = toCard === 0 ? 1 : Math.sign(toCard);
    const dipAmp = Math.min(1, Math.abs(toCard) / 30) * 14 + 0.08 * Math.abs(toCard);
    const dy0 = fromInterrupted ? fromInterrupted.dy : -toCard;
    const sy0 = fromInterrupted ? fromInterrupted.sy : clamp(m.triggerHeight / Math.max(1, m.cardHeight), 0.36, 0.9);
    const sx0 = fromInterrupted ? fromInterrupted.sx : 0.94;
    const op0 = fromInterrupted ? clamp(fromInterrupted.op, 0.3, 1) : 0.3;
    this.androidMorphFrom = { dy: dy0, sx: sx0, sy: sy0, op: op0 };

    const keyframes = bakeSpringTransformKeyframes(
      20, dy0, 0, sx0, 1, sy0, 1, op0, 1, 400, 0.754, 0.45, dipSign, dipAmp, 'open',
    );
    const cardAnim = this.optionsContainer.animate(keyframes, {
      duration: 340,
      easing: 'linear',
    });
    this.openAnimations.push(cardAnim);

    const trigAnim = this.trigger.animate(
      [{ opacity: 1 }, { opacity: 0 }],
      { duration: 90, easing: 'ease-out', fill: 'forwards' },
    );
    trigAnim.onfinish = () => {
      if (this.morphMode !== 'open') return;
      this.element.classList.add('is-trigger-hidden');
      trigAnim.cancel();
    };
    this.openAnimations.push(trigAnim);

    cardAnim.onfinish = () => {
      if (this.morphMode !== 'open') return;
      this.clearMorphContainerStyles();
      if (this.element.classList.contains('open')) {
        this.clearTriggerInlineStyles();
        this.element.classList.add('is-trigger-hidden');
      }
    };
  }
  private startAndroidBasicCloseMorph() {
    const m = this.layoutMetrics;
    if (!m || typeof this.optionsContainer.animate !== 'function') {
      this.hideOptions();
      return;
    }
    const triggerCenterY = m.triggerTop + m.triggerHeight / 2;
    const cardCenterY = m.cardTop + m.cardHeight / 2;
    const endDy = triggerCenterY - cardCenterY;
    const endSY = clamp(m.triggerHeight / Math.max(1, m.cardHeight), 0.36, 0.9);
    let startDy = this.morphMode === null ? this.elastic.y : 0;
    let startSX = this.morphMode === null ? this.elastic.sx : 1;
    let startSY = this.morphMode === null ? this.elastic.sy : 1;
    let startOp = 1;
    if (this.morphMode === 'open') {
      const f = clamp((performance.now() - this.androidMorphStartedAt) / 340, 0.08, 1);
      const progress = evalSpring(f * 0.45, 400, 0.754);
      startDy = lerp(this.androidMorphFrom.dy, 0, progress);
      startSX = lerp(this.androidMorphFrom.sx, 1, clamp(progress * 1.25, 0, 1));
      startSY = lerp(this.androidMorphFrom.sy, 1, progress);
      startOp = clamp(lerp(this.androidMorphFrom.op, 1, f / 0.14), 0.3, 1);
    }
    const startIsVisible = !this.element.classList.contains('is-trigger-hidden') ? 0.5 : 0;

    this.clearElasticState();
    this.clearMorphContainerStyles();
    this.markMorphVisibleOptions(m.cardHeight);
    this.morphMode = 'close';
    this.androidMorphStartedAt = performance.now();
    this.androidMorphFrom = { dy: startDy, sx: startSX, sy: startSY, op: startOp };
    this.element.classList.add('closing', 'is-morphing');
    this.element.classList.remove('is-trigger-hidden');
    this.trigger.style.opacity = String(startIsVisible);
    this.trigger.style.pointerEvents = 'none';

    const dist = Math.abs(cardCenterY - triggerCenterY);
    const away = (cardCenterY - triggerCenterY) === 0 ? 1 : Math.sign(cardCenterY - triggerCenterY);
    const dipAmp = Math.min(0.21 * dist, 18);
    const keyframes = bakeSpringTransformKeyframes(
      16, startDy, endDy, startSX, 0.96, startSY, endSY, startOp, 0, 300, 0.82, 0.42, away, dipAmp, 'close',
    );
    const cardAnim = this.optionsContainer.animate(keyframes, {
      duration: 260,
      easing: 'linear',
      fill: 'forwards',
    });
    this.closeAnimations.push(cardAnim);

    const trigAnim = this.trigger.animate(
      [
        { offset: 0, opacity: startIsVisible },
        { offset: 0.25, opacity: startIsVisible },
        { offset: 0.85, opacity: 1 },
        { offset: 1, opacity: 1 },
      ],
      { duration: 260, easing: 'linear', fill: 'forwards' },
    );
    this.closeAnimations.push(trigAnim);

    cardAnim.onfinish = () => {
      if (this.morphMode !== 'close') return;
      this.hideOptions();
    };
  }
  private startBasicOpenMorph(fromState: MorphSnapshot | null) {
    const m = this.layoutMetrics;
    if (!m || prefersReducedMotion()) {
      this.element.classList.add('is-trigger-hidden');
      return;
    }
    if (document.body.classList.contains('is-android')) {
      let interruptedAndroid: { dy: number; sx: number; sy: number; op: number } | null = null;
      if (this.morphMode === 'close') {
        const f = clamp((performance.now() - this.androidMorphStartedAt) / 260, 0.05, 0.95);
        const progress = evalSpring(f * 0.42, 300, 0.82);
        const cardCenterY = m.cardTop + m.cardHeight / 2;
        const endDy = (m.triggerTop + m.triggerHeight / 2) - cardCenterY;
        const endSY = clamp(m.triggerHeight / Math.max(1, m.cardHeight), 0.36, 0.9);
        interruptedAndroid = {
          dy: lerp(this.androidMorphFrom.dy, endDy, progress),
          sx: lerp(this.androidMorphFrom.sx, 0.96, progress),
          sy: lerp(this.androidMorphFrom.sy, endSY, progress),
          op: lerp(this.androidMorphFrom.op, 0, clamp((f - 0.56) / 0.44, 0, 1)),
        };
      }
      this.startAndroidBasicOpenMorph(interruptedAndroid);
      return;
    }
    this.clearMorphContainerStyles();
    this.clearElasticState();
    this.morphMode = 'open';
    this.element.classList.add('is-morphing');
    this.element.classList.remove('is-trigger-hidden');

    const startTop = fromState ? fromState.top : m.triggerTop;
    const startH = fromState ? fromState.height : m.triggerHeight;
    const startSX = fromState ? fromState.scaleX : 0.96;
    const startCenterY = startTop + startH / 2;
    const cardCenterY = m.cardTop + m.cardHeight / 2;
    const maxCapR = Math.min(m.cardHeight / 2, Math.max(22, Math.min(m.cardWidth * 0.16, 44)));
    const startReveal = fromState ? fromState.reveal : 0;
    const startOpacity = fromState ? Math.max(0.28, fromState.opacity) : 0.25;
    const started = performance.now();

    const applyFrame = (elapsedSec: number) => {
      if (!isBasicTheme() || !this.element.classList.contains('open')) {
        this.cancelMorph(false);
        if (!this.element.classList.contains('open')) this.hideOptions();
        return false;
      }
      const progress = evalSpring(elapsedSec, 400, 0.754);
      const settled = clamp(progress, 0, 1);
      const toCard = cardCenterY - startCenterY;
      const dist = Math.abs(toCard);
      const dip = (toCard === 0 ? 1 : Math.sign(toCard))
        * (Math.min(1, dist / 30) * 14 + 0.08 * dist)
        * riseAndFall(elapsedSec, 0.098, 3.3);
      const curH = Math.max(Math.min(startH, m.cardHeight), lerp(startH, m.cardHeight, progress));
      const curCenterY = lerp(startCenterY, cardCenterY, progress) + dip;
      const curTop = curCenterY - curH / 2;
      const dy = curTop - m.cardTop;
      const sx = lerp(startSX, 1, clamp(progress * 1.25, 0, 1));

      // FlClash delayed corners spring (_cornersDelay = 70ms, stiffness 324, ratio 0.75).
      const cornersVal = 1 - evalSpring(Math.max(0, elapsedSec - 0.07), 324, 0.75);
      const curRadius = clamp(20 + (maxCapR - 20) * Math.max(0, cornersVal), 16, curH / 2);

      // Hide source trigger under glass droplet cleanly inside the first ~115ms.
      const under = clamp(settled / 0.25, 0, 1);
      if (under < 1) {
        this.trigger.style.opacity = (1 - under).toFixed(3);
        this.trigger.style.filter = `blur(${(3.2 * under).toFixed(2)}px)`;
        this.trigger.style.pointerEvents = 'none';
      } else {
        this.clearTriggerInlineStyles();
        this.element.classList.add('is-trigger-hidden');
      }

      const optOpacity = clamp(lerp(startReveal, 1, clamp((settled - 0.05) / 0.62, 0, 1)), 0, 1);
      const blurPx = Math.hypot(4.5 * (1 - settled), 1.7 * riseAndFall(elapsedSec, 0.069, 2.1));
      const shiftY = -Math.max(0, m.cardHeight - curH) * 0.34;
      const glassOpacity = clamp(lerp(startOpacity, 1, elapsedSec / 0.05), 0.25, 1);

      const s = this.optionsContainer.style;
      s.height = `${curH.toFixed(1)}px`;
      s.borderRadius = `${curRadius.toFixed(1)}px`;
      s.transform = `translate3d(0, ${dy.toFixed(2)}px, 0) scaleX(${sx.toFixed(4)})`;
      s.opacity = glassOpacity.toFixed(3);
      s.setProperty('--opt-opacity', optOpacity.toFixed(3));
      s.setProperty('--opt-shift-y', `${shiftY.toFixed(1)}px`);
      s.setProperty('--opt-filter', blurPx > 0.2 ? `blur(${blurPx.toFixed(2)}px)` : 'none');

      this.morphState = {
        top: curTop,
        height: curH,
        radius: curRadius,
        scaleX: sx,
        opacity: glassOpacity,
        reveal: optOpacity,
      };
      return elapsedSec < 0.45;
    };

    applyFrame(0);
    const tick = (now: number) => {
      if (this.morphMode !== 'open') return;
      const elapsedSec = (now - started) / 1000;
      if (applyFrame(elapsedSec)) {
        this.morphFrame = requestAnimationFrame(tick);
      } else {
        this.clearMorphContainerStyles();
        if (this.element.classList.contains('open')) {
          this.clearTriggerInlineStyles();
          this.element.classList.add('is-trigger-hidden');
        }
      }
    };
    this.morphFrame = requestAnimationFrame(tick);
  }
  private startBasicCloseMorph() {
    const m = this.layoutMetrics;
    if (!m || !isBasicTheme() || prefersReducedMotion()) {
      this.hideOptions();
      return;
    }
    if (document.body.classList.contains('is-android')) {
      this.startAndroidBasicCloseMorph();
      return;
    }
    const interrupted = this.morphMode === 'open' ? this.morphState : null;
    const elasticOffsetY = this.morphMode === null ? this.elastic.y : 0;
    const elasticScaleX = this.morphMode === null ? this.elastic.sx : 1;
    this.clearElasticState();
    this.clearMorphContainerStyles();
    this.markMorphVisibleOptions(m.cardHeight);
    this.morphMode = 'close';
    this.element.classList.add('closing', 'is-morphing');
    this.element.classList.remove('is-trigger-hidden');
    // Keep trigger invisible until droplet return re-materializes it (+110ms).
    this.trigger.style.opacity = '0';
    this.trigger.style.pointerEvents = 'none';

    const startTop = interrupted ? interrupted.top : m.cardTop + elasticOffsetY;
    const startH = interrupted ? interrupted.height : m.cardHeight;
    const startR = interrupted ? interrupted.radius : 20;
    const startSX = interrupted ? interrupted.scaleX : elasticScaleX;
    const startReveal = interrupted ? interrupted.reveal : 1;
    const startCenterY = startTop + startH / 2;
    const triggerCenterY = m.triggerTop + m.triggerHeight / 2;
    const started = performance.now();

    const applyFrame = (elapsedSec: number) => {
      if (!isBasicTheme() || this.element.classList.contains('open')) {
        this.morphFrame = null;
        if (!this.element.classList.contains('open')) this.hideOptions();
        return false;
      }
      const ringing = 1 - Math.pow(clamp((elapsedSec - 0.34) / 0.08, 0, 1), 2);
      const progress = 1 + (evalSpring(elapsedSec, 300, 0.82) - 1) * ringing;
      const clamped = clamp(progress, 0, 1);
      const toCard = startCenterY - triggerCenterY;
      const away = toCard === 0 ? 1 : Math.sign(toCard);
      const dip = Math.min(0.21 * Math.abs(toCard), 20) * riseAndFall(elapsedSec, 0.072, 2.4) * ringing;
      const curH = Math.max(m.triggerHeight, lerp(startH, m.triggerHeight, clamped));
      const curCenterY = lerp(startCenterY, triggerCenterY, clamped) + away * dip;
      const curTop = curCenterY - curH / 2;
      const dy = curTop - m.cardTop;
      const sx = lerp(startSX, 1, clamped);

      const pillPeak = Math.min(curH / 2, 34);
      const curRadius = clamp(
        lerp(startR, m.triggerRadius, clamped) + Math.max(0, pillPeak - startR) * Math.sin(clamped * Math.PI),
        m.triggerRadius,
        curH / 2,
      );

      // Options blur and fade within the first 100ms (_dropletContentOut = 0.1).
      const outT = clamp(elapsedSec / 0.1, 0, 1);
      const reveal = startReveal * (1 - Math.pow(outT, 0.75));
      const blurPx = 7.5 * (1 - clamp(reveal, 0, 1));
      const shiftY = -Math.max(0, m.cardHeight - curH) * 0.34;

      // Trigger re-materializes beneath returning glass droplet (+110ms -> +320ms).
      const glyphT = clamp((elapsedSec - 0.11) / 0.21, 0, 1);
      const lift = Math.min(0.2 * Math.abs(toCard), 10) * riseAndFall(elapsedSec, 0.176, 4.9) * ringing;
      this.trigger.style.opacity = glyphT.toFixed(3);
      this.trigger.style.filter = glyphT > 0.01 && glyphT < 0.98
        ? `blur(${(3.2 * (1 - glyphT)).toFixed(2)}px)` : '';
      this.trigger.style.transform = lift > 0.1 ? `translate3d(0, ${(-away * lift).toFixed(2)}px, 0)` : '';
      this.trigger.style.pointerEvents = '';

      // Glass droplet dissolves smoothly after wrapping back onto trigger (+240ms -> +420ms).
      const absorbedT = clamp((elapsedSec - 0.24) / 0.18, 0, 1);
      const glassOpacity = 1 - absorbedT * absorbedT * (3 - 2 * absorbedT);

      const s = this.optionsContainer.style;
      s.height = `${curH.toFixed(1)}px`;
      s.borderRadius = `${curRadius.toFixed(1)}px`;
      s.transform = `translate3d(0, ${dy.toFixed(2)}px, 0) scaleX(${sx.toFixed(4)})`;
      s.opacity = glassOpacity.toFixed(3);
      s.setProperty('--opt-opacity', reveal.toFixed(3));
      s.setProperty('--opt-shift-y', `${shiftY.toFixed(1)}px`);
      s.setProperty('--opt-filter', reveal > 0.01 && blurPx > 0.2 ? `blur(${blurPx.toFixed(2)}px)` : 'none');

      this.morphState = {
        top: curTop,
        height: curH,
        radius: curRadius,
        scaleX: sx,
        opacity: glassOpacity,
        reveal,
      };
      return elapsedSec < 0.42;
    };

    applyFrame(0);
    const tick = (now: number) => {
      if (this.morphMode !== 'close') return;
      const elapsedSec = (now - started) / 1000;
      if (applyFrame(elapsedSec)) {
        this.morphFrame = requestAnimationFrame(tick);
      } else {
        this.hideOptions();
      }
    };
    this.morphFrame = requestAnimationFrame(tick);
  }
  private static schedulePlacement(): void {
    if (!CustomSelect.openInstance || CustomSelect.placementFrame !== null) return;
    CustomSelect.placementFrame = requestAnimationFrame(() => {
      CustomSelect.placementFrame = null;
      const current = CustomSelect.openInstance;
      if (!current) return;
      const m = current.layoutMetrics;
      if (m && current.morphMode === 'open') {
        const trigger = current.trigger.getBoundingClientRect();
        const b = readViewportBounds(current.element, trigger);
        const unchanged = Math.abs(trigger.left - m.triggerLeft) < 0.5
          && Math.abs(trigger.top - m.triggerTop) < 0.5
          && Math.abs(trigger.width - m.triggerWidth) < 0.5
          && Math.abs(trigger.height - m.triggerHeight) < 0.5
          && Math.abs(b.left - m.boundLeft) < 0.5
          && Math.abs(b.right - m.boundRight) < 0.5
          && Math.abs(b.top - m.boundTop) < 0.5
          && Math.abs(b.bottom - m.boundBottom) < 0.5;
        if (unchanged) return;
        current.cancelMorph(true);
      }
      if (!current.positionMenu()) current.close(true);
    });
  }
  private observeMenuLayout(): void {
    const observer = CustomSelect.layoutObserver;
    if (!observer) return;
    const scroller = this.element.closest('main, .modal-content');
    const header = this.element.closest('.page')?.querySelector('.page-header');
    const billingDock = this.element.closest('#billing-center')?.querySelector('.billing-mobile-nav');
    for (const element of [this.trigger, scroller, header, document.getElementById('nav'), billingDock]) {
      if (element) observer.observe(element, { box: 'border-box' });
    }
  }
  static closeOpen(): void {
    CustomSelect.openInstance?.close();
  }
  close(immediate = false) {
    if (CustomSelect.openInstance === this) {
      CustomSelect.openInstance = null;
      CustomSelect.layoutObserver?.disconnect();
    }
    this.setPressHover(null);
    if (this.scrollFrame !== null) { cancelAnimationFrame(this.scrollFrame); this.scrollFrame = null; }
    if (!this.element.classList.contains('open')) {
      if (immediate && this.element.classList.contains('closing')) this.hideOptions();
      return;
    }
    this.element.classList.remove('open'); this.trigger.setAttribute('aria-expanded', 'false');
    this.trigger.removeAttribute('aria-activedescendant');
    this.highlightedOption?.classList.remove('keyboard-active');
    this.highlightedOption = null;
    this.optionsContainer.inert = true;
    if (this.closeTimer !== null) window.clearTimeout(this.closeTimer);
    if (immediate || prefersReducedMotion()) {
      this.closeTimer = null;
      this.hideOptions();
      return;
    }
    if (this.openedInBasic && isBasicTheme() && this.layoutMetrics) {
      this.closeTimer = null;
      this.startBasicCloseMorph();
      return;
    }
    this.clearMorphContainerStyles();
    this.element.classList.remove('is-trigger-hidden');
    this.clearTriggerInlineStyles();
    const style = getComputedStyle(this.optionsContainer);
    const milliseconds = (value: string) => Math.max(0, ...value.split(',').map(part => Number.parseFloat(part) * (part.trim().endsWith('ms') ? 1 : 1000)));
    const duration = milliseconds(style.transitionDuration) + milliseconds(style.transitionDelay);
    const finishClose = () => {
      this.closeTimer = null;
      if (!this.element.classList.contains('open')) this.hideOptions();
    };
    if (duration <= 0) {
      finishClose();
      return;
    }
    this.element.classList.add('closing');
    this.closeTimer = window.setTimeout(finishClose, duration);
  }
  private highlight(index: number, keyboard = true) {
    const options = this.options(); if (!options.length) return;
    this.activeIndex = (index + options.length) % options.length;
    const option = options[this.activeIndex];
    const highlighted = keyboard ? option : null;
    if (this.highlightedOption !== highlighted) {
      this.highlightedOption?.classList.remove('keyboard-active');
      highlighted?.classList.add('keyboard-active');
      this.highlightedOption = highlighted;
    }
    this.trigger.setAttribute('aria-activedescendant', option.id);
    if (!keyboard && this.morphMode === 'open') return;
    // Coalesce rapid opens/arrow presses into one list-only scroll per frame.
    // Closing/replacing the menu cancels the pending read; ancestors never move.
    if (this.scrollFrame !== null) cancelAnimationFrame(this.scrollFrame);
    this.scrollFrame = requestAnimationFrame(() => {
      this.scrollFrame = null;
      if (!this.element.isConnected || !this.element.classList.contains('open')) return;
      const top = option.offsetTop;
      const bottom = top + option.offsetHeight;
      const viewport = this.optionsContainer;
      const scrollTop = viewport.scrollTop;
      const height = this.morphFrame !== null && this.layoutMetrics ? this.layoutMetrics.cardHeight : viewport.clientHeight;
      if (top < scrollTop) viewport.scrollTop = top;
      else if (bottom > scrollTop + height) viewport.scrollTop = bottom - height;
    });
  }
  private open(keyboard = false) {
    if (this.disabled) return;
    const prevOpen = CustomSelect.openInstance;
    if (prevOpen && prevOpen !== this) prevOpen.close();
    CustomSelect.openInstance = this;
    const interrupted = this.morphMode === 'close' ? this.morphState : null;
    const interruptedWasClose = this.morphMode === 'close';
    if (this.closeTimer !== null) { window.clearTimeout(this.closeTimer); this.closeTimer = null; }
    this.element.classList.remove('closing');
    if (this.morphMode !== null && (!interruptedWasClose || !document.body.classList.contains('is-android'))) {
      this.clearMorphContainerStyles();
    } else if (this.morphMode !== null) {
      // Android interruption keeps androidMorphStartedAt/From until startBasicOpenMorph reads it.
      this.cancelAnimationsList(this.openAnimations);
      this.cancelAnimationsList(this.closeAnimations);
    }
    this.clearElasticState();
    this.clearTriggerInlineStyles();
    this.element.classList.add('menu-positioning');
    this.optionsContainer.hidden = false;
    this.optionsContainer.inert = false;
    if (this.usePopover() && !this.optionsContainer.matches(':popover-open')) {
      try { (this.optionsContainer as HTMLElement & { showPopover: () => void }).showPopover(); } catch {}
    }
    // Resolve placement while the flyout is invisible. Keep that placement on
    // exit too; changing top/bottom halfway through a transform causes a jump.
    if (!this.positionMenu()) {
      this.element.classList.remove('menu-positioning');
      this.hideOptions();
      CustomSelect.openInstance = null;
      return;
    }
    const basic = isBasicTheme();
    if (!basic) {
      this.optionsContainer.getBoundingClientRect();
    }
    if (this.isScrollableList) {
      const clientH = this.optionsContainer.clientHeight;
      const maxScroll = Math.max(0, this.optionsContainer.scrollHeight - clientH);
      if (maxScroll > 2) {
        const selectedEl = this.selectedOptions[0];
        if (selectedEl) {
          const ideal = selectedEl.offsetTop + selectedEl.offsetHeight / 2 - clientH * 0.45;
          this.optionsContainer.scrollTop = Math.max(0, Math.min(maxScroll, ideal));
        }
      }
    }
    this.openedInBasic = basic;
    if (basic) {
      this.optionsContainer.style.touchAction = this.isScrollableList ? 'pan-y' : 'none';
    }
    if (basic && !this.isScrollableList) {
      this.optionsContainer.classList.remove('can-scroll-up', 'can-scroll-down');
    } else {
      this.syncScrollMask();
    }
    this.element.classList.remove('menu-positioning');
    this.element.classList.add('open'); this.trigger.setAttribute('aria-expanded', 'true');
    this.observeMenuLayout();
    this.scrollPositions = new WeakMap();
    for (let parent = this.element.parentElement; parent; parent = parent.parentElement) {
      this.scrollPositions.set(parent, {top: parent.scrollTop, left: parent.scrollLeft});
    }
    this.highlight(Math.max(0, this.options().findIndex(option => option.dataset.value === this._value)), keyboard);
    if (basic) {
      this.startBasicOpenMorph(interruptedWasClose ? interrupted : null);
    } else {
      this.element.classList.remove('is-trigger-hidden');
    }
  }
  private prepareOptions() {
    this.allOptions = Array.from(this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option'));
    this.optionsByValue.clear();
    this.selectedOptions = [];
    this.allOptions.forEach((option, index) => {
      option.id = `${this.element.id}-option-${index}`; option.setAttribute('role', 'option');
      const value = option.dataset.value || '';
      const group = this.optionsByValue.get(value) || [];
      group.push(option); this.optionsByValue.set(value, group);
      const selected = value === this._value;
      option.classList.toggle('selected', selected);
      option.setAttribute('aria-selected', String(selected));
      if (selected) this.selectedOptions.push(option);
    });
  }
  /** Computes C0-continuous elastic displacement across inside & outside-edge drags. */
  private updateElasticFromPointer(clientX: number, clientY: number, startX: number, startY: number, stepDy = 0) {
    if (!isBasicTheme() || this.morphMode !== null || !this.element.classList.contains('open') || !this.layoutMetrics) return;
    const { cardLeft, cardTop, cardWidth, cardHeight } = this.layoutMetrics;
    const cardRight = cardLeft + cardWidth;
    const cardBottom = cardTop + cardHeight;

    // Continuous X across left/right edges (zero jump when crossing bounds).
    const clampedX = clamp(clientX, cardLeft, cardRight);
    const clampedStartX = clamp(startX, cardLeft, cardRight);
    const rawX = (clampedX - clampedStartX) * 0.16 + (clientX - clampedX) * 0.85;

    let rawY = 0;
    const el = this.optionsContainer;
    const maxScroll = this.isScrollableList ? Math.max(0, el.scrollHeight - el.clientHeight) : 0;

    if (maxScroll <= 1) {
      // Non-scrollable card: C0-continuous across top/bottom edges.
      const clampedY = clamp(clientY, cardTop, cardBottom);
      const clampedStartY = clamp(startY, cardTop, cardBottom);
      rawY = (clampedY - clampedStartY) * 0.32 + (clientY - clampedY) * 0.9;
      this.elasticAnchorY = null;
      this.elasticBoundaryMode = null;
    } else {
      const outsideX = clientX < cardLeft || clientX > cardRight;
      const outsideY = clientY < cardTop || clientY > cardBottom;
      const atTop = el.scrollTop <= 0.5 && stepDy > 0;
      const atBottom = el.scrollTop >= maxScroll - 0.5 && stepDy < 0;

      if (this.elasticAnchorY === null) {
        if (outsideY || outsideX || atTop || atBottom) {
          this.elasticBoundaryMode = (outsideY || outsideX) ? 'edge' : atTop ? 'top' : 'bottom';
          this.elasticAnchorY = clientY < cardTop ? cardTop : clientY > cardBottom ? cardBottom : clientY;
        }
      }
      if (this.elasticAnchorY !== null) {
        rawY = clientY - this.elasticAnchorY;
        if (!outsideX && !outsideY) {
          if ((this.elasticBoundaryMode === 'top' && rawY < 0) || (this.elasticBoundaryMode === 'bottom' && rawY > 0)) {
            this.elasticAnchorY = null;
            this.elasticBoundaryMode = null;
            rawY = 0;
          }
        }
      }
    }

    const pullX = rubberBand(rawX, 11);
    const pullY = rubberBand(rawY, 20);
    const pullMag = Math.hypot(pullX, pullY);
    const swell = 1 + Math.min(0.014, pullMag / 450);
    const stretchY = 1 + Math.min(0.032, Math.abs(pullY) / 380);
    this.applyElasticTransform(pullX, pullY, swell / Math.sqrt(stretchY), swell * stretchY);
  }
  constructor(elementId: string) {
    this.element = document.getElementById(elementId)!;
    this.trigger = this.element.querySelector('.custom-select-trigger')!;
    this.triggerSpan = this.trigger.querySelector('span')!;
    this.optionsContainer = this.element.querySelector('.custom-select-options')!;
    if (this.usePopover()) this.optionsContainer.setAttribute('popover', 'manual');
    this.optionsContainer.hidden = true;
    this.optionsContainer.inert = true;
    this.trigger.tabIndex = 0; this.trigger.setAttribute('role', 'combobox');
    this.trigger.setAttribute('aria-haspopup', 'listbox'); this.trigger.setAttribute('aria-expanded', 'false');
    const accessibleLabel = this.element.getAttribute('aria-label');
    if (accessibleLabel) this.trigger.setAttribute('aria-label', accessibleLabel);
    this.optionsContainer.id ||= `${elementId}-options`;
    this.optionsContainer.setAttribute('role', 'listbox'); this.trigger.setAttribute('aria-controls', this.optionsContainer.id);
    const selected = this.optionsContainer.querySelector<HTMLElement>('.custom-option.selected');
    if (selected) { this._value = selected.dataset.value || ''; this.triggerSpan.textContent = selected.textContent; }
    this.prepareOptions();

    let ignoreOptionClickUntil = 0;

    // Suppress trailing synthetic click on wrapper/trigger when a carried press already opened the menu.
    this.element.addEventListener('click', event => {
      if (!event.isTrusted || performance.now() >= this.ignoreTriggerClickUntil) return;
      if (this.optionsContainer.contains(event.target as Node) && !(event.target as HTMLElement).closest('.custom-option')) {
        event.stopPropagation();
        return;
      }
      if (event.target === this.element || this.trigger.contains(event.target as Node)) {
        event.stopPropagation();
      }
    });

    this.trigger.addEventListener('click', event => {
      event.stopPropagation();
      if (event.isTrusted && performance.now() < this.ignoreTriggerClickUntil) {
        this.ignoreTriggerClickUntil = 0;
        return;
      }
      if (this.disabled) return;
      if (this.element.classList.contains('open')) this.close(); else this.open();
    });

    // FlClash carried press: mouse opens immediately on pointerdown; touch opens after a 140ms hold
    // unless scrolled (> 8px), then tracks sliding over options until release.
    this.trigger.addEventListener('pointerdown', event => {
      if (!isBasicTheme() || this.disabled || !event.isPrimary) return;
      if (event.pointerType === 'mouse' && event.button !== 0) return;
      if (this.element.classList.contains('open')) return;

      const pointerId = event.pointerId;
      const startX = event.clientX;
      const startY = event.clientY;
      let prevY = startY;
      let activeCarried = false;
      let moved = false;

      const startCarriedSession = () => {
        activeCarried = true;
        this.elasticAnchorY = null;
        this.elasticBoundaryMode = null;
        this.ignoreTriggerClickUntil = performance.now() + 500;
        if (!this.element.classList.contains('open')) this.open(false);
        this.trigger.focus({ preventScroll: true });
      };

      const onWindowMove = (moveEv: PointerEvent) => {
        if (moveEv.pointerId !== pointerId) return;
        const dist = Math.hypot(moveEv.clientX - startX, moveEv.clientY - startY);
        const stepDy = moveEv.clientY - prevY;
        prevY = moveEv.clientY;
        if (!activeCarried) {
          if (dist > 8) cleanupWindow();
          return;
        }
        if (!moved) {
          if (dist <= 6) return;
          moved = true;
        }
        if (!this.element.classList.contains('open')) return;
        const hit = document.elementFromPoint(moveEv.clientX, moveEv.clientY)?.closest<HTMLElement>('.custom-option');
        const hovered = hit && this.optionsContainer.contains(hit) && hit.getAttribute('aria-disabled') !== 'true' ? hit : null;
        this.setPressHover(hovered);
        this.updateElasticFromPointer(moveEv.clientX, moveEv.clientY, startX, startY, stepDy);
      };

      const onWindowEnd = (endEv: PointerEvent) => {
        if (endEv.pointerId !== pointerId) return;
        const shouldCommit = activeCarried && moved && endEv.type === 'pointerup';
        const picked = shouldCommit ? this.pressHoveredOption : null;
        cleanupWindow();
        if (activeCarried) {
          this.ignoreTriggerClickUntil = performance.now() + 180;
          if (this.element.classList.contains('open')) this.springElasticBack();
        }
        if (picked) {
          ignoreOptionClickUntil = performance.now() + 350;
          picked.click();
        }
      };

      const cleanupWindow = () => {
        if (this.triggerHoldTimer !== null) {
          window.clearTimeout(this.triggerHoldTimer);
          this.triggerHoldTimer = null;
        }
        window.removeEventListener('pointermove', onWindowMove);
        window.removeEventListener('pointerup', onWindowEnd);
        window.removeEventListener('pointercancel', onWindowEnd);
        this.setPressHover(null);
      };

      window.addEventListener('pointermove', onWindowMove);
      window.addEventListener('pointerup', onWindowEnd);
      window.addEventListener('pointercancel', onWindowEnd);

      if (event.pointerType === 'mouse') {
        startCarriedSession();
      } else {
        this.triggerHoldTimer = window.setTimeout(() => {
          this.triggerHoldTimer = null;
          startCarriedSession();
        }, 140);
      }
    });

    this.trigger.addEventListener('keydown', event => {
      if (this.disabled) return;
      if (event.key === 'Escape' || event.key === 'Tab') {
        if (event.key === 'Escape' && this.element.classList.contains('open')) { event.preventDefault(); event.stopPropagation(); }
        this.close(); return;
      }
      if (!['Enter', ' ', 'ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      if (!this.element.classList.contains('open')) {
        this.open(['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key));
        if (event.key === 'Home' || event.key === 'End') this.highlight(event.key === 'Home' ? 0 : this.options().length - 1);
        return;
      }
      if (event.key === 'Enter' || event.key === ' ') { this.options()[this.activeIndex]?.click(); return; }
      this.highlight(event.key === 'Home' ? 0 : event.key === 'End' ? this.options().length - 1 : this.activeIndex + (event.key === 'ArrowUp' ? -1 : 1));
    });
    this.trigger.addEventListener('blur', event => {
      if (!this.element.contains(event.relatedTarget as Node | null)) this.close();
    });
    this.optionsContainer.addEventListener('mousedown', event => {
      if ((event.target as HTMLElement).closest('.custom-option')) event.preventDefault();
    });
    this.optionsContainer.addEventListener('scroll', () => {
      if (this.element.classList.contains('open')) this.syncScrollMask();
    }, { passive: true });

    let pressPointerId: number | null = null;
    let pressPointerType = 'mouse';
    let pressStartX = 0;
    let pressStartY = 0;
    let pressStartScrollTop = 0;
    let pressDownOption: HTMLElement | null = null;
    let pressScrollingList = false;
    let pressSlid = false;
    const onListMove = (event: PointerEvent) => {
      if (event.pointerId !== pressPointerId) return;
      const stepDy = event.clientY - this.lastPointerY;
      this.lastPointerY = event.clientY;
      const scrollable = this.isScrollableList && this.optionsContainer.scrollHeight > this.optionsContainer.clientHeight + 2;
      if (
        (scrollable && event.pointerType !== 'mouse' && Math.abs(event.clientY - pressStartY) > 8)
        || Math.abs(this.optionsContainer.scrollTop - pressStartScrollTop) > 2
      ) {
        pressScrollingList = true;
        this.setPressHover(null);
        if (pressPointerType === 'mouse') {
          this.updateElasticFromPointer(event.clientX, event.clientY, pressStartX, pressStartY, stepDy);
        }
        return;
      }
      if (pressScrollingList) {
        if (pressPointerType === 'mouse') {
          this.updateElasticFromPointer(event.clientX, event.clientY, pressStartX, pressStartY, stepDy);
        }
        return;
      }
      if (pressPointerType === 'mouse') {
        this.updateElasticFromPointer(event.clientX, event.clientY, pressStartX, pressStartY, stepDy);
      }
      const hit = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('.custom-option');
      const hovered = hit && this.optionsContainer.contains(hit) && hit.getAttribute('aria-disabled') !== 'true' ? hit : null;
      if (hovered && hovered !== pressDownOption) pressSlid = true;
      this.setPressHover(hovered);
    };
    const clearPressTracking = () => {
      if (pressPointerId !== null) {
        window.removeEventListener('pointermove', onListMove);
        window.removeEventListener('pointerup', onListUp);
        window.removeEventListener('pointercancel', onListCancel);
      }
      pressPointerId = null;
      pressDownOption = null;
      pressScrollingList = false;
      pressSlid = false;
      this.setPressHover(null);
      if (pressPointerType === 'mouse' && isBasicTheme() && this.element.classList.contains('open')) {
        this.springElasticBack();
      }
    };
    const onListUp = (event: PointerEvent) => {
      if (event.pointerId !== pressPointerId) return;
      const moved = Math.hypot(event.clientX - pressStartX, event.clientY - pressStartY) > 6;
      const releasedOutside = !this.optionsContainer.contains(event.target as Node | null);
      const picked = !pressScrollingList && pressSlid ? this.pressHoveredOption : null;
      clearPressTracking();
      if (moved && releasedOutside) {
        CustomSelect.ignoreOutsideClickUntil = performance.now() + 180;
        ignoreOptionClickUntil = performance.now() + 180;
        return;
      }
      if (picked) {
        ignoreOptionClickUntil = performance.now() + 350;
        picked.click();
      }
    };
    const onListCancel = (event: PointerEvent) => {
      if (event.pointerId === pressPointerId) clearPressTracking();
    };
    this.optionsContainer.addEventListener('pointerdown', event => {
      if (!event.isPrimary || (event.pointerType === 'mouse' && event.button !== 0)) return;
      if (pressPointerId !== null) clearPressTracking();
      pressPointerId = event.pointerId;
      pressPointerType = event.pointerType;
      pressStartX = event.clientX;
      pressStartY = event.clientY;
      this.lastPointerY = event.clientY;
      this.elasticAnchorY = null;
      this.elasticBoundaryMode = null;
      pressStartScrollTop = this.optionsContainer.scrollTop;
      pressScrollingList = false;
      pressSlid = false;
      pressDownOption = (event.target as HTMLElement).closest<HTMLElement>('.custom-option');
      if (pressDownOption && pressDownOption.getAttribute('aria-disabled') !== 'true') {
        this.setPressHover(pressDownOption);
      }
      if (isBasicTheme() && this.morphMode === null && this.elasticFrame !== null) {
        cancelAnimationFrame(this.elasticFrame);
        this.elasticFrame = null;
      }
      window.addEventListener('pointermove', onListMove);
      window.addEventListener('pointerup', onListUp);
      window.addEventListener('pointercancel', onListCancel);
    });

    // Dedicated touch elastic tracking: Android cancels pointermove during pan-y scrolls,
    // so touchstart/touchmove lets both scrollable-boundary and edge-exit stretches engage cleanly.
    const findTouchById = (list: TouchList, id: number): Touch | null => {
      for (let i = 0; i < list.length; i++) {
        if (list[i].identifier === id) return list[i];
      }
      return null;
    };
    this.optionsContainer.addEventListener('touchstart', event => {
      if (!isBasicTheme() || !this.element.classList.contains('open') || this.morphMode !== null) return;
      if (event.touches.length !== 1) return;
      const touch = event.touches[0];
      this.touchElasticId = touch.identifier;
      this.touchStartX = touch.clientX;
      this.touchStartY = touch.clientY;
      this.touchLastY = touch.clientY;
      this.touchAnchorY = null;
      this.touchElasticActive = false;
      this.touchBoundaryMode = null;
      this.elasticAnchorY = null;
      this.elasticBoundaryMode = null;
      if (this.elasticFrame !== null) {
        cancelAnimationFrame(this.elasticFrame);
        this.elasticFrame = null;
      }
    }, { passive: true });

    this.optionsContainer.addEventListener('touchmove', event => {
      if (this.touchElasticId === null || !isBasicTheme() || !this.element.classList.contains('open') || this.morphMode !== null || !this.layoutMetrics) return;
      const touch = findTouchById(event.touches, this.touchElasticId);
      if (!touch) return;
      const stepDy = touch.clientY - this.touchLastY;
      this.touchLastY = touch.clientY;
      const { cardLeft, cardTop, cardWidth, cardHeight } = this.layoutMetrics;
      const el = this.optionsContainer;
      const maxScroll = this.isScrollableList ? Math.max(0, el.scrollHeight - el.clientHeight) : 0;
      const outsideX = touch.clientX < cardLeft || touch.clientX > cardLeft + cardWidth;
      const outsideY = touch.clientY < cardTop || touch.clientY > cardTop + cardHeight;
      const atTop = el.scrollTop <= 0.5 && stepDy > 0;
      const atBottom = el.scrollTop >= maxScroll - 0.5 && stepDy < 0;

      if (!this.touchElasticActive) {
        if (maxScroll <= 1 || outsideX || outsideY || atTop || atBottom) {
          this.touchElasticActive = true;
          this.touchBoundaryMode = (maxScroll <= 1 || outsideX || outsideY) ? 'edge' : atTop ? 'top' : 'bottom';
          this.touchAnchorY = touch.clientY < cardTop ? cardTop : touch.clientY > cardTop + cardHeight ? cardTop + cardHeight : touch.clientY;
          this.elasticAnchorY = this.touchAnchorY;
          this.elasticBoundaryMode = this.touchBoundaryMode;
        }
      }

      if (!this.touchElasticActive) return;

      if (maxScroll > 1 && !outsideX && !outsideY && this.touchAnchorY !== null) {
        const relY = touch.clientY - this.touchAnchorY;
        if ((this.touchBoundaryMode === 'top' && relY < 0) || (this.touchBoundaryMode === 'bottom' && relY > 0)) {
          this.touchElasticActive = false;
          this.touchAnchorY = null;
          this.touchBoundaryMode = null;
          this.elasticAnchorY = null;
          this.elasticBoundaryMode = null;
          this.applyElasticTransform(0, 0, 1, 1);
          return;
        }
      }

      if (event.cancelable) event.preventDefault();
      this.updateElasticFromPointer(touch.clientX, touch.clientY, this.touchStartX, this.touchStartY, stepDy);
    }, { passive: false });

    const finishTouchElastic = (event: TouchEvent) => {
      if (this.touchElasticId === null) return;
      if (findTouchById(event.touches, this.touchElasticId)) return;
      const wasActive = this.touchElasticActive;
      this.touchElasticId = null;
      this.touchAnchorY = null;
      this.touchBoundaryMode = null;
      this.touchElasticActive = false;
      if (wasActive && isBasicTheme() && this.element.classList.contains('open')) {
        this.springElasticBack();
      }
    };
    this.optionsContainer.addEventListener('touchend', finishTouchElastic, { passive: true });
    this.optionsContainer.addEventListener('touchcancel', finishTouchElastic, { passive: true });

    // Consume wheel momentum inside the list, including at its limits. Letting
    // it reach the page used to scroll an ancestor and close the flyout.
    this.optionsContainer.addEventListener('wheel', event => {
      if (!this.element.classList.contains('open') || !event.deltaY) return;
      event.preventDefault(); event.stopPropagation();
      const unit = event.deltaMode === 1 ? 20 : event.deltaMode === 2 ? this.optionsContainer.clientHeight : 1;
      const delta = event.deltaY * unit;
      const prevTop = this.optionsContainer.scrollTop;
      const maxScroll = this.isScrollableList ? Math.max(0, this.optionsContainer.scrollHeight - this.optionsContainer.clientHeight) : 0;
      if (this.isScrollableList) {
        this.optionsContainer.scrollTop += delta;
        this.syncScrollMask();
      }
      if (isBasicTheme() && this.morphMode === null) {
        const atTop = prevTop <= 0.5 && delta < 0;
        const atBottom = prevTop >= maxScroll - 0.5 && delta > 0;
        if (atTop || atBottom || maxScroll <= 1) {
          this.wheelPullY = clamp(this.wheelPullY - delta * 0.45, -72, 72);
          const pullY = rubberBand(this.wheelPullY, 14);
          const stretchY = 1 + Math.min(0.025, Math.abs(pullY) / 420);
          this.applyElasticTransform(0, pullY, 1 / Math.sqrt(stretchY), stretchY);
          if (this.wheelBounceTimer !== null) window.clearTimeout(this.wheelBounceTimer);
          this.wheelBounceTimer = window.setTimeout(() => {
            this.wheelBounceTimer = null;
            this.wheelPullY = 0;
            this.springElasticBack();
          }, 90);
        }
      }
    }, {passive: false});
    this.optionsContainer.addEventListener('click', event => {
      if (event.isTrusted && performance.now() < ignoreOptionClickUntil) {
        ignoreOptionClickUntil = 0;
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      const option = (event.target as HTMLElement).closest<HTMLElement>('.custom-option');
      if (this.disabled || !option || option.getAttribute('aria-disabled') === 'true') return;
      const value = option.dataset.value || '';
      this.setValue(value);
      this.close();
      this.trigger.focus({ preventScroll: true });
      this.onChangeCallbacks.forEach(callback => callback(value));
      // If callback switched theme away from Basic mid-close (e.g. setting-theme), snap cleanup cleanly.
      if (this.morphMode !== null && !isBasicTheme()) this.hideOptions();
    });
    if (!CustomSelect.outsideHandlerInstalled) {
      document.addEventListener('click', event => {
        if (event.isTrusted && performance.now() < CustomSelect.ignoreOutsideClickUntil) {
          CustomSelect.ignoreOutsideClickUntil = 0;
          return;
        }
        CustomSelect.closeOpen();
      });
      const schedulePlacement = CustomSelect.schedulePlacement;
      if (typeof ResizeObserver !== 'undefined') CustomSelect.layoutObserver = new ResizeObserver(schedulePlacement);
      window.addEventListener('resize', schedulePlacement);
      window.visualViewport?.addEventListener('resize', schedulePlacement);
      window.visualViewport?.addEventListener('scroll', schedulePlacement);
      document.addEventListener('scroll', event => {
        const current = CustomSelect.openInstance;
        if (current && event.target instanceof Element && !current.optionsContainer.contains(event.target)
          && event.target.contains(current.element)) {
          const before = current.scrollPositions.get(event.target);
          // Focusing a trigger can queue a scroll event before it opens. Its
          // offset is already captured above; it must not close the new menu.
          if (!before || Math.abs(before.top-event.target.scrollTop) > .5 || Math.abs(before.left-event.target.scrollLeft) > .5) current.close();
        }
      }, {capture:true,passive:true});
      CustomSelect.outsideHandlerInstalled = true;
    }
  }
  get value(): string { return this._value; }
  set value(value: string) { this.setValue(value); }
  setValue(value: string) {
    const selected = this.optionsByValue.get(value) || [];
    for (const option of this.selectedOptions) {
      if (!selected.includes(option)) { option.classList.remove('selected'); option.setAttribute('aria-selected', 'false'); }
    }
    for (const option of selected) {
      if (!this.selectedOptions.includes(option)) { option.classList.add('selected'); option.setAttribute('aria-selected', 'true'); }
    }
    this._value = value;
    this.selectedOptions = selected;
    const text = selected[selected.length - 1]?.textContent || value;
    if (this.triggerSpan.textContent !== text) {
      const canCrossfade = this.hasPaintedValue
        && isBasicTheme()
        && typeof this.triggerSpan.animate === 'function'
        && !prefersReducedMotion();
      this.triggerSpan.textContent = text;
      if (canCrossfade) {
        this.triggerSpan.getAnimations().forEach(anim => anim.cancel());
        const isAndroid = document.body.classList.contains('is-android');
        this.triggerSpan.animate(
          isAndroid
            ? [{ opacity: 0.28, transform: 'translate3d(0, 2px, 0)' }, { opacity: 1, transform: 'translate3d(0, 0, 0)' }]
            : [{ opacity: 0.18, filter: 'blur(3.5px)' }, { opacity: 1, filter: 'blur(0px)' }],
          { duration: isAndroid ? 180 : 210, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' },
        );
      }
    }
    this.hasPaintedValue = true;
  }
  setDisabled(disabled: boolean) {
    this.disabled = disabled; this.element.classList.toggle('is-disabled', disabled);
    this.trigger.setAttribute('aria-disabled', String(disabled)); this.trigger.tabIndex = disabled ? -1 : 0;
    if (disabled) this.close();
  }
  addEventListener(event: 'change', callback: (event: { target: { value: string } }) => void) {
    if (event === 'change') this.onChangeCallbacks.push(value => callback({ target: { value } }));
  }
  setOptions(options: { value: string, text: string }[]) {
    const value = options.some(option => option.value === this._value) ? this._value : options[0]?.value || '';
    // Configuration refreshes often resend an unchanged list. Preserve its DOM,
    // scroll position and open state instead of rebuilding every option.
    if (options.length === this.allOptions.length && options.every((option, i) =>
      option.value === this.allOptions[i].dataset.value && option.text === this.allOptions[i].textContent)) {
      this.setValue(value);
      return;
    }
    this.close();
    const fragment = document.createDocumentFragment();
    options.forEach(option => {
      const element = document.createElement('div'); element.className = 'custom-option';
      element.dataset.value = option.value; element.textContent = option.text; fragment.append(element);
    });
    this.optionsContainer.replaceChildren(fragment);
    this.prepareOptions();
    this.setValue(value);
  }
}
