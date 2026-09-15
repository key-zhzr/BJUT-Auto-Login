import { CustomSelect } from '../src/custom-select';
import { setupKeyboardNavigation } from '../src/keyboard-navigation';
import { applyAppearance } from '../src/appearance';

const assert = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
const tick = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
const result = document.getElementById('result')!;
const run = document.getElementById('run') as HTMLButtonElement;
let observerCallbacks = 0;
const Observer = window.MutationObserver;
window.MutationObserver = class extends Observer {
  constructor(callback: MutationCallback) {
    super((records, observer) => { observerCallbacks++; callback(records, observer); });
  }
};

function control(id: string, parent: string) {
  const row = document.createElement('div'); row.className = 'setting-item';
  const label = document.createElement('span'); label.textContent = id;
  const host = document.createElement('div'); host.id = id.replaceAll(' ', '-'); host.className = 'custom-select'; host.setAttribute('aria-label', id);
  const trigger = document.createElement('div'); trigger.className = 'custom-select-trigger'; trigger.append(document.createElement('span'));
  const options = document.createElement('div'); options.className = 'custom-select-options';
  host.append(trigger, options); row.append(label, host); document.getElementById(parent)!.append(row);
  const component = new CustomSelect(host.id);
  component.setOptions(Array.from({ length: 30 }, (_, i) => ({ value: String(i), text: `选项 ${i}` })));
  return component;
}
const native = Array.from({ length: 24 }, (_, i) => control(`Android 选项 ${i + 1}`, i < 2 ? 'native-controls' : 'extra-controls'));
document.body.classList.remove('is-android');
const desktop = [control('桌面选项 A', 'desktop-controls'), control('桌面选项 B', 'desktop-controls')];
document.body.classList.add('is-android');
applyAppearance('basic', 'blue', 'light');
setupKeyboardNavigation();

run.addEventListener('click', async () => {
  run.disabled = true; result.textContent = '';
  const note = (text: string) => { result.textContent += `${text}\n`; };
  const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
  HTMLElement.prototype.scrollIntoView = function () { throw new Error('控件不应滚动祖先容器'); };
  let changes = 0;
  const callbacks = native.map(component => { const callback = () => { changes++; }; component.onChangeCallbacks.push(callback); return callback; });
  try {
    for (const theme of ['basic', 'apple27', 'winui'] as const) {
      applyAppearance(theme, 'blue', 'light'); await tick();
      const before = changes;
      for (let i = 0; i < 2000; i++) {
        const component = native[i % native.length];
        const select = component.element.querySelector('select')!;
        select.value = String(i % 30); select.dispatchEvent(new Event('change', { bubbles: true }));
        component.trigger.click(); // Presentation cannot activate a WebView flyout.
        assert(component.value === select.value, '系统选项没有同步到应用值');
        assert(!component.element.classList.contains('open'), 'Android 不应展开 WebView 弹层');
        if (i % 100 === 0) await tick();
      }
      assert(changes - before === 2000, '选择回调重复或丢失');
      const style = getComputedStyle(native[0].optionsContainer);
      assert(style.display === 'none' && style.backdropFilter === 'none', 'Android 弹层仍参与模糊渲染');
      assert(getComputedStyle(document.getElementById('native-controls')!).contentVisibility === 'visible', '设置卡片仍使用动态显示锁');
      note(`通过：${theme} · 2000 次系统选项状态切换`);
    }
    const first = native[0]; const select = first.element.querySelector('select')!;
    first.setDisabled(true); assert(select.disabled, '系统选项未同步禁用');
    const beforeDisabled = changes; select.dispatchEvent(new Event('change', { bubbles: true }));
    assert(changes === beforeDisabled, '禁用选项仍触发回调'); first.setDisabled(false);
    const beforeReplace = changes;
    first.setOptions([{ value: 'a', text: '动态 A' }, { value: 'b', text: '动态 B' }]); first.setValue('b');
    assert(select.value === 'b' && first.triggerSpan.textContent === '动态 B', '动态选项未同步');
    assert(changes === beforeReplace, '程序更新不应触发用户选择回调');
    first.setOptions(Array.from({ length: 30 }, (_, i) => ({ value: String(i), text: `选项 ${i}` })));
    note('通过：动态列表、程序更新和禁用状态');

    document.body.classList.remove('is-android');
    for (let i = 0; i < 400; i++) {
      desktop[0].trigger.click(); desktop[1].trigger.click();
      assert(!desktop[0].element.classList.contains('open') && desktop[1].element.classList.contains('open'), '存在多个展开菜单');
      desktop[1].trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
      if (i % 20 === 0) await tick();
    }
    desktop[0].trigger.click();
    assert(!desktop[0].optionsContainer.querySelector('.keyboard-active'), '指针打开菜单时不应出现键盘指示框');
    desktop[0].trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    assert(desktop[0].optionsContainer.querySelector('.keyboard-active')?.getAttribute('data-value') === '29', '键盘导航后缺少指示框');
    assert(desktop[0].optionsContainer.scrollTop > 0, '键盘未滚动长列表');
    desktop[0].trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    assert(desktop[0].value === '29', '键盘选项提交错误');
    note('通过：800 次桌面菜单展开及长列表键盘选择');

    const modal = document.getElementById('test-modal')!;
    desktop[0].trigger.focus(); modal.classList.remove('hidden'); await tick(); await tick();
    assert(document.activeElement?.id === 'modal-first', '弹窗焦点未进入');
    const baseline = observerCallbacks;
    for (let i = 0; i < 100; i++) {
      first.setValue(String(i % 30)); document.getElementById('background-updates')!.textContent = String(i);
      await Promise.resolve();
    }
    await tick(); assert(observerCallbacks === baseline, '无关内容更新仍触发全页面焦点监听');
    document.getElementById('modal-first')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true }));
    assert(document.activeElement?.id === 'modal-last', '弹窗 Tab 循环失效');
    modal.classList.add('hidden'); await tick(); await tick();
    assert(document.activeElement === desktop[0].trigger, '关闭弹窗未恢复焦点');
    note('通过：弹窗焦点、Tab 循环及无关更新零监听回调');
    note('全部通过。原生 Android 崩溃仍需故障设备复测。');
  } catch (error) {
    note(`失败：${String(error)}`); throw error;
  } finally {
    HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
    native.forEach((component, index) => { component.onChangeCallbacks = component.onChangeCallbacks.filter(callback => callback !== callbacks[index]); });
    document.getElementById('test-modal')!.classList.add('hidden');
    document.body.classList.add('is-android'); run.disabled = false;
  }
});
