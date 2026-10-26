export function presentationDialog(id: string, title: string) {
    document.getElementById(id)?.remove();
    const previous = document.activeElement as HTMLElement | null;
    const overlay = document.createElement('div');
    overlay.id = id;
    overlay.className = 'modal-overlay hidden';
    const form = document.createElement('form');
    form.className = 'modal-content glass-card presentation-dialog';
    form.setAttribute('role', 'dialog');
    form.setAttribute('aria-modal', 'true');
    form.setAttribute('aria-labelledby', `${id}-title`);
    const heading = document.createElement('h3');
    heading.id = `${id}-title`;
    heading.textContent = title;
    form.append(heading);
    overlay.append(form);
    let closed = false;
    const close = () => { if (closed)
        return; closed = true; document.removeEventListener('keydown', keyboard); form.dispatchEvent(new Event('presentation-close')); overlay.classList.add('hidden'); window.setTimeout(() => {
            // A completed action can reveal the next control before this exit
            // animation ends. Do not steal focus from that control afterwards.
            const restore = overlay.contains(document.activeElement) || document.activeElement === document.body;
            overlay.remove(); if (restore && previous?.isConnected) previous.focus({ preventScroll: true });
        }, window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 300); };
    const keyboard = (event: KeyboardEvent) => {
        const overlays = Array.from(document.querySelectorAll('.modal-overlay:not(.hidden)'));
        if (overlays[overlays.length - 1] !== overlay)
            return;
        if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            close();
        }
        if (event.key === 'Tab') {
            const controls = Array.from(form.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href]')).filter(x => !x.hidden);
            const first = controls[0], last = controls[controls.length - 1];
            if (event.shiftKey && document.activeElement === first) {
                event.preventDefault();
                last?.focus();
            }
            else if (!event.shiftKey && document.activeElement === last) {
                event.preventDefault();
                first?.focus();
            }
        }
    };
    document.addEventListener('keydown', keyboard);
    document.body.append(overlay);
    requestAnimationFrame(() => requestAnimationFrame(() => { if (!closed) {
        overlay.classList.remove('hidden');
        form.querySelector<HTMLElement>('button,input,select')?.focus({ preventScroll: true });
    } }));
    return { form, close };
}
