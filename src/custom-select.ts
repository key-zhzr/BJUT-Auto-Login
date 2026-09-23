export class CustomSelect {
  private static openInstance: CustomSelect | null = null;
  private static outsideHandlerInstalled = false;
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
  onChangeCallbacks: ((value: string) => void)[] = [];

  private options() { return this.allOptions.filter(option => option.getAttribute('aria-disabled') !== 'true'); }
  private close() {
    if (CustomSelect.openInstance === this) CustomSelect.openInstance = null;
    if (this.scrollFrame !== null) { cancelAnimationFrame(this.scrollFrame); this.scrollFrame = null; }
    if (!this.element.classList.contains('open')) return;
    this.element.classList.remove('open'); this.trigger.setAttribute('aria-expanded', 'false');
    this.trigger.removeAttribute('aria-activedescendant');
    this.highlightedOption?.classList.remove('keyboard-active');
    this.highlightedOption = null;
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
    const trigger = this.trigger.getBoundingClientRect();
    const scroller = this.element.closest<HTMLElement>('.page-content, .modal-content');
    const bounds = scroller?.getBoundingClientRect();
    const viewport = window.visualViewport;
    const top = Math.max(bounds?.top ?? 0, viewport?.offsetTop ?? 0);
    const bottom = Math.min(bounds?.bottom ?? window.innerHeight, (viewport?.offsetTop ?? 0) + (viewport?.height ?? window.innerHeight));
    const below = Math.max(0, bottom - trigger.bottom - 12);
    const above = Math.max(0, trigger.top - top - 12);
    const opensUp = below < 160 && above > below;
    this.element.classList.toggle('opens-up', opensUp);
    this.optionsContainer.style.maxHeight = `${Math.min(220, opensUp ? above : below)}px`;
    this.element.classList.add('open'); this.trigger.setAttribute('aria-expanded', 'true');
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
    this.optionsContainer.addEventListener('click', event => {
      const option = (event.target as HTMLElement).closest<HTMLElement>('.custom-option');
      if (this.disabled || !option || option.getAttribute('aria-disabled') === 'true') return;
      const value = option.dataset.value || ''; this.setValue(value); this.close(); this.trigger.focus({ preventScroll: true });
      this.onChangeCallbacks.forEach(callback => callback(value));
    });
    if (!CustomSelect.outsideHandlerInstalled) {
      document.addEventListener('click', () => CustomSelect.openInstance?.close());
      document.addEventListener('scroll', event => {
        const current = CustomSelect.openInstance;
        if (current && event.target instanceof Element && !current.optionsContainer.contains(event.target)
          && event.target.contains(current.element)) current.close();
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
