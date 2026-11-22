import { collectFlowSiblings } from './layout-motion';

/** Expand/fade a status panel using compositor-only clipPath and sibling FLIP animations. */
export class AnimatedVisibility {
  private visible: boolean;
  private animation: Animation | null = null;
  private siblingAnims: Animation[] = [];
  private animFrom = 0;
  private animTo = 0;

  constructor(private readonly element: HTMLElement) {
    this.visible = !element.hidden;
  }

  private cancelActive() {
    if (this.animation) {
      this.animation.onfinish = null;
      this.animation.cancel();
      this.animation = null;
    }
    this.siblingAnims.forEach(a => a.cancel());
    this.siblingAnims = [];
    this.element.classList.remove('progress-animating');
  }

  setVisible(visible: boolean, options?: { autoAnchor?: boolean }) {
    if (visible === this.visible) return;
    this.visible = visible;

    let startFrac = this.element.hidden ? 0 : 1;
    if (this.animation) {
      const rawProgress = this.animation.effect?.getComputedTiming().progress;
      const progress = typeof rawProgress === 'number' && Number.isFinite(rawProgress)
        ? Math.max(0, Math.min(1, rawProgress))
        : 1;
      startFrac = Math.max(0, Math.min(1, this.animFrom + (this.animTo - this.animFrom) * progress));
    }
    this.cancelActive();

    const scroller = this.element.closest<HTMLElement>('#main-content');
    const page = this.element.closest<HTMLElement>('.page.active');

    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches || !this.element.animate) {
      if (options?.autoAnchor && scroller && page) {
        const anchor = collectFlowSiblings(this.element, 'next').find(s => s.getBoundingClientRect().height > 0);
        if (anchor) {
          const anchorTopBefore = anchor.getBoundingClientRect().top;
          const scrollerRect = scroller.getBoundingClientRect();
          const elRect = this.element.getBoundingClientRect();
          const referenceY = !this.element.hidden && elRect.height > 0
            ? elRect.top + elRect.height * 0.5
            : anchorTopBefore;
          this.element.hidden = !visible;
          if (referenceY < scrollerRect.top + scrollerRect.height * 0.5) {
            const diff = anchor.getBoundingClientRect().top - anchorTopBefore;
            if (Math.abs(diff) >= 0.5) scroller.scrollTop = Math.max(0, scroller.scrollTop + diff);
          }
          return;
        }
      }
      this.element.hidden = !visible;
      return;
    }

    const savedScroll = scroller?.scrollTop ?? 0;
    const nextSiblings = collectFlowSiblings(this.element, 'next');
    const prevSiblings = collectFlowSiblings(this.element, 'prev');
    const anchor = nextSiblings.find(s => s.getBoundingClientRect().height > 0) || null;

    let useUpperAnchor = false;
    if (options?.autoAnchor && scroller && page && anchor) {
      const scrollerRect = scroller.getBoundingClientRect();
      const elRect = this.element.getBoundingClientRect();
      const anchorRect = anchor.getBoundingClientRect();
      const referenceY = !this.element.hidden && elRect.height > 0
        ? elRect.top + elRect.height * 0.5
        : anchorRect.top;
      useUpperAnchor = referenceY < scrollerRect.top + scrollerRect.height * 0.5;
    }

    this.element.hidden = true;
    const nextBefore = nextSiblings.map(s => s.getBoundingClientRect().top);
    const prevBefore = prevSiblings.map(s => s.getBoundingClientRect().top);
    const anchorBefore = anchor ? anchor.getBoundingClientRect().top : 0;

    this.element.hidden = false;
    if (scroller && scroller.scrollTop !== savedScroll) scroller.scrollTop = savedScroll;
    const fullHeight = this.element.getBoundingClientRect().height;
    // A panel inside an inactive page has no measurable size; keep its final state.
    if (!(fullHeight > 0)) {
      this.element.hidden = !visible;
      return;
    }

    const nextAfter = nextSiblings.map(s => s.getBoundingClientRect().top);
    const anchorDy = anchor ? anchor.getBoundingClientRect().top - anchorBefore : 0;

    const toFrac = visible ? 1 : 0;
    this.animFrom = startFrac;
    this.animTo = toFrac;
    const timing: KeyframeAnimationOptions = {
      duration: visible ? 220 : 180,
      easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)',
      fill: 'both',
    };
    const radius = getComputedStyle(this.element).borderTopLeftRadius || '0px';

    if (useUpperAnchor && scroller && anchorDy > 0.5) {
      this.element.classList.add('progress-animating');
      if (visible) {
        const previousOverflowAnchor = scroller.style.overflowAnchor;
        scroller.style.overflowAnchor = 'none';
        const scrollBeforeAdjust = scroller.scrollTop;
        scroller.scrollTop = Math.max(0, savedScroll + anchorDy);
        const actualDelta = scroller.scrollTop - scrollBeforeAdjust;
        const prevAfterScroll = prevSiblings.map(s => s.getBoundingClientRect().top);
        scroller.style.overflowAnchor = previousOverflowAnchor;

        const animation = this.element.animate(
          [
            {
              clipPath: `inset(${((1 - startFrac) * fullHeight).toFixed(1)}px 0px 0px 0px round ${radius})`,
              opacity: startFrac,
            },
            {
              clipPath: `inset(0px 0px 0px 0px round ${radius})`,
              opacity: 1,
            },
          ],
          timing,
        );
        const siblingAnims: Animation[] = [];
        for (let i = 0; i < prevSiblings.length; i++) {
          const shift = (prevBefore[i] - prevAfterScroll[i]) || actualDelta;
          if (Math.abs(shift) < 0.5 || !prevSiblings[i].animate) continue;
          siblingAnims.push(prevSiblings[i].animate(
            [
              { transform: `translate3d(0, ${((1 - startFrac) * shift).toFixed(2)}px, 0)` },
              { transform: 'translate3d(0, 0, 0)' },
            ],
            timing,
          ));
        }
        this.animation = animation;
        this.siblingAnims = siblingAnims;
        animation.onfinish = () => {
          if (this.animation !== animation) return;
          this.element.hidden = !this.visible;
          this.cancelActive();
        };
        return;
      }

      const animation = this.element.animate(
        [
          {
            clipPath: `inset(${((1 - startFrac) * fullHeight).toFixed(1)}px 0px 0px 0px round ${radius})`,
            opacity: startFrac,
          },
          {
            clipPath: `inset(${fullHeight.toFixed(1)}px 0px 0px 0px round ${radius})`,
            opacity: 0,
          },
        ],
        timing,
      );
      const siblingAnims: Animation[] = [];
      for (let i = 0; i < prevSiblings.length; i++) {
        if (!prevSiblings[i].animate) continue;
        siblingAnims.push(prevSiblings[i].animate(
          [
            { transform: `translate3d(0, ${((1 - startFrac) * anchorDy).toFixed(2)}px, 0)` },
            { transform: `translate3d(0, ${anchorDy.toFixed(2)}px, 0)` },
          ],
          timing,
        ));
      }
      this.animation = animation;
      this.siblingAnims = siblingAnims;
      animation.onfinish = () => {
        if (this.animation !== animation) return;
        const previousOverflowAnchor = scroller.style.overflowAnchor;
        scroller.style.overflowAnchor = 'none';
        const anchorTopBeforeHide = anchor?.isConnected ? anchor.getBoundingClientRect().top : null;
        this.element.hidden = !this.visible;
        if (anchorTopBeforeHide !== null && anchor?.isConnected) {
          const diff = anchor.getBoundingClientRect().top - anchorTopBeforeHide;
          if (Math.abs(diff) >= 0.5) scroller.scrollTop = Math.max(0, scroller.scrollTop + diff);
        }
        this.cancelActive();
        scroller.style.overflowAnchor = previousOverflowAnchor;
      };
      return;
    }

    this.element.classList.add('progress-animating');
    const animation = this.element.animate(
      [
        {
          clipPath: `inset(0px 0px ${((1 - startFrac) * fullHeight).toFixed(1)}px 0px round ${radius})`,
          opacity: startFrac,
          transform: `translate3d(0, ${((1 - startFrac) * -4).toFixed(2)}px, 0)`,
        },
        {
          clipPath: `inset(0px 0px ${((1 - toFrac) * fullHeight).toFixed(1)}px 0px round ${radius})`,
          opacity: toFrac,
          transform: `translate3d(0, ${((1 - toFrac) * -4).toFixed(2)}px, 0)`,
        },
      ],
      timing,
    );
    const siblingAnims: Animation[] = [];
    for (let i = 0; i < nextSiblings.length; i++) {
      const dy = nextAfter[i] - nextBefore[i];
      if (Math.abs(dy) < 1 || !nextSiblings[i].animate) continue;
      siblingAnims.push(nextSiblings[i].animate(
        [
          { transform: `translate3d(0, ${(-(1 - startFrac) * dy).toFixed(2)}px, 0)` },
          { transform: `translate3d(0, ${(-(1 - toFrac) * dy).toFixed(2)}px, 0)` },
        ],
        timing,
      ));
    }
    this.animation = animation;
    this.siblingAnims = siblingAnims;
    animation.onfinish = () => {
      if (this.animation !== animation) return;
      this.element.hidden = !this.visible;
      this.cancelActive();
    };
  }
}
