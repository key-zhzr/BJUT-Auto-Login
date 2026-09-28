import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface Prompt { id: string; title: string; image: string; expiresInSeconds: number }

export async function setupImageCaptchas() {
  const queue: Prompt[] = [];
  let active: { id: string; overlay: HTMLElement; input: HTMLInputElement } | null = null;
  const showNext = () => {
    if (active || !queue.length) return;
    const prompt = queue.shift()!;
    const previous = document.activeElement as HTMLElement | null;
    const overlay = document.createElement('div');
    overlay.className = 'modal-overlay hidden image-captcha-overlay';
    overlay.innerHTML = `<form class="modal-content glass-card config-transfer-dialog" role="dialog" aria-modal="true">
      <h3></h3><p class="transfer-note">请填写图片中的验证码。</p>
      <div class="image-captcha-picture"><img alt="验证码"><button type="button" class="btn btn-secondary btn-sm" data-refresh>换一张</button></div>
      <div class="form-group"><label>验证码<input type="text" name="captcha" autocomplete="off" autocapitalize="off" spellcheck="false" maxlength="32" required></label></div>
      <p class="transfer-error" role="alert"></p>
      <div class="modal-actions"><button type="button" class="btn btn-secondary" data-cancel>取消</button><button type="submit" class="btn btn-primary">继续</button></div>
    </form>`;
    overlay.querySelector('h3')!.textContent = prompt.title;
    overlay.querySelector('form')!.setAttribute('aria-label', prompt.title);
    overlay.querySelector('img')!.src = prompt.image;
    const input = overlay.querySelector('input')!;
    const error = overlay.querySelector<HTMLElement>('.transfer-error')!;
    overlay.querySelector('img')!.addEventListener('error', () => { error.textContent = '图片未能显示，请换一张。'; });
    active = { id: prompt.id, overlay, input };
    document.body.append(overlay);
    let busy = false;
    const answer = async (text: string | null, refresh = false) => {
      if (busy) return;
      busy = true;
      overlay.querySelectorAll<HTMLButtonElement>('button').forEach(button => { button.disabled = true; });
      try { await invoke('answer_image_captcha', {id:prompt.id,text,refresh}); }
      catch (reason) {
        error.textContent = String(reason);
        busy = false;
        overlay.querySelectorAll<HTMLButtonElement>('button').forEach(button => { button.disabled = false; });
      }
    };
    overlay.querySelector('form')!.addEventListener('submit', event => { event.preventDefault(); void answer(input.value); });
    overlay.querySelector('[data-refresh]')!.addEventListener('click', () => void answer(null, true));
    overlay.querySelector('[data-cancel]')!.addEventListener('click', () => void answer(null));
    overlay.addEventListener('keydown', event => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); void answer(null); }
    });
    overlay.addEventListener('captcha-closed', () => {
      input.value = '';
      overlay.classList.add('hidden');
      setTimeout(() => {
        overlay.remove();
        if (!active && document.activeElement === document.body && previous?.isConnected) previous.focus({preventScroll:true});
      }, window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 320);
    }, {once:true});
    requestAnimationFrame(() => requestAnimationFrame(() => {
      if (active?.id === prompt.id) { overlay.classList.remove('hidden'); input.focus({preventScroll:true}); }
    }));
  };
  await Promise.all([
    listen<Prompt>('image-captcha', event => {
      const prompt = event.payload;
      if (!/^data:image\/(png|jpeg|gif|webp);base64,/.test(prompt.image) || prompt.image.length > 750000) {
        void invoke('answer_image_captcha', {id:prompt.id,text:null}).catch(() => {});
        return;
      }
      if (active?.id === prompt.id || queue.some(item => item.id === prompt.id)) return;
      queue.push(prompt); showNext();
    }),
    listen<{id:string}>('image-captcha-close', event => {
      const index = queue.findIndex(item => item.id === event.payload.id);
      if (index >= 0) queue.splice(index, 1);
      if (active?.id === event.payload.id) {
        const closed = active; active = null;
        closed.overlay.dispatchEvent(new Event('captcha-closed'));
        showNext();
      }
    }),
  ]);
}
