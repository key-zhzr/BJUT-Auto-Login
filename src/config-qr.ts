// QR frames carry only the encrypted backup envelope, never plaintext credentials.
const PREFIX = 'BJUTALQR1';
// Keep symbols below the high QR versions that older WebViews/decoders handle poorly.
const FRAME_SIZE = 600;
const MAX_FRAMES = 128;
const MAX_AGE_MS = 10 * 60_000;
const digest = async (text: string) => Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))))
  .map(byte => byte.toString(16).padStart(2, '0')).join('');

export async function encodeConfigQr(payload: string): Promise<string[]> {
  if (!payload || payload.length > FRAME_SIZE * MAX_FRAMES || /[^\x20-\x7e]/.test(payload)) {
    throw new Error('备份太大，无法生成二维码，请使用剪贴板导出。');
  }
  const id = await digest(payload);
  const count = Math.ceil(payload.length / FRAME_SIZE);
  return Array.from({ length: count }, (_, index) => `${PREFIX}:${id}:${index + 1}:${count}:${payload.slice(index * FRAME_SIZE, (index + 1) * FRAME_SIZE)}`);
}

export class ConfigQrCollector {
  private id = '';
  private total = 0;
  private started = 0;
  private frames = new Map<number, string>();
  private readonly now: () => number;
  constructor(now = () => Date.now()) { this.now = now; }
  async add(frame: string): Promise<{ received: number; total: number; payload?: string }> {
    if (frame.length > FRAME_SIZE + 100) throw new Error('二维码内容过大。');
    const match = /^BJUTALQR1:([a-f0-9]{64}):([1-9]\d{0,2}):([1-9]\d{0,2}):([\x20-\x7e]+)$/.exec(frame);
    if (!match) throw new Error('这不是配置二维码。');
    const [, id, part, count, data] = match;
    const index = Number(part), total = Number(count);
    if (total > MAX_FRAMES || index > total || data.length > FRAME_SIZE) throw new Error('二维码格式无效。');
    if (this.started && this.now() - this.started > MAX_AGE_MS) throw new Error('扫描已超时，请关闭后重新扫描。');
    if (this.id && (id !== this.id || total !== this.total)) throw new Error('请继续扫描同一份备份的二维码。');
    if (this.frames.has(index) && this.frames.get(index) !== data) throw new Error('二维码内容冲突，请重新扫描。');
    if (!this.id) { this.id = id; this.total = total; this.started = this.now(); }
    this.frames.set(index, data);
    const progress = { received: this.frames.size, total };
    if (this.frames.size !== total) return progress;
    const payload = Array.from({ length: total }, (_, i) => this.frames.get(i + 1)!).join('');
    if (await digest(payload) !== id) throw new Error('二维码校验失败，请重新导出。');
    if (!/^BJUT4:[A-Za-z0-9_-]{59,}$/.test(payload)) {
      const envelope = JSON.parse(payload) as { version?: unknown; ciphertext?: unknown };
      if (envelope.version !== 3 || typeof envelope.ciphertext !== 'string') throw new Error('备份格式不受支持。');
    }
    return { ...progress, payload };
  }
}
