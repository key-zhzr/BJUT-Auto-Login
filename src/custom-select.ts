export class CustomSelect {
  private static instances = new Set<CustomSelect>();
  private static outsideHandlerInstalled = false;
  element: HTMLElement;
  trigger: HTMLElement;
  triggerSpan: HTMLSpanElement;
  optionsContainer: HTMLElement;
  private _value = '';
  private activeIndex = 0;
  private disabled = false;
  onChangeCallbacks: ((value: string) => void)[] = [];

  private options() { return Array.from(this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option:not([aria-disabled="true"])')); }
  private close() {
    this.element.classList.remove('open'); this.trigger.setAttribute('aria-expanded', 'false');
    this.trigger.removeAttribute('aria-activedescendant');
    this.options().forEach(option => option.classList.remove('keyboard-active'));
  }
  private highlight(index: number) {
    const options = this.options(); if (!options.length) return;
    this.activeIndex = (index + options.length) % options.length;
    options.forEach((option, i) => option.classList.toggle('keyboard-active', i === this.activeIndex));
    this.trigger.setAttribute('aria-activedescendant', options[this.activeIndex].id);
    options[this.activeIndex].scrollIntoView({ block: 'nearest' });
  }
  private open() {
    if (this.disabled) return;
    CustomSelect.instances.forEach(instance => { if (instance !== this) instance.close(); });
    this.element.classList.add('open'); this.trigger.setAttribute('aria-expanded', 'true');
    this.highlight(Math.max(0, this.options().findIndex(option => option.dataset.value === this._value)));
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
    CustomSelect.instances.add(this);
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
      if (!this.element.classList.contains('open')) { this.open(); return; }
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
      const value = option.dataset.value || ''; this.setValue(value); this.close(); this.trigger.focus();
      this.onChangeCallbacks.forEach(callback => callback(value));
    });
    if (!CustomSelect.outsideHandlerInstalled) {
      document.addEventListener('click', () => CustomSelect.instances.forEach(instance => instance.close()));
      CustomSelect.outsideHandlerInstalled = true;
    }
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
    this.close(); this.optionsContainer.replaceChildren();
    options.forEach(option => {
      const element = document.createElement('div'); element.className = 'custom-option';
      element.dataset.value = option.value; element.textContent = option.text; this.optionsContainer.append(element);
    });
    this.setValue(options.some(option => option.value === this._value) ? this._value : options[0]?.value || '');
    this.prepareOptions();
  }
}
