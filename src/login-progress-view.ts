import type { LoginProgress } from './models';
const element = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

const phases: Record<string, string> = { safety: '安全检查', checking: '核对网卡', gateway: '确认网关', queued: '等待操作', logout: '注销旧账号', preparing: '准备认证', authenticating: '提交认证', verifying: '验证联网', complete: '完成', stopped: '已停止' };

export class LoginProgressView {
  private id = '';
  private started = 0;
  private timer: number | null = null;
  private cancelled = false;
  private canCancel = false;
  private safetyMs: number | null = null;
  private hideTimer: number | null = null;
  private finished = false;
  private onFinished: (summary: string, success?: boolean) => void;
  private onCancel: (id: string) => Promise<void>;

  constructor(onCancel: (id: string) => Promise<void>, onFinished: (summary: string, success?: boolean) => void = () => {}) {
    this.onCancel = onCancel; this.onFinished = onFinished;
    element('btn-cancel-login').addEventListener('click', async () => {
      if (!this.canCancel) return;
      element<HTMLButtonElement>('btn-cancel-login').disabled = true;
      try {
        await this.onCancel(this.id); this.cancelled = true;
        element('login-progress-message').textContent = '已请求取消，正在停止提交前的准备工作…';
      } catch (error) { element('login-progress-message').textContent = String(error); }
    });
  }

  begin() {
    this.id = crypto.randomUUID(); this.started = performance.now(); this.cancelled = false; this.safetyMs = null; this.finished = false;
    if (this.hideTimer !== null) { window.clearTimeout(this.hideTimer); this.hideTimer = null; }
    element('btn-cancel-login').hidden = false;
    if (this.timer !== null) window.clearInterval(this.timer);
    this.timer = window.setInterval(() => { element('login-progress-time').textContent = `已用时 ${((performance.now() - this.started) / 1000).toFixed(1)} 秒`; }, 100);
    element('login-progress-panel').hidden = false;
    element('login-progress-phases').replaceChildren();
    this.local('安全检查中，可在提交认证前取消');
    return this.id;
  }

  isCancelled() { return this.cancelled; }
  local(message: string) {
    this.canCancel = true;
    element('login-progress-message').textContent = message;
    element<HTMLButtonElement>('btn-cancel-login').disabled = false;
  }

  render(progress: LoginProgress) {
    if (progress.operationId !== this.id) return;
    this.safetyMs ??= Math.max(0, performance.now() - this.started - progress.elapsedMs);
    this.canCancel = progress.canCancel && !this.cancelled;
    element<HTMLButtonElement>('btn-cancel-login').disabled = !this.canCancel;
    element('login-progress-message').textContent = progress.message;
    element('login-progress-phases').textContent = [`安全检查 ${(this.safetyMs / 1000).toFixed(1)} 秒`, ...progress.timings.map(timing => `${phases[timing.phase] || timing.phase} ${(timing.durationMs / 1000).toFixed(1)} 秒`)].join(' · ');
    if (progress.phase === 'complete' || progress.phase === 'stopped') this.finish(progress.message, progress.phase === 'complete');
  }

  finish(message?: string, success?: boolean) {
    if (this.finished) return;
    this.finished = true;
    this.canCancel = false;
    if (this.timer !== null) { window.clearInterval(this.timer); this.timer = null; }
    element<HTMLButtonElement>('btn-cancel-login').disabled = true;
    element('login-progress-time').textContent = `总用时 ${((performance.now() - this.started) / 1000).toFixed(1)} 秒`;
    if (message) element('login-progress-message').textContent = message;
    element('btn-cancel-login').hidden = true;
    this.onFinished([element('login-progress-message').textContent, element('login-progress-time').textContent, element('login-progress-phases').textContent].filter(Boolean).join('；'), success);
    this.hideTimer = window.setTimeout(() => { element('login-progress-panel').hidden = true; this.hideTimer = null; }, 15_000);
  }
}
