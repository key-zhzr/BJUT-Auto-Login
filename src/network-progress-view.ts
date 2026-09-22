import { AnimatedVisibility } from './animated-visibility';

export interface NetworkCheckProgress {
  id: number;
  generation: number;
  revision: number;
  message: string;
  percent: number;
  elapsedMs: number;
  complete: boolean;
}

export class NetworkProgressView {
  private current: NetworkCheckProgress | null = null;
  private updated = 0;
  private displayed = 0;
  private timer: number | null = null;
  private hideTimer: number | null = null;
  private readonly panel = document.getElementById('network-check-progress')!;
  private readonly visibility = new AnimatedVisibility(this.panel);
  private readonly label = document.getElementById('network-check-message')!;
  private readonly elapsed = document.getElementById('network-check-elapsed')!;
  private readonly bar = document.getElementById('network-check-bar')!;

  render(progress: NetworkCheckProgress) {
    if (this.current && (progress.id < this.current.id
      || (progress.id === this.current.id && progress.revision <= this.current.revision))) return;
    if (this.current?.id !== progress.id) this.displayed = 0;
    this.current = progress;
    this.updated = performance.now();
    this.label.textContent = progress.message;
    if (this.timer !== null) window.clearInterval(this.timer);
    if (this.hideTimer !== null) window.clearTimeout(this.hideTimer);
    this.timer = null; this.hideTimer = null;
    this.paint();
    this.visibility.setVisible(true);
    if (progress.complete) {
      this.hideTimer = window.setTimeout(() => { this.visibility.setVisible(false); this.hideTimer = null; }, 4000);
    } else {
      this.timer = window.setInterval(() => this.paint(), 150);
    }
  }

  private paint() {
    if (!this.current) return;
    const delta = this.current.complete ? 0 : performance.now() - this.updated;
    const base = this.current.percent;
    // While I/O is pending show bounded movement, reserving 100% for completion.
    const ceiling = Math.min(96, base + 12);
    const value = this.current.complete ? 100 : base + (ceiling - base) * (1 - Math.exp(-delta / 1800));
    this.displayed = Math.max(this.displayed, value);
    this.bar.style.width = `${this.displayed}%`;
    this.elapsed.textContent = `${((this.current.elapsedMs + delta) / 1000).toFixed(1)} 秒`;
  }
}
