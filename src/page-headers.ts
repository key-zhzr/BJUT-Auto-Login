/** Extend sticky header backgrounds to the scrollport edges without nesting
 * another scroller or changing the pages' content/shadow space. */
export function setupPageHeaders() {
  const main = document.getElementById('main-content');
  if (!main) return;
  const headers = Array.from(main.querySelectorAll<HTMLElement>(':scope > .page > .page-header'));
  let frame: number | null = null;
  const update = () => {
    frame = null;
    const bounds = main.getBoundingClientRect();
    const top = Number.parseFloat(getComputedStyle(main).paddingTop) || 0;
    for (const header of headers) {
      if (!header.parentElement?.classList.contains('active')) continue;
      const rect = header.getBoundingClientRect();
      header.style.setProperty('--header-left', `${Math.max(0, rect.left - bounds.left)}px`);
      header.style.setProperty('--header-right', `${Math.max(0, bounds.right - rect.right)}px`);
      header.style.setProperty('--header-top', `${top}px`);
    }
  };
  const schedule = () => { if (frame === null) frame = requestAnimationFrame(update); };
  if (typeof ResizeObserver !== 'undefined') {
    const observer = new ResizeObserver(schedule);
    observer.observe(main);
    headers.forEach(header => observer.observe(header));
  }
  window.addEventListener('resize', schedule, {passive:true});
  schedule();
}
