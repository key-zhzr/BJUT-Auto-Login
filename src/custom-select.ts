export class CustomSelect {
  private static instances = new Set<CustomSelect>();
  private static outsideHandlerInstalled = false;
  element: HTMLElement;
  trigger: HTMLElement;
  triggerSpan: HTMLSpanElement;
  optionsContainer: HTMLElement;
  private _value = '';
  onChangeCallbacks: ((value: string) => void)[] = [];

  private close() {
    this.element.classList.remove('open');
    this.trigger.setAttribute('aria-expanded', 'false');
  }

  constructor(elementId: string) {
    this.element = document.getElementById(elementId)!;
    this.trigger = this.element.querySelector('.custom-select-trigger')!;
    this.triggerSpan = this.trigger.querySelector('span')!;
    this.optionsContainer = this.element.querySelector('.custom-select-options')!;
    CustomSelect.instances.add(this);
    this.trigger.setAttribute('role', 'combobox');
    this.trigger.setAttribute('aria-haspopup', 'listbox');
    this.trigger.setAttribute('aria-expanded', 'false');
    const accessibleLabel = this.element.getAttribute('aria-label');
    if (accessibleLabel) this.trigger.setAttribute('aria-label', accessibleLabel);
    this.optionsContainer.setAttribute('role', 'listbox');
    this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option').forEach(option => {
      option.setAttribute('role', 'option');
      option.setAttribute('aria-selected', String(option.classList.contains('selected')));
    });

    this.trigger.addEventListener('click', event => {
      event.stopPropagation();
      CustomSelect.instances.forEach(instance => {
        if (instance !== this) instance.close();
      });
      this.element.classList.toggle('open');
      this.trigger.setAttribute('aria-expanded', String(this.element.classList.contains('open')));
    });

    this.trigger.addEventListener('keydown', event => {
      if (event.key === 'Escape') {
        this.close();
        return;
      }
      if (!['Enter', ' ', 'ArrowDown', 'ArrowUp'].includes(event.key)) return;
      event.preventDefault();
      if (!this.element.classList.contains('open')) {
        this.trigger.click();
        return;
      }
      const options = Array.from(
        this.optionsContainer.querySelectorAll<HTMLElement>('.custom-option'),
      );
      const selectedIndex = Math.max(
        0,
        options.findIndex(option => option.classList.contains('selected')),
      );
      const nextIndex = event.key === 'ArrowUp'
        ? (selectedIndex - 1 + options.length) % options.length
        : (selectedIndex + 1) % options.length;
      if (event.key === 'Enter' || event.key === ' ') options[selectedIndex]?.click();
      else options[nextIndex]?.click();
    });

    this.optionsContainer.addEventListener('click', event => {
      const option = (event.target as HTMLElement).closest('.custom-option') as HTMLElement;
      if (!option) return;
      const value = option.getAttribute('data-value') || '';
      this.value = value;
      this.close();
      this.onChangeCallbacks.forEach(callback => callback(value));
    });

    if (!CustomSelect.outsideHandlerInstalled) {
      document.addEventListener('click', () => {
        CustomSelect.instances.forEach(instance => instance.close());
      });
      CustomSelect.outsideHandlerInstalled = true;
    }

    const selectedOption = this.optionsContainer.querySelector(
      '.custom-option.selected',
    ) as HTMLElement;
    if (selectedOption) {
      this._value = selectedOption.getAttribute('data-value') || '';
      this.triggerSpan.textContent = selectedOption.textContent;
    }
  }

  get value(): string {
    return this._value;
  }

  set value(value: string) {
    this.setValue(value);
  }

  setValue(value: string) {
    this._value = value;
    let selectedText = '';
    this.optionsContainer.querySelectorAll('.custom-option').forEach(option => {
      if (option.getAttribute('data-value') === value) {
        option.classList.add('selected');
        option.setAttribute('aria-selected', 'true');
        selectedText = option.textContent || '';
      } else {
        option.classList.remove('selected');
        option.setAttribute('aria-selected', 'false');
      }
    });
    this.triggerSpan.textContent = selectedText || value;
  }

  setDisabled(disabled: boolean) {
    this.element.classList.toggle('is-disabled', disabled);
    this.trigger.setAttribute('aria-disabled', String(disabled));
    this.trigger.tabIndex = disabled ? -1 : 0;
    if (disabled) this.close();
  }

  addEventListener(
    event: 'change',
    callback: (event: { target: { value: string } }) => void,
  ) {
    if (event === 'change') {
      this.onChangeCallbacks.push(value => {
        callback({ target: { value } });
      });
    }
  }

  setOptions(options: { value: string, text: string }[]) {
    this.optionsContainer.innerHTML = '';
    options.forEach(option => {
      const element = document.createElement('div');
      element.className = 'custom-option';
      element.setAttribute('role', 'option');
      element.setAttribute('aria-selected', 'false');
      element.setAttribute('data-value', option.value);
      element.textContent = option.text;
      if (option.value === this._value) {
        element.classList.add('selected');
        element.setAttribute('aria-selected', 'true');
        this.triggerSpan.textContent = option.text;
      }
      this.optionsContainer.appendChild(element);
    });
    if (!this.optionsContainer.querySelector('.custom-option.selected') && options.length > 0) {
      this.setValue(options[0].value);
    }
  }
}
