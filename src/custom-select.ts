export class CustomSelect {
  private static openInstance: CustomSelect | null = null;
  private static outsideHandlerInstalled = false;
  private static layoutObserver: ResizeObserver | null = null;
  private static placementFrame: number | null = null;
  element: HTMLElement;
  trigger: HTMLElement;
  triggerSpan: HTMLSpanElement;
  optionsContainer: HTMLElement;
  private _value = '';
  private activeIndex = 0;
  private disabled = false;
  private allOptions: HTMLElement[] = [];
  private optionsByValue = new Map<string, HTMLElement[]>();
  private selectedOptions: HTMLElement[] = [];
  private highlightedOption: HTMLElement | null = null;
  private scrollFrame: number | null = null;
  private closeTimer: number | null = null;
  private scrollPositions = new WeakMap<Element, {top: number; left: number}>();
  onChangeCallbacks: ((value: string) => void)[] = [];

  private options() { return this.allOptions.filter(option => option.getAttribute('aria-disabled') !== 'true'); }
  private usePopover() {
    return typeof (this.optionsContainer as HTMLElement & { showPopover?: () => void }).showPopover === 'function';
  }
  private hideOptions() {
    const popover = this.optionsContainer as HTMLElement & { hidePopover?: () => void };
    if (typeof popover.hidePopover === 'function' && this.optionsContainer.matches(':popover-open')) {
      try { popover.hidePopover(); } catch {}
    }
    this.optionsContainer.hidden = true;
    this.optionsContainer.inert = true;
    this.optionsContainer.style.top = '';
    this.optionsContainer.style.bottom = '';
    this.optionsContainer.style.left = '';
    this.optionsContainer.style.width = '';
  }
  private positionMenu(): boolean {
    const trigger = this.trigger.getBoundingClientRect();
    const viewport = window.visualViewport;
    const left = viewport?.offsetLeft ?? 0;
    const width = viewport?.width ?? document.documentElement.clientWidth;
    const right = left + width;
    const viewportTop = viewport?.offsetTop ?? 0;
    const scroller = this.element.closest<HTMLElement>('main, .modal-content');
    const bounds = scroller?.getBoundingClientRect();
    let top = Math.max(bounds?.top ?? 0, viewportTop);
    let bottom = Math.min(bounds?.bottom ?? window.innerHeight, viewportTop + (viewport?.height ?? window.innerHeight));
    if (scroller?.tagName === 'MAIN') {
      const header = this.element.closest('.page')?.querySelector('.page-header');
      if (header && !header.contains(this.element)) top = Math.max(top, header.getBoundingClientRect().bottom);
      const dock = this.element.closest('#billing-center')?.querySelector('.billing-mobile-nav');
      for (const nav of [document.querySelector('body:not(.is-desktop) #nav'), dock]) {
        if (!nav) continue;
        const style = getComputedStyle(nav);
        const rect = nav.getBoundingClientRect();
        // A sidebar can retain its full height while animating into a bottom dock.
        if (style.position === 'fixed' && style.visibility === 'visible' && rect.height > 0 && rect.width > rect.height
          && rect.right > trigger.left && rect.left < trigger.right) {
          bottom = Math.min(bottom, rect.top);
        }
      }
    }
    if (!trigger.width || !trigger.height || !width || trigger.right <= left || trigger.left >= right
      || trigger.bottom <= top || trigger.top >= bottom) return false;

    const below = Math.max(0, bottom - trigger.bottom - 12);
    const above = Math.max(0, trigger.top - top - 12);
    const desired = Math.min(220, this.optionsContainer.scrollHeight || 220);
    const opensUp = below < desired && above > below;
    const available = opensUp ? above : below;
    if (!available) return false;
    this.element.classList.toggle('opens-up', opensUp);
    const { style } = this.optionsContainer;
    style.maxHeight = `${Math.min(220, available)}px`;
    if (this.usePopover()) {
      const menuWidth = Math.min(trigger.width, width);
      const gap = document.documentElement.dataset.theme === 'apple27' ? 6 : document.documentElement.dataset.theme === 'winui' ? 4 : 5;
      style.left = `${Math.max(left, Math.min(trigger.left, right - menuWidth))}px`;
      style.width = `${menuWidth}px`;
      style.top = opensUp ? 'auto' : `${trigger.bottom + gap}px`;
      style.bottom = opensUp ? `${window.innerHeight - trigger.top + gap}px` : 'auto';
    }
    return true;
  }
  private static schedulePlacement(): void {
    if (!CustomSelect.openInstance || CustomSelect.placementFrame !== null) return;
    CustomSelect.placementFrame = requestAnimationFrame(() => {
      CustomSelect.placementFrame = null;
      const current = CustomSelect.openInstance;
      if (current && !current.positionMenu()) current.close(true);
    });
  }
  private observeMenuLayout(): void {
    const observer = CustomSelect.layoutObserver;
    if (!observer) return;
    const scroller = this.element.closest('main, .modal-content');
    const header = this.element.closest('.page')?.querySelector('.page-header');
    const billingDock = this.element.closest('#billing-center')?.querySelector('.billing-mobile-nav');
    for (const element of [this.trigger, scroller, header, document.getElementById('nav'), billingDock]) {
      if (element) observer.observe(element, { box: 'border-box' });
    }
  }
  static closeOpen(): void {
    CustomSelect.openInstance?.close();
  }
  close(immediate = false) {
    if (CustomSelect.openInstance === this) {
      CustomSelect.openInstance = null;
      CustomSelect.layoutObserver?.disconnect();
    }
    if (this.scrollFrame !== null) { cancelAnimationFrame(this.scrollFrame); this.scrollFrame = null; }
    if (!this.element.classList.contains('open')) return;
    this.element.classList.remove('open'); this.trigger.setAttribute('aria-expanded', 'false');
    this.trigger.removeAttribute('aria-activedescendant');
    this.highlightedOption?.classList.remove('keyboard-active');
    this.highlightedOption = null;
    this.optionsContainer.inert = true;
    if (this.closeTimer !== null) window.clearTimeout(this.closeTimer);
    if (immediate) {
      this.closeTimer = null;
      this.hideOptions();
      return;
    }
    const style = getComputedStyle(this.optionsContainer);
    const milliseconds = (value: string) => Math.max(0, ...value.split(',').map(part => Number.parseFloat(part) * (part.trim().endsWith('ms') ? 1 : 1000)));
    const duration = window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : milliseconds(style.transitionDuration) + milliseconds(style.transitionDelay);
    const finishClose = () => {
      this.closeTimer = null;
      if (!this.element.classList.contains('open')) this.hideOptions();
    };
    if (duration <= 0) {
      finishClose();
      return;
    }
    this.closeTimer = window.setTimeout(finishClose, duration);
  }
  private highlight(index: number, keyboard = true) {
    const options = this.options(); if (!options.length) return;
    this.activeIndex = (index + options.length) % options.length;
    const option = options[this.activeIndex];
    const highlighted = keyboard ? option : null;
    if (this.highlightedOption !== highlighted) {
      this.highlightedOption?.classList.remove('keyboard-active');
      highlighted?.classList.add('keyboard-active');
      this.highlightedOption = highlighted;
    }
    this.trigger.setAttribute('aria-activedescendant', option.id);
    // Coalesce rapid opens/arrow presses into one list-only scroll per frame.
    // Closing/replacing the menu cancels the pending read; ancestors never move.
    if (this.scrollFrame !== null) cancelAnimationFrame(this.scrollFrame);
    this.scrollFrame = requestAnimationFrame(() => {
      this.scrollFrame = null;
      if (!this.element.isConnected || !this.element.classList.contains('open')) return;
      const top = option.offsetTop;
      const bottom = top + option.offsetHeight;
      const viewport = this.optionsContainer;
      const scrollTop = viewport.scrollTop;
      const height = viewport.clientHeight;
      if (top < scrollTop) viewport.scrollTop = top;
      else if (bottom > scrollTop + height) viewport.scrollTop = bottom - height;
    });
  }
  private open(keyboard = false) {
    if (this.disabled) return;
    CustomSelect.openInstance?.close();
    CustomSelect.openInstance = this;
    if (this.closeTimer !== null) { window.clearTimeout(this.closeTimer); this.closeTimer = null; }
    this.element.classList.add('menu-positioning');
    this.optionsContainer.hidden = false;
    this.optionsContainer.inert = false;
    if (this.usePopover() && !this.optionsContainer.matches(':popover-open')) {
      try { (this.optionsContainer as HTMLElement & { showPopover: () => void }).showPopover(); } catch {}
    }
    // Resolve placement while the flyout is invisible. Keep that placement on
    // exit too; changing top/bottom halfway through a transform causes a jump.
    if (!this.positionMenu()) {
      this.element.classList.remove('menu-positioning');
      this.hideOptions();
      CustomSelect.openInstance = null;
      return;
    }
    this.optionsContainer.getBoundingClientRect();
    this.element.classList.remove('menu-positioning');
    this.element.classList.add('open'); this.trigger.setAttribute('aria-expanded', 'true');
    this.observeMenuLayout();
    this.scrollPositions = new WeakMap();
    for (let parent = this.element.parentElement; parent; parent = parent.parentElement) {
      this.scrollPositions.set(parent, {top: parent.scrollTop, left: parent.scrollLeft});
    }
    this.highlight(Math.max(0, this.options().findIndex(option => option.dataset.value === this._value)), keyboard);
  }
  private prepareOptions() {
    this.allOptions = Array.from(this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option'));
    this.optionsByValue.clear();
    this.selectedOptions = [];
    this.allOptions.forEach((option, index) => {
      option.id = `${this.element.id}-option-${index}`; option.setAttribute('role', 'option');
      const value = option.dataset.value || '';
      const group = this.optionsByValue.get(value) || [];
      group.push(option); this.optionsByValue.set(value, group);
      const selected = value === this._value;
      option.classList.toggle('selected', selected);
      option.setAttribute('aria-selected', String(selected));
      if (selected) this.selectedOptions.push(option);
    });
  }
  constructor(elementId: string) {
    this.element = document.getElementById(elementId)!;
    this.trigger = this.element.querySelector('.custom-select-trigger')!;
    this.triggerSpan = this.trigger.querySelector('span')!;
    this.optionsContainer = this.element.querySelector('.custom-select-options')!;
    if (this.usePopover()) this.optionsContainer.setAttribute('popover', 'manual');
    this.optionsContainer.hidden = true;
    this.optionsContainer.inert = true;
    this.trigger.tabIndex = 0; this.trigger.setAttribute('role', 'combobox');
    this.trigger.setAttribute('aria-haspopup', 'listbox'); this.trigger.setAttribute('aria-expanded', 'false');
    const accessibleLabel = this.element.getAttribute('aria-label');
    if (accessibleLabel) this.trigger.setAttribute('aria-label', accessibleLabel);
    this.optionsContainer.id ||= `${elementId}-options`;
    this.optionsContainer.setAttribute('role', 'listbox'); this.trigger.setAttribute('aria-controls', this.optionsContainer.id);
    const selected = this.optionsContainer.querySelector<HTMLElement>('.custom-option.selected');
    if (selected) { this._value = selected.dataset.value || ''; this.triggerSpan.textContent = selected.textContent; }
    this.prepareOptions();
    this.trigger.addEventListener('click', event => {
      event.stopPropagation(); if (this.disabled) return;
      if (this.element.classList.contains('open')) this.close(); else this.open();
    });
    this.trigger.addEventListener('keydown', event => {
      if (this.disabled) return;
      if (event.key === 'Escape' || event.key === 'Tab') {
        if (event.key === 'Escape' && this.element.classList.contains('open')) { event.preventDefault(); event.stopPropagation(); }
        this.close(); return;
      }
      if (!['Enter', ' ', 'ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      if (!this.element.classList.contains('open')) {
        this.open(['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key));
        if (event.key === 'Home' || event.key === 'End') this.highlight(event.key === 'Home' ? 0 : this.options().length - 1);
        return;
      }
      if (event.key === 'Enter' || event.key === ' ') { this.options()[this.activeIndex]?.click(); return; }
      this.highlight(event.key === 'Home' ? 0 : event.key === 'End' ? this.options().length - 1 : this.activeIndex + (event.key === 'ArrowUp' ? -1 : 1));
    });
    this.trigger.addEventListener('blur', event => {
      if (!this.element.contains(event.relatedTarget as Node | null)) this.close();
    });
    this.optionsContainer.addEventListener('mousedown', event => {
      if ((event.target as HTMLElement).closest('.custom-option')) event.preventDefault();
    });
    // Consume wheel momentum inside the list, including at its limits. Letting
    // it reach the page used to scroll an ancestor and close the flyout.
    this.optionsContainer.addEventListener('wheel', event => {
      if (!this.element.classList.contains('open') || !event.deltaY) return;
      event.preventDefault(); event.stopPropagation();
      const unit = event.deltaMode === 1 ? 20 : event.deltaMode === 2 ? this.optionsContainer.clientHeight : 1;
      this.optionsContainer.scrollTop += event.deltaY * unit;
    }, {passive: false});
    this.optionsContainer.addEventListener('click', event => {
      const option = (event.target as HTMLElement).closest<HTMLElement>('.custom-option');
      if (this.disabled || !option || option.getAttribute('aria-disabled') === 'true') return;
      const value = option.dataset.value || ''; this.setValue(value); this.close(); this.trigger.focus({ preventScroll: true });
      this.onChangeCallbacks.forEach(callback => callback(value));
    });
    if (!CustomSelect.outsideHandlerInstalled) {
      document.addEventListener('click', CustomSelect.closeOpen);
      const schedulePlacement = CustomSelect.schedulePlacement;
      if (typeof ResizeObserver !== 'undefined') CustomSelect.layoutObserver = new ResizeObserver(schedulePlacement);
      window.addEventListener('resize', schedulePlacement);
      window.visualViewport?.addEventListener('resize', schedulePlacement);
      window.visualViewport?.addEventListener('scroll', schedulePlacement);
      document.addEventListener('scroll', event => {
        const current = CustomSelect.openInstance;
        if (current && event.target instanceof Element && !current.optionsContainer.contains(event.target)
          && event.target.contains(current.element)) {
          const before = current.scrollPositions.get(event.target);
          // Focusing a trigger can queue a scroll event before it opens. Its
          // offset is already captured above; it must not close the new menu.
          if (!before || Math.abs(before.top-event.target.scrollTop) > .5 || Math.abs(before.left-event.target.scrollLeft) > .5) current.close();
        }
      }, {capture:true,passive:true});
      CustomSelect.outsideHandlerInstalled = true;
    }
  }
  get value(): string { return this._value; }
  set value(value: string) { this.setValue(value); }
  setValue(value: string) {
    const selected = this.optionsByValue.get(value) || [];
    for (const option of this.selectedOptions) {
      if (!selected.includes(option)) { option.classList.remove('selected'); option.setAttribute('aria-selected', 'false'); }
    }
    for (const option of selected) {
      if (!this.selectedOptions.includes(option)) { option.classList.add('selected'); option.setAttribute('aria-selected', 'true'); }
    }
    this._value = value;
    this.selectedOptions = selected;
    const text = selected[selected.length - 1]?.textContent || value;
    if (this.triggerSpan.textContent !== text) this.triggerSpan.textContent = text;
  }
  setDisabled(disabled: boolean) {
    this.disabled = disabled; this.element.classList.toggle('is-disabled', disabled);
    this.trigger.setAttribute('aria-disabled', String(disabled)); this.trigger.tabIndex = disabled ? -1 : 0;
    if (disabled) this.close();
  }
  addEventListener(event: 'change', callback: (event: { target: { value: string } }) => void) {
    if (event === 'change') this.onChangeCallbacks.push(value => callback({ target: { value } }));
  }
  setOptions(options: { value: string, text: string }[]) {
    const value = options.some(option => option.value === this._value) ? this._value : options[0]?.value || '';
    // Configuration refreshes often resend an unchanged list. Preserve its DOM,
    // scroll position and open state instead of rebuilding every option.
    if (options.length === this.allOptions.length && options.every((option, i) =>
      option.value === this.allOptions[i].dataset.value && option.text === this.allOptions[i].textContent)) {
      this.setValue(value);
      return;
    }
    this.close();
    const fragment = document.createDocumentFragment();
    options.forEach(option => {
      const element = document.createElement('div'); element.className = 'custom-option';
      element.dataset.value = option.value; element.textContent = option.text; fragment.append(element);
    });
    this.optionsContainer.replaceChildren(fragment);
    this.prepareOptions();
    this.setValue(value);
  }
}
