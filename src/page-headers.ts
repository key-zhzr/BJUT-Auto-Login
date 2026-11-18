/** Extend sticky header backgrounds to the scrollport edges and enable
 * translucent gradient blur once page content begins scrolling underneath. */
export function setupPageHeaders() {
  const main = document.getElementById('main-content');
  if (!main) return;
  const headers = Array.from(main.querySelectorAll<HTMLElement>(':scope > .page > .page-header'));
  let frame: number | null = null;
  const resetHorizontalScroll = () => {
    if (main.scrollLeft !== 0) main.scrollLeft = 0;
    if (document.documentElement.scrollLeft !== 0) document.documentElement.scrollLeft = 0;
    if (document.body.scrollLeft !== 0) document.body.scrollLeft = 0;
  };
  const syncScrollState = () => {
    resetHorizontalScroll();
    main.classList.toggle('is-scrolled', main.scrollTop > 4);
  };
  const update = () => {
    frame = null;
    const bounds = main.getBoundingClientRect();
    const clientRight = bounds.left + main.clientWidth;
    const top = Number.parseFloat(getComputedStyle(main).paddingTop) || 0;
    syncScrollState();
    for (const header of headers) {
      if (!header.parentElement?.classList.contains('active')) continue;
      const rect = header.getBoundingClientRect();
      header.style.setProperty('--header-left', `${Math.max(0, rect.left - bounds.left)}px`);
      header.style.setProperty('--header-right', `${Math.max(0, clientRight - rect.right)}px`);
      header.style.setProperty('--header-top', `${top}px`);
    }
  };
  const schedule = () => { if (frame === null) frame = requestAnimationFrame(update); };
  main.addEventListener('scroll', syncScrollState, { passive: true });
  window.addEventListener('scroll', resetHorizontalScroll, { passive: true });
  if (typeof ResizeObserver !== 'undefined') {
    const observer = new ResizeObserver(schedule);
    observer.observe(main);
    headers.forEach(header => observer.observe(header));
  }
  window.addEventListener('resize', schedule, { passive: true });
  schedule();
}
