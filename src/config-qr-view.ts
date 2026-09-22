import { ConfigQrCollector, encodeConfigQr } from './config-qr';
import { IS_ANDROID } from './platform';

function dialog(title: string) {
  const returnFocus = document.activeElement as HTMLElement | null;
  const overlay = document.createElement('div');
  overlay.className = 'modal-overlay config-qr-overlay';
  overlay.innerHTML = `<section class="modal-content config-qr-dialog" role="dialog" aria-modal="true" aria-labelledby="config-qr-title">
    <h2 id="config-qr-title"></h2><div class="config-qr-body"></div>
    <p class="config-qr-status" role="status"></p><div class="config-qr-actions"></div>
    <button type="button" class="btn btn-secondary config-qr-close">关闭</button></section>`;
  overlay.querySelector('h2')!.textContent = title;
  document.body.append(overlay);
  const closeButton = overlay.querySelector<HTMLButtonElement>('.config-qr-close')!;
  let closeAction = () => {};
  const close = () => { if (!overlay.isConnected) return; closeAction(); overlay.remove(); returnFocus?.focus(); };
  closeButton.onclick = close;
  overlay.addEventListener('keydown', event => { if (event.key === 'Escape') { event.preventDefault(); close(); } });
  closeButton.focus();
  return { overlay, close, onClose: (action: () => void) => { closeAction = action; },
    body: overlay.querySelector<HTMLElement>('.config-qr-body')!,
    actions: overlay.querySelector<HTMLElement>('.config-qr-actions')!,
    status: overlay.querySelector<HTMLElement>('.config-qr-status')! };
}

export async function showConfigQr(payload: string) {
  const frames = await encodeConfigQr(payload);
  const QRCode = await import('qrcode');
  const view = dialog('配置二维码');
  const canvas = document.createElement('canvas');
  canvas.setAttribute('aria-label', '加密配置二维码');
  view.body.append(canvas);
  let index = 0, revision = 0;
  const render = async () => {
    const current = ++revision;
    const buffer = document.createElement('canvas');
    await QRCode.toCanvas(buffer, frames[index], { width: 480, margin: 3, errorCorrectionLevel: 'L' });
    if (current !== revision || !view.overlay.isConnected) return;
    canvas.width = buffer.width; canvas.height = buffer.height;
    canvas.getContext('2d')!.drawImage(buffer, 0, 0);
    view.status.textContent = frames.length > 1
      ? `第 ${index + 1} / ${frames.length} 张 · 请在另一台设备扫描全部二维码`
      : '在另一台设备扫描后，输入备份密码即可导入。';
  };
  if (frames.length > 1) {
    for (const [label, delta] of [['上一张', -1], ['下一张', 1]] as const) {
      const button = document.createElement('button'); button.className = 'btn btn-secondary'; button.textContent = label;
      button.onclick = () => { index = (index + delta + frames.length) % frames.length; void render(); };
      view.actions.append(button);
    }
  }
  view.onClose(() => { revision++; canvas.width = canvas.height = 0; frames.fill(''); });
  await render();
}

export async function scanConfigQr(): Promise<string | null> {
  const view = dialog('导入二维码');
  const collector = new ConfigQrCollector();
  view.status.textContent = '选择二维码图片，或使用相机扫描。';
  return new Promise(resolve => {
    let busy = false, nativeScanning = false, complete = false;
    view.onClose(() => {
      if (nativeScanning) void import('@tauri-apps/plugin-barcode-scanner').then(scanner => scanner.cancel()).catch(() => {});
      if (!complete) resolve(null);
    });
    const accept = async (content: string) => {
      const progress = await collector.add(content);
      if (!view.overlay.isConnected) return;
      view.status.textContent = `已读取 ${progress.received} / ${progress.total} 张`;
      if (progress.payload) { complete = true; resolve(progress.payload); view.close(); }
    };
    const file = document.createElement('input'); file.type = 'file'; file.accept = 'image/*'; file.multiple = true; file.hidden = true;
    const upload = document.createElement('button'); upload.className = 'btn btn-secondary'; upload.textContent = '选择二维码图片'; upload.onclick = () => file.click();
    file.onchange = async () => {
      if (busy) return; busy = true;
      try {
        const { default: jsQR } = await import('jsqr');
        for (const image of Array.from(file.files || [])) {
          if (!view.overlay.isConnected) break;
          if (image.size > 12 * 1024 * 1024) throw new Error('请选择小于 12 MB 的图片。');
          const bitmap = await createImageBitmap(image);
          if (bitmap.width * bitmap.height > 32_000_000) { bitmap.close(); throw new Error('图片尺寸过大。'); }
          const canvas = document.createElement('canvas');
          const scale = Math.min(1, 2000 / Math.max(bitmap.width, bitmap.height));
          canvas.width = Math.round(bitmap.width * scale); canvas.height = Math.round(bitmap.height * scale);
          const context = canvas.getContext('2d', { willReadFrequently: true })!;
          context.drawImage(bitmap, 0, 0, canvas.width, canvas.height); bitmap.close();
          const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
          const code = jsQR(pixels.data, pixels.width, pixels.height);
          canvas.width = canvas.height = 0;
          if (!code) throw new Error('没有识别到二维码，请使用清晰完整的图片。');
          await accept(code.data);
        }
      } catch (error) { view.status.textContent = String(error); }
      finally { busy = false; file.value = ''; }
    };
    view.body.append(file); view.actions.append(upload);
    if (IS_ANDROID) {
      const camera = document.createElement('button'); camera.className = 'btn btn-primary'; camera.textContent = '相机扫描';
      camera.onclick = async () => {
        if (busy) return; busy = true; camera.disabled = true;
        try {
          const scanner = await import('@tauri-apps/plugin-barcode-scanner');
          const permission = await scanner.requestPermissions();
          if (permission !== 'granted') throw new Error('需要允许相机权限，也可选择二维码图片。');
          nativeScanning = true;
          const result = await scanner.scan({ formats: [scanner.Format.QRCode], windowed: false });
          nativeScanning = false;
          if (view.overlay.isConnected) await accept(result.content);
        } catch { if (view.overlay.isConnected) view.status.textContent = '扫描未完成，可重试或选择二维码图片。'; }
        finally { nativeScanning = false; busy = false; camera.disabled = false; }
      };
      view.actions.prepend(camera);
    } else {
      view.status.textContent = '选择二维码图片；多张二维码可一起选择。';
    }
  });
}
