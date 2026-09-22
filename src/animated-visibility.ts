/** Expand/fade a small status panel without leaving hidden space in the page. */
export class AnimatedVisibility {
  private visible: boolean;
  private animation: Animation | null = null;

  constructor(private readonly element: HTMLElement) {
    this.visible = !element.hidden;
  }

  setVisible(visible: boolean) {
    if (visible === this.visible) return;
    this.visible = visible;
    const wasHidden = this.element.hidden;
    // Read the interpolated state before cancelling, so a new check can reverse
    // an in-flight exit without jumping or being hidden by its old completion.
    const previous = wasHidden ? null : this.frame();
    if (this.animation) {
      this.animation.onfinish = null;
      this.animation.cancel();
      this.animation = null;
    }
    this.element.classList.remove('progress-animating');
    this.element.hidden = false;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches || !this.element.animate) {
      this.element.hidden = !visible;
      return;
    }
    const expanded = this.frame();
    // A panel inside an inactive page has no measurable size; just keep its
    // final state, rather than animate invisible content or poll for layout.
    if (!(Number.parseFloat(expanded.height) > 0)) {
      this.element.hidden = !visible;
      return;
    }
    const collapsed = { height: '0px', marginTop: '0px', marginBottom: '0px', opacity: '0', transform: 'translateY(-4px)' };
    this.element.classList.add('progress-animating');
    const animation = this.element.animate(
      [previous || collapsed, visible ? expanded : collapsed],
      { duration: visible ? 220 : 180, easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)', fill: 'both' },
    );
    this.animation = animation;
    animation.onfinish = () => {
      if (this.animation !== animation) return;
      this.element.hidden = !this.visible;
      this.element.classList.remove('progress-animating');
      this.animation = null;
      animation.cancel();
    };
  }

  private frame() {
    const style = getComputedStyle(this.element);
    return { height: style.height, marginTop: style.marginTop, marginBottom: style.marginBottom, opacity: style.opacity, transform: style.transform };
  }
}
