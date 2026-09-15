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
  private nativeSelect: HTMLSelectElement | null = null;
  onChangeCallbacks: ((value: string) => void)[] = [];

  private options() { return Array.from(this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option:not([aria-disabled="true"])')); }
  private close() {
    if (CustomSelect.openInstance === this) CustomSelect.openInstance = null;
    if (!this.element.classList.contains('open')) return;
    this.element.classList.remove('open'); this.trigger.setAttribute('aria-expanded', 'false');
    this.trigger.removeAttribute('aria-activedescendant');
    this.options().filter(option => option.classList.contains('keyboard-active')).forEach(option => option.classList.remove('keyboard-active'));
  }
  private highlight(index: number, keyboard = true) {
    const options = this.options(); if (!options.length) return;
    this.activeIndex = (index + options.length) % options.length;
    options.forEach((option, i) => option.classList.toggle('keyboard-active', keyboard && i === this.activeIndex));
    this.trigger.setAttribute('aria-activedescendant', options[this.activeIndex].id);
    // Scroll only the list. scrollIntoView can unlock/reposition every ancestor
    // (including content-visibility sections) while the popup is being painted.
    const option = options[this.activeIndex];
    const top = option.offsetTop;
    const bottom = top + option.offsetHeight;
    const viewport = this.optionsContainer;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (bottom > viewport.scrollTop + viewport.clientHeight) viewport.scrollTop = bottom - viewport.clientHeight;
  }
  private open(keyboard = false) {
    if (this.disabled || this.nativeSelect) return;
    CustomSelect.openInstance?.close();
    CustomSelect.openInstance = this;
    this.element.classList.add('open'); this.trigger.setAttribute('aria-expanded', 'true');
    this.highlight(Math.max(0, this.options().findIndex(option => option.dataset.value === this._value)), keyboard);
  }
  private prepareOptions() {
    this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option').forEach((option, index) => {
      option.id = `${this.element.id}-option-${index}`; option.setAttribute('role', 'option');
      option.setAttribute('aria-selected', String(option.dataset.value === this._value));
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
    if (document.body.classList.contains('is-android')) {
      this.installNativeSelect(accessibleLabel || this.triggerSpan.textContent || '选择选项');
      return;
    }
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
      CustomSelect.outsideHandlerInstalled = true;
    }
  }
  private installNativeSelect(label: string) {
    // Android opens its native option picker instead of animating a WebView
    // popup. Keep the themed trigger as presentation, with one accessible input.
    const select = document.createElement('select');
    this.nativeSelect = select;
    select.className = 'native-select-input';
    select.id = `${this.element.id}-native`;
    select.setAttribute('aria-label', label);
    this.element.classList.add('native-select');
    this.trigger.tabIndex = -1;
    this.trigger.setAttribute('aria-hidden', 'true');
    this.optionsContainer.hidden = true;
    this.element.append(select);
    this.syncNativeOptions();
    select.addEventListener('change', () => {
      if (this.disabled) return;
      this.setValue(select.value);
      this.onChangeCallbacks.forEach(callback => callback(this._value));
    });
  }
  private syncNativeOptions() {
    if (!this.nativeSelect) return;
    const options = Array.from(this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option')).map(source => {
      const option = document.createElement('option');
      option.value = source.dataset.value || '';
      option.textContent = source.textContent;
      option.disabled = source.getAttribute('aria-disabled') === 'true';
      return option;
    });
    this.nativeSelect.replaceChildren(...options);
    this.nativeSelect.value = this._value;
  }
  get value(): string { return this._value; }
  set value(value: string) { this.setValue(value); }
  setValue(value: string) {
    this._value = value; let selectedText = '';
    this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option').forEach(option => {
      const selected = option.dataset.value === value;
      option.classList.toggle('selected', selected); option.setAttribute('aria-selected', String(selected));
      if (selected) selectedText = option.textContent || '';
    });
    this.triggerSpan.textContent = selectedText || value;
    if (this.nativeSelect) this.nativeSelect.value = value;
  }
  setDisabled(disabled: boolean) {
    this.disabled = disabled; this.element.classList.toggle('is-disabled', disabled);
    this.trigger.setAttribute('aria-disabled', String(disabled)); this.trigger.tabIndex = disabled || this.nativeSelect ? -1 : 0;
    if (this.nativeSelect) this.nativeSelect.disabled = disabled;
    if (disabled) this.close();
  }
  addEventListener(event: 'change', callback: (event: { target: { value: string } }) => void) {
    if (event === 'change') this.onChangeCallbacks.push(value => callback({ target: { value } }));
  }
  setOptions(options: { value: string, text: string }[]) {
    this.close(); this.optionsContainer.replaceChildren();
    options.forEach(option => {
      const element = document.createElement('div'); element.className = 'custom-option';
      element.dataset.value = option.value; element.textContent = option.text; this.optionsContainer.append(element);
    });
    this.setValue(options.some(option => option.value === this._value) ? this._value : options[0]?.value || '');
    this.prepareOptions();
    this.syncNativeOptions();
  }
}
