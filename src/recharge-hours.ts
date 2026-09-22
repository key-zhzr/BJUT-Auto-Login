export const rechargeServiceIsOpen = (unixMs: number) => {
  const hour = Math.floor((unixMs / 3_600_000 + 8) % 24);
  return hour >= 6 && hour < 23;
};

export class RechargeHours {
  private sample: { unixMs: number; received: number } | null = null;
  private active = false;
  private busy = false;
  private timer: number | null = null;
  private readonly panel = document.querySelector<HTMLElement>('.billing-recharge-hours')!;
  private readonly description = this.panel.querySelector('span')!;

  private readonly load: () => Promise<{unixMs: number}>;
  constructor(load: () => Promise<{unixMs: number}>) {
    this.load = load;
    document.addEventListener('visibilitychange', () => this.schedule());
  }

  setActive(active: boolean) { this.active = active; this.schedule(); }
  private schedule() {
    if (this.timer !== null) window.clearInterval(this.timer);
    this.timer = null;
    if (!this.active || document.hidden) return;
    void this.refresh();
    this.timer = window.setInterval(() => void this.refresh(), 30_000);
  }

  private async refresh() {
    if (this.busy) return;
    this.busy = true;
    try {
      if (!this.sample || performance.now() - this.sample.received >= 300_000) {
        if (!this.sample) this.description.textContent = '正在校准北京时间…';
        const time = await this.load();
        if (!Number.isFinite(time?.unixMs)) throw new Error('Invalid time');
        this.sample = { unixMs: time.unixMs, received: performance.now() };
      }
      const now = this.sample.unixMs + performance.now() - this.sample.received;
      const open = rechargeServiceIsOpen(now);
      this.panel.classList.toggle('is-open', open);
      this.panel.classList.toggle('is-closed', !open);
      const clock = new Intl.DateTimeFormat('zh-CN', { timeZone:'Asia/Shanghai', hour:'2-digit', minute:'2-digit', hourCycle:'h23' }).format(new Date(now));
      this.description.textContent = `北京时间 ${clock} · ${open ? '当前处于开放时段' : '当前不在开放时段，仍可尝试充值'}`;
    } catch {
      this.sample = null;
      this.panel.classList.remove('is-open', 'is-closed');
      this.description.textContent = '暂时无法校时，仍可尝试充值。';
    } finally { this.busy = false; }
  }
}
