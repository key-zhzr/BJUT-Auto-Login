import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { SETTINGS_COPY } from '../src/ui-copy/settings.ts';

// Guard the existing "first paint has complete copy before JS loads" contract.
// See docs/ui-text-guide.md and the header of src/ui-text.ts.
const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
const normalize = (value) => value.replace(/\s+/g, ' ').trim();
const flatHtml = normalize(html);

// Action buttons (<button id="…">) only receive their title as a runtime
// tooltip: ui-text.ts::setSettingItem resolves .setting-info h4 and is a no-op
// inside .setting-actions. They therefore do not participate in the first-paint
// title assertion. The check is self-describing, not a hardcoded allowlist.
const isActionTrigger = (id) => new RegExp(`<button[^>]*\\bid="${id}"`).test(html);

test('每个设置控件 id 都存在于首帧 HTML', () => {
  const missing = Object.keys(SETTINGS_COPY.items).filter((id) => !html.includes(`id="${id}"`));
  assert.deepEqual(missing, [], `index.html 缺少这些控件 id：${missing.join(', ')}`);
});

test('每个设置项标题都在 JS 加载前就存在于首帧 HTML', () => {
  const missing = Object.entries(SETTINGS_COPY.items)
    .filter(([id]) => !isActionTrigger(id))
    .filter(([, copy]) => !flatHtml.includes(normalize(copy.title)))
    .map(([id]) => id);
  assert.deepEqual(missing, [], `index.html 缺少这些设置项的回退标题：${missing.join(', ')}`);
});

test('设置页标题与说明保留首帧回退文案', () => {
  for (const value of [SETTINGS_COPY.pageTitle, SETTINGS_COPY.pageDescription]) {
    assert.ok(flatHtml.includes(normalize(value)), `index.html 缺少回退文案：${value}`);
  }
});
