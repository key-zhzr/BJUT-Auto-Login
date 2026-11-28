export interface HeaderOverflowMenusOptions {
  renderIcons?: (root: ParentNode) => void;
  onLogSearchToggle?: (open: boolean) => void;
  onLogFiltersToggle?: (open: boolean) => void;
}

export interface HeaderOverflowMenusController {
  closeAll: (immediate?: boolean) => void;
  openMenu: (menuId: string, keyboard?: boolean) => void;
  closeMenu: (menuId: string, immediate?: boolean) => void;
  setTriggerDot: (triggerOrId: string | HTMLElement, active: boolean) => void;
  setLogSearchOpen: (open: boolean, focusInput?: boolean) => void;
  setLogFiltersOpen: (open: boolean) => void;
  isLogSearchOpen: () => boolean;
  isLogFiltersOpen: () => boolean;
  syncLogHeaderState: () => void;
  destroy: () => void;
}

const FALLBACK_ICON_PATHS: Readonly<Record<string, string>> = {
  'more-vertical':
    '<circle cx="12" cy="12" r="1"/><circle cx="12" cy="5" r="1"/><circle cx="12" cy="19" r="1"/>',
  filter:
    '<polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>',
  search:
    '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  x:
    '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
};

function ensureFallbackSvgIcons(root: ParentNode, renderIcons?: (root: ParentNode) => void): void {
  renderIcons?.(root);
  for (const placeholder of Array.from(root.querySelectorAll<HTMLElement>('i[data-lucide]'))) {
    const name = placeholder.getAttribute('data-lucide')?.trim() || '';
    const inner = FALLBACK_ICON_PATHS[name];
    if (!inner) continue;
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('width', '16');
    svg.setAttribute('height', '16');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('fill', 'none');
    svg.setAttribute('stroke', 'currentColor');
    svg.setAttribute('stroke-width', '2');
    svg.setAttribute('stroke-linecap', 'round');
    svg.setAttribute('stroke-linejoin', 'round');
    svg.setAttribute('aria-hidden', 'true');
    svg.setAttribute('data-lucide', name);
    svg.setAttribute('data-icon-name', name);
    svg.setAttribute('data-icon-engine', 'lucide');
    svg.setAttribute('class', `lucide lucide-${name}${placeholder.className ? ` ${placeholder.className}` : ''}`);
    svg.innerHTML = inner;
    placeholder.replaceWith(svg);
  }
}

export function setOverflowTriggerActiveDot(triggerOrId: string | HTMLElement, active: boolean): void {
  const trigger = typeof triggerOrId === 'string'
    ? document.getElementById(triggerOrId)
    : triggerOrId;
  if (!trigger) return;
  trigger.classList.toggle('has-active-dot', active);
  const dot = trigger.querySelector<HTMLElement>('.header-action-dot');
  if (dot) dot.hidden = !active;
}

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === 'function'
    && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function supportsPopover(element: HTMLElement): boolean {
  return typeof (element as HTMLElement & { showPopover?: () => void }).showPopover === 'function';
}

class OverflowMenuInstance {
  readonly menu: HTMLElement;
  readonly trigger: HTMLElement;
  readonly id: string;
  private closeTimer: number | null = null;
  private isOpenState = false;
  private readonly usePopover: boolean;

  constructor(
    trigger: HTMLElement,
    menu: HTMLElement,
    private readonly onWillOpen: (instance: OverflowMenuInstance) => void,
  ) {
    this.trigger = trigger;
    this.menu = menu;
    this.id = menu.id;
    this.usePopover = supportsPopover(menu);
    if (this.usePopover && !menu.hasAttribute('popover')) {
      menu.setAttribute('popover', 'manual');
    }
    menu.hidden = true;
    menu.inert = true;
    trigger.setAttribute('aria-expanded', 'false');
  }

  isOpen(): boolean {
    return this.isOpenState;
  }

  private getItems(): HTMLButtonElement[] {
    return Array.from(this.menu.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
      .filter(item => !item.hidden && !item.disabled && item.getAttribute('aria-disabled') !== 'true');
  }

  private clearTimer(): void {
    if (this.closeTimer !== null) {
      window.clearTimeout(this.closeTimer);
      this.closeTimer = null;
    }
  }

  position(): void {
    const triggerRect = this.trigger.getBoundingClientRect();
    if (!triggerRect.width || !triggerRect.height) return;

    const vv = window.visualViewport;
    const vpLeft = vv?.offsetLeft ?? 0;
    const vpTop = vv?.offsetTop ?? 0;
    const vpWidth = vv?.width ?? document.documentElement.clientWidth;
    const vpHeight = vv?.height ?? window.innerHeight;
    const vpRight = vpLeft + vpWidth;
    const vpBottom = vpTop + vpHeight;

    const pad = 8;
    const gap = document.documentElement.dataset.theme === 'winui' ? 4 : 6;
    const menuWidth = Math.max(168, this.menu.offsetWidth || 196);
    const menuHeight = Math.max(44, this.menu.offsetHeight || 120);

    const maxLeft = Math.max(vpLeft + pad, vpRight - pad - menuWidth);
    const left = Math.round(clamp(triggerRect.right - menuWidth, vpLeft + pad, maxLeft));

    const spaceBelow = vpBottom - pad - triggerRect.bottom - gap;
    const spaceAbove = triggerRect.top - gap - (vpTop + pad);
    const opensUp = spaceBelow < menuHeight && spaceAbove > spaceBelow;

    const rawTop = opensUp
      ? triggerRect.top - gap - menuHeight
      : triggerRect.bottom + gap;
    const maxTop = Math.max(vpTop + pad, vpBottom - pad - menuHeight);
    const top = Math.round(clamp(rawTop, vpTop + pad, maxTop));

    this.menu.classList.toggle('opens-up', opensUp);
    this.menu.style.left = `${left}px`;
    this.menu.style.top = `${top}px`;
    this.menu.style.right = 'auto';
    this.menu.style.bottom = 'auto';
  }

  open(focusMode: 'first' | 'last' | 'none' = 'none'): void {
    if (this.trigger.hasAttribute('disabled') || this.trigger.getAttribute('aria-disabled') === 'true') {
      return;
    }
    this.onWillOpen(this);
    this.clearTimer();
    this.isOpenState = true;
    this.menu.classList.remove('is-closing');
    this.menu.hidden = false;
    this.menu.inert = false;

    if (this.usePopover && !this.menu.matches(':popover-open')) {
      try {
        (this.menu as HTMLElement & { showPopover: () => void }).showPopover();
      } catch {
        // Fallback to fixed positioning when Top Layer popover cannot open.
      }
    }

    this.position();
    void this.menu.offsetWidth;
    this.menu.classList.add('is-open');
    this.trigger.classList.add('is-open');
    this.trigger.setAttribute('aria-expanded', 'true');

    if (focusMode !== 'none') {
      const items = this.getItems();
      const target = focusMode === 'last' ? items[items.length - 1] : items[0];
      target?.focus({ preventScroll: true });
    }
  }

  close(options: { immediate?: boolean; restoreFocus?: boolean } = {}): void {
    if (!this.isOpenState && this.menu.hidden) return;
    this.clearTimer();
    this.isOpenState = false;
    this.trigger.classList.remove('is-open');
    this.trigger.setAttribute('aria-expanded', 'false');
    this.menu.classList.remove('is-open');
    this.menu.inert = true;

    const finalize = () => {
      this.closeTimer = null;
      this.menu.classList.remove('is-closing');
      this.menu.hidden = true;
      if (this.usePopover && this.menu.matches(':popover-open')) {
        try {
          (this.menu as HTMLElement & { hidePopover: () => void }).hidePopover();
        } catch {
          // Ignore if already dismissed.
        }
      }
    };

    if (options.restoreFocus && this.trigger.isConnected) {
      this.trigger.focus({ preventScroll: true });
    }

    if (options.immediate || prefersReducedMotion()) {
      finalize();
      return;
    }

    this.menu.classList.add('is-closing');
    this.closeTimer = window.setTimeout(finalize, 180);
  }

  handleMenuKeydown(event: KeyboardEvent): void {
    const items = this.getItems();
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      this.close({ restoreFocus: true });
      return;
    }
    if (event.key === 'Tab') {
      this.close({ immediate: true });
      return;
    }
    if (!items.length) return;
    const currentIndex = items.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      items[(currentIndex + 1 + items.length) % items.length].focus({ preventScroll: true });
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      items[(currentIndex - 1 + items.length) % items.length].focus({ preventScroll: true });
    } else if (event.key === 'Home') {
      event.preventDefault();
      items[0].focus({ preventScroll: true });
    } else if (event.key === 'End') {
      event.preventDefault();
      items[items.length - 1].focus({ preventScroll: true });
    }
  }
}

let activeController: HeaderOverflowMenusController | null = null;

export function setupHeaderOverflowMenus(options: HeaderOverflowMenusOptions = {}): HeaderOverflowMenusController {
  if (activeController) {
    activeController.syncLogHeaderState();
    return activeController;
  }

  const abortController = new AbortController();
  const { signal } = abortController;

  const instances = new Map<string, OverflowMenuInstance>();
  const closeAll = (immediate = false) => {
    for (const instance of instances.values()) {
      if (instance.isOpen()) instance.close({ immediate });
    }
  };

  const anchors = Array.from(document.querySelectorAll<HTMLElement>('.header-overflow-anchor'));
  for (const anchor of anchors) {
    const trigger = anchor.querySelector<HTMLElement>('.header-overflow-trigger');
    const menu = anchor.querySelector<HTMLElement>('.header-overflow-menu');
    if (!trigger || !menu || !menu.id) continue;

    ensureFallbackSvgIcons(anchor, options.renderIcons);

    const instance = new OverflowMenuInstance(trigger, menu, opening => {
      for (const other of instances.values()) {
        if (other !== opening && other.isOpen()) other.close({ immediate: true });
      }
    });
    instances.set(menu.id, instance);

    trigger.addEventListener('click', event => {
      event.stopPropagation();
      if (instance.isOpen()) {
        instance.close();
      } else {
        instance.open('none');
      }
    }, { signal });

    trigger.addEventListener('keydown', event => {
      if (event.key === 'ArrowDown' || event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        event.stopPropagation();
        instance.open('first');
      } else if (event.key === 'ArrowUp') {
        event.preventDefault();
        event.stopPropagation();
        instance.open('last');
      } else if (event.key === 'Escape' && instance.isOpen()) {
        event.preventDefault();
        event.stopPropagation();
        instance.close({ restoreFocus: true });
      }
    }, { signal });

    menu.addEventListener('keydown', event => {
      instance.handleMenuKeydown(event);
    }, { signal });

    menu.addEventListener('click', event => {
      const target = event.target instanceof Element ? event.target : null;
      const item = target?.closest<HTMLButtonElement>('[role="menuitem"]');
      if (!item || item.disabled || item.getAttribute('aria-disabled') === 'true') return;
      instance.close({ immediate: true });
    }, { signal });
  }

  document.addEventListener('pointerdown', event => {
    const target = event.target instanceof Node ? event.target : null;
    for (const instance of instances.values()) {
      if (!instance.isOpen()) continue;
      if (target && (instance.menu.contains(target) || instance.trigger.contains(target))) continue;
      instance.close();
    }
  }, { capture: true, signal });

  window.addEventListener('resize', () => {
    for (const instance of instances.values()) {
      if (instance.isOpen()) instance.position();
    }
  }, { passive: true, signal });

  window.visualViewport?.addEventListener('resize', () => {
    for (const instance of instances.values()) {
      if (instance.isOpen()) instance.position();
    }
  }, { passive: true, signal });

  window.addEventListener('blur', () => closeAll(true), { signal });
  document.getElementById('main-content')?.addEventListener('scroll', () => closeAll(true), { passive: true, signal });

  // Log search & filter drawer state management
  const drawer = document.getElementById('log-filters-drawer');
  const searchBar = document.getElementById('log-search-bar');
  const filterControls = document.getElementById('log-filter-controls');
  const btnToggleSearch = document.getElementById('btn-toggle-log-search');
  const btnToggleFilters = document.getElementById('btn-toggle-log-filters');
  const btnLogsMore = document.getElementById('btn-logs-more');
  const filterSummaryBadge = document.getElementById('logs-filter-summary-badge');
  const logSearchInput = document.getElementById('log-search') as HTMLInputElement | null;
  const btnClearSearch = document.getElementById('btn-clear-log-search');
  const sessionFilterEl = document.getElementById('log-session-filter');
  const levelFilterEl = document.getElementById('log-level-filter');

  if (drawer) ensureFallbackSvgIcons(drawer, options.renderIcons);
  if (btnToggleSearch) ensureFallbackSvgIcons(btnToggleSearch, options.renderIcons);

  let searchOpen = searchBar ? !searchBar.hidden : false;
  let filtersOpen = filterControls ? !filterControls.hidden : false;

  const readSelectedValue = (root: HTMLElement | null, fallback = 'all') =>
    root?.querySelector<HTMLElement>('.custom-option.selected')?.dataset.value || fallback;

  const syncLogHeaderState = () => {
    const query = logSearchInput?.value.trim() || '';
    const sessionValue = readSelectedValue(sessionFilterEl, 'all');
    const levelValue = readSelectedValue(levelFilterEl, 'all');
    const activeFiltersCount = (sessionValue !== 'all' ? 1 : 0) + (levelValue !== 'all' ? 1 : 0);

    if (btnToggleSearch) {
      btnToggleSearch.setAttribute('aria-expanded', String(searchOpen));
      btnToggleSearch.classList.toggle('is-active', searchOpen || query.length > 0);
      setOverflowTriggerActiveDot(btnToggleSearch, query.length > 0);
    }

    if (btnToggleFilters) {
      btnToggleFilters.setAttribute('aria-expanded', String(filtersOpen));
      btnToggleFilters.classList.toggle('is-active', filtersOpen || activeFiltersCount > 0);
    }

    if (filterSummaryBadge) {
      if (activeFiltersCount > 0) {
        filterSummaryBadge.textContent = `${activeFiltersCount} 项已选`;
        filterSummaryBadge.hidden = false;
      } else if (filtersOpen) {
        filterSummaryBadge.textContent = '已展开';
        filterSummaryBadge.hidden = false;
      } else {
        filterSummaryBadge.textContent = '';
        filterSummaryBadge.hidden = true;
      }
    }

    if (btnLogsMore) {
      setOverflowTriggerActiveDot(btnLogsMore, activeFiltersCount > 0);
    }

    if (btnClearSearch) {
      btnClearSearch.hidden = query.length === 0;
    }

    if (searchBar) searchBar.hidden = !searchOpen;
    if (filterControls) filterControls.hidden = !filtersOpen;
    if (drawer) drawer.hidden = !searchOpen && !filtersOpen;
  };

  const setLogSearchOpen = (open: boolean, focusInput = false) => {
    searchOpen = open;
    syncLogHeaderState();
    options.onLogSearchToggle?.(open);
    if (open && focusInput && logSearchInput) {
      requestAnimationFrame(() => logSearchInput.focus({ preventScroll: true }));
    }
  };

  const setLogFiltersOpen = (open: boolean) => {
    filtersOpen = open;
    syncLogHeaderState();
    options.onLogFiltersToggle?.(open);
  };

  btnToggleSearch?.addEventListener('click', () => {
    setLogSearchOpen(!searchOpen, !searchOpen);
  }, { signal });

  btnToggleFilters?.addEventListener('click', () => {
    setLogFiltersOpen(!filtersOpen);
  }, { signal });

  btnClearSearch?.addEventListener('click', () => {
    if (!logSearchInput) return;
    logSearchInput.value = '';
    logSearchInput.dispatchEvent(new Event('input', { bubbles: true }));
    syncLogHeaderState();
    logSearchInput.focus({ preventScroll: true });
  }, { signal });

  logSearchInput?.addEventListener('input', syncLogHeaderState, { signal });

  const filterObserver = typeof MutationObserver === 'undefined' ? null : new MutationObserver(syncLogHeaderState);
  if (filterObserver) {
    if (sessionFilterEl) filterObserver.observe(sessionFilterEl, { subtree: true, attributes: true, attributeFilter: ['class', 'aria-selected'] });
    if (levelFilterEl) filterObserver.observe(levelFilterEl, { subtree: true, attributes: true, attributeFilter: ['class', 'aria-selected'] });
  }

  syncLogHeaderState();

  const controller: HeaderOverflowMenusController = {
    closeAll,
    openMenu: (menuId, keyboard = false) => {
      instances.get(menuId)?.open(keyboard ? 'first' : 'none');
    },
    closeMenu: (menuId, immediate = false) => {
      instances.get(menuId)?.close({ immediate });
    },
    setTriggerDot: setOverflowTriggerActiveDot,
    setLogSearchOpen,
    setLogFiltersOpen,
    isLogSearchOpen: () => searchOpen,
    isLogFiltersOpen: () => filtersOpen,
    syncLogHeaderState,
    destroy: () => {
      closeAll(true);
      filterObserver?.disconnect();
      abortController.abort();
      if (activeController === controller) activeController = null;
    },
  };

  activeController = controller;
  return controller;
}
