import test from 'node:test';
import assert from 'node:assert/strict';
import {
  DEFAULT_MAX_LOG_ENTRIES,
  SESSION_START_MARKER,
  VirtualLogModel,
  VirtualLogsController,
  computeVirtualRange,
  estimateLogEntryHeight,
  mergeLogSnapshots,
} from '../src/virtual-logs.ts';

function makeEntry(index, overrides = {}) {
  return {
    time: `2026-11-28 12:${String(Math.floor(index / 60) % 60).padStart(2, '0')}:${String(index % 60).padStart(2, '0')}`,
    module: index % 3 === 0 ? '网络' : index % 3 === 1 ? '认证' : '计费',
    message: `日志消息-${index} campus status ok`,
    type: index % 20 === 0 ? 'error' : index % 5 === 0 ? 'success' : index % 7 === 0 ? 'debug' : 'info',
    ...overrides,
  };
}

test('5,000-entry pre-indexed filtering completes well under 25ms and preserves exact matches', () => {
  const model = new VirtualLogModel();
  const entries = [];
  for (let i = 0; i < 5000; i += 1) {
    if (i === 1000 || i === 4200) {
      entries.push(makeEntry(i, {
        module: '系统',
        message: `${SESSION_START_MARKER} 2026-11-28 12:00:00`,
        type: 'info',
      }));
    } else if (i === 4321) {
      entries.push(makeEntry(i, {
        module: 'Portal',
        message: 'Special-Token-Needle authentication retry',
        type: 'error',
      }));
    } else {
      entries.push(makeEntry(i));
    }
  }
  model.setEntries(entries);
  assert.equal(model.totalCount, 5000);
  assert.equal(model.getLastSessionStartIndex(), 4200);

  // Warm JIT lightly then measure query + level + session switches across 5,000 items.
  model.setQuery('Special-Token-Needle');
  model.setQuery('');

  const start = performance.now();
  model.setQuery('SPECIAL-token-needle');
  assert.equal(model.filteredCount, 1);
  assert.equal(model.getFilteredEntry(0)?.module, 'Portal');

  model.setQuery('');
  model.setSession('current');
  assert.equal(model.filteredCount, 5000 - 4200);

  model.setLevel('error');
  assert.ok(model.filteredCount > 0 && model.filteredCount < 100);

  model.setLevel('all');
  model.setSession('all');
  model.setModuleFilter('认证');
  const authEntries = model.getFilteredEntries();
  assert.ok(authEntries.length > 0 && authEntries.every(entry => entry.module === '认证'));

  model.setModuleFilter('');
  const elapsedMs = performance.now() - start;
  assert.ok(elapsedMs < 25, `5,000-entry filtering took ${elapsedMs.toFixed(2)}ms (expected < 25ms)`);
});

test('variable-height virtual range calculation keeps rendered slice small (<= 70 rows out of 5,000)', () => {
  const model = new VirtualLogModel();
  const entries = Array.from({ length: 5000 }, (_, i) => makeEntry(i, {
    message: i % 15 === 0
      ? '长日志换行内容 '.repeat(12) + `row-${i}`
      : `short-row-${i}`,
  }));
  model.setEntries(entries);

  // Top, middle, and bottom scroll positions in a 620px viewport with 12-row overscan.
  const prefix = model.getPrefixSums(680);
  assert.equal(prefix.length, 5001);
  const totalHeight = prefix[5000];
  assert.ok(totalHeight > 5000 * 24);

  for (const scrollTop of [0, totalHeight * 0.45, Math.max(0, totalHeight - 620)]) {
    const range = computeVirtualRange(prefix, 5000, scrollTop, 620, 12);
    assert.ok(range.startIndex >= 0);
    assert.ok(range.endIndex < 5000);
    assert.ok(range.startIndex <= range.firstVisible);
    assert.ok(range.endIndex >= range.lastVisible);
    const sliceLen = range.endIndex - range.startIndex + 1;
    assert.ok(sliceLen <= 70, `expected rendered slice <= 70 rows, got ${sliceLen}`);
    assert.equal(
      Math.round(range.topOffset + (prefix[range.endIndex + 1] - prefix[range.startIndex]) + range.bottomOffset),
      Math.round(totalHeight),
    );
  }

  // Updating measured height for a wrapped entry adjusts subsequent prefix sums & anchor cleanly.
  const beforeTop = prefix[1000] + 5;
  const anchor = model.getAnchorForScrollTop(beforeTop, 680);
  assert.deepEqual(anchor, { index: 1000, offset: 5 });
  const targetEntry = model.getFilteredEntry(500);
  assert.ok(targetEntry);
  const oldEstimated = estimateLogEntryHeight(targetEntry, 680);
  model.setMeasuredHeight(targetEntry.id, oldEstimated + 42, 500);
  const adjustedScrollTop = model.getScrollTopForAnchor(anchor, 680);
  assert.equal(Math.round(adjustedScrollTop - beforeTop), 42);
});

test('ring eviction at >5,000 logs stays bounded and updates lastSessionStart in O(1)', () => {
  const model = new VirtualLogModel({ maxEntries: DEFAULT_MAX_LOG_ENTRIES });

  // Put a session divider at index 50, then push 5,050 entries total.
  for (let i = 0; i < 50; i += 1) {
    model.append(makeEntry(i));
  }
  model.append(makeEntry(50, { module: '系统', message: `${SESSION_START_MARKER} first` }));
  for (let i = 51; i < 5040; i += 1) {
    model.append(makeEntry(i));
  }
  assert.equal(model.totalCount, 5000);
  // 40 entries evicted from front (0..39), so divider originally at 50 is now at 10.
  assert.equal(model.getLastSessionStartIndex(), 10);

  // Push 11 more entries so the divider at index 10 falls off the front!
  for (let i = 5040; i < 5051; i += 1) {
    model.append(makeEntry(i));
  }
  assert.equal(model.totalCount, 5000);
  assert.equal(model.getLastSessionStartIndex(), -1);
  assert.equal(model.getEntries()[0].message.includes('51'), true);
  assert.equal(model.getEntries().at(-1)?.message.includes('5050'), true);
});

test('mergeLogSnapshots deduplicates boundary overlap without dropping duplicate live entries', () => {
  const e1 = makeEntry(1);
  const e2 = makeEntry(2);
  const e3 = makeEntry(3);
  const e4 = makeEntry(4);
  const dup3 = makeEntry(3);

  assert.deepEqual(
    mergeLogSnapshots([e1, e2, e3], [e2, e3, e4]),
    [e1, e2, e3, e4],
  );
  // First e3 overlaps the tail of initialLogs; second identical dup3 was emitted after snapshot and must survive.
  assert.deepEqual(
    mergeLogSnapshots([e1, e2, e3], [e3, dup3]),
    [e1, e2, e3, dup3],
  );
  // Disjoint snapshots concatenate cleanly.
  assert.deepEqual(
    mergeLogSnapshots([e1, e2], [e3, e4]),
    [e1, e2, e3, e4],
  );
  // Enforces 5,000 cap by trimming oldest entries.
  const capped = mergeLogSnapshots(
    Array.from({ length: 4998 }, (_, i) => makeEntry(i)),
    [makeEntry(4997), makeEntry(4998), makeEntry(4999), makeEntry(5000)],
    5000,
  );
  assert.equal(capped.length, 5000);
  assert.equal(capped[0].message, '日志消息-1 campus status ok');
  assert.equal(capped.at(-1)?.message, '日志消息-5000 campus status ok');
});

test('race-free ensureLoaded merges in-flight live appends and blocks stale history resurrection after clear()', async () => {
  // 1. Single-flight merge with overlapping + newer live entries during async get_logs().
  let resolveLoader;
  let loaderCalls = 0;
  const controller = new VirtualLogsController({
    loadHistory: () => {
      loaderCalls += 1;
      return new Promise(resolve => {
        resolveLoader = resolve;
      });
    },
  });

  // Live entry arriving before load begins:
  controller.append(makeEntry(10));

  const p1 = controller.ensureLoaded();
  const p2 = controller.ensureLoaded();
  assert.equal(loaderCalls, 1, 'ensureLoaded must coalesce concurrent calls into one single-flight request');

  // Live entries arriving while get_logs() is in flight:
  controller.append(makeEntry(11));
  controller.append(makeEntry(12));

  resolveLoader([makeEntry(8), makeEntry(9), makeEntry(10), makeEntry(11)]);
  await Promise.all([p1, p2]);

  assert.deepEqual(
    controller.getFilteredEntries().map(e => e.message),
    [8, 9, 10, 11, 12].map(i => `日志消息-${i} campus status ok`),
  );

  // Calling ensureLoaded again after loaded is a no-op.
  await controller.ensureLoaded();
  assert.equal(loaderCalls, 1);

  // 2. clear() called WHILE ensureLoaded() is in flight discards stale resolved history while keeping post-clear logs.
  let resolveStale;
  const inFlightController = new VirtualLogsController({
    loadHistory: () => new Promise(resolve => {
      resolveStale = resolve;
    }),
  });
  const pendingLoad = inFlightController.ensureLoaded();
  inFlightController.append(makeEntry(1));
  inFlightController.clear();
  inFlightController.append(makeEntry(99));

  resolveStale([makeEntry(0), makeEntry(1), makeEntry(2)]);
  await pendingLoad;

  assert.deepEqual(
    inFlightController.getFilteredEntries().map(e => e.message),
    ['日志消息-99 campus status ok'],
  );

  // 3. clear() called BEFORE ensureLoaded() never fetches/resurrects stale disk history.
  let preClearedLoaderCalled = false;
  const preClearedController = new VirtualLogsController({
    loadHistory: async () => {
      preClearedLoaderCalled = true;
      return [makeEntry(1), makeEntry(2)];
    },
  });
  preClearedController.clear();
  await preClearedController.ensureLoaded();
  assert.equal(preClearedLoaderCalled, false);
  assert.equal(preClearedController.getStateSummary().totalCount, 0);
});

test('VirtualLogsController search debounces at configured interval while filters & clear apply immediately', async () => {
  const controller = new VirtualLogsController({ searchDebounceMs: 40 });
  controller.setEntries([
    makeEntry(1, { module: '认证', message: 'alpha login ok', type: 'success' }),
    makeEntry(2, { module: '网络', message: 'beta probe fail', type: 'error' }),
    makeEntry(3, { module: '计费', message: 'alpha balance', type: 'info' }),
  ]);
  assert.equal(controller.getCountBadgeText(), '3 条');

  controller.setQuery('alpha');
  // Still 3 immediately before 40ms debounce fires:
  assert.equal(controller.getStateSummary().filteredCount, 3);
  await new Promise(resolve => setTimeout(resolve, 60));
  assert.equal(controller.getStateSummary().filteredCount, 2);
  assert.equal(controller.getCountBadgeText(), '2 / 3 条');

  // Level & module filters update synchronously without debounce:
  controller.setLevel('success');
  assert.equal(controller.getStateSummary().filteredCount, 1);
  controller.setLevel('all');
  controller.filterByModule('网络');
  assert.equal(controller.getStateSummary().filteredCount, 1);
  assert.match(controller.formatFilteredLogsText(), /\[网络\] beta probe fail/);
  controller.filterByModule('');
  assert.equal(controller.getStateSummary().filteredCount, 3);

  controller.clear();
  assert.equal(controller.getStateSummary().totalCount, 0);
  assert.equal(controller.getCountBadgeText(), '0 条');
  controller.destroy();
});

test('VirtualLogsController mounts <= 70 DOM rows out of 5,000, updates scroll capsule visibility, and empties DOM at 0 rows', () => {
  function createMockElement(tagName = 'div') {
    const listeners = new Map();
    const classes = new Set();
    const el = {
      tagName: tagName.toUpperCase(),
      children: [],
      parentElement: null,
      ownerDocument: null,
      style: {},
      dataset: {},
      hidden: false,
      textContent: '',
      clientWidth: 680,
      clientHeight: 560,
      scrollTop: 0,
      _attributes: new Map(),
      get className() {
        return Array.from(classes).join(' ');
      },
      set className(val) {
        classes.clear();
        for (const cls of String(val).split(/\s+/).filter(Boolean)) classes.add(cls);
      },
      classList: {
        add: (...names) => names.forEach(n => classes.add(n)),
        remove: (...names) => names.forEach(n => classes.delete(n)),
        contains: name => classes.has(name),
        toggle: (name, force) => {
          if (force === undefined ? !classes.has(name) : force) classes.add(name);
          else classes.delete(name);
        },
      },
      setAttribute: (k, v) => el._attributes.set(k, String(v)),
      getAttribute: k => el._attributes.get(k) ?? null,
      addEventListener: (type, fn) => {
        if (!listeners.has(type)) listeners.set(type, new Set());
        listeners.get(type).add(fn);
      },
      removeEventListener: (type, fn) => {
        listeners.get(type)?.delete(fn);
      },
      dispatchEvent: (event) => {
        const set = listeners.get(event.type);
        if (set) {
          for (const fn of set) fn(event);
        }
        return true;
      },
      get childElementCount() {
        return el.children.length;
      },
      get firstElementChild() {
        return el.children[0] ?? null;
      },
      get lastElementChild() {
        return el.children[el.children.length - 1] ?? null;
      },
      get nextElementSibling() {
        if (!el.parentElement) return null;
        const idx = el.parentElement.children.indexOf(el);
        return idx >= 0 ? (el.parentElement.children[idx + 1] ?? null) : null;
      },
      remove() {
        if (!el.parentElement) return;
        const idx = el.parentElement.children.indexOf(el);
        if (idx >= 0) el.parentElement.children.splice(idx, 1);
        el.parentElement = null;
      },
      append(...nodes) {
        for (const node of nodes) {
          node.remove();
          node.parentElement = el;
          el.children.push(node);
        }
      },
      prepend(...nodes) {
        for (let i = nodes.length - 1; i >= 0; i -= 1) {
          const node = nodes[i];
          node.remove();
          node.parentElement = el;
          el.children.unshift(node);
        }
      },
      insertBefore(node, refNode) {
        if (!refNode) {
          el.append(node);
          return node;
        }
        node.remove();
        const idx = el.children.indexOf(refNode);
        node.parentElement = el;
        if (idx >= 0) el.children.splice(idx, 0, node);
        else el.children.push(node);
        return node;
      },
      replaceChildren(...nodes) {
        for (const child of el.children) child.parentElement = null;
        el.children = [];
        el.append(...nodes);
      },
      getBoundingClientRect() {
        if (el.classList.contains('virtual-logs-spacer')) {
          return { height: Number.parseFloat(el.style.height) || 0 };
        }
        return { height: 28 };
      },
      get offsetHeight() {
        return el.getBoundingClientRect().height;
      },
      get scrollHeight() {
        if (el.children.length === 0) return 0;
        return el.children.reduce((sum, child) => sum + child.scrollHeight, 0)
          || el.getBoundingClientRect().height;
      },
      scrollTo({ top }) {
        el.scrollTop = top;
      },
    };
    return el;
  }

  const mockDoc = {
    createElement(tag) {
      const node = createMockElement(tag);
      node.ownerDocument = mockDoc;
      return node;
    },
  };
  const scroller = createMockElement('div');
  scroller.ownerDocument = mockDoc;
  const logsContent = createMockElement('div');
  logsContent.ownerDocument = mockDoc;
  scroller.append(logsContent);
  Object.defineProperty(scroller, 'scrollHeight', {
    get() {
      return logsContent.children.reduce((sum, child) => sum + child.getBoundingClientRect().height, 0);
    },
  });

  const scrollButton = createMockElement('button');
  scrollButton.hidden = true;
  const countBadge = createMockElement('span');

  const controller = new VirtualLogsController({ overscan: 12 });
  controller.setup({
    logsContent,
    scroller,
    scrollButton,
    filterCountElement: countBadge,
  });

  const entries = Array.from({ length: 5000 }, (_, i) =>
    i === 4990
      ? makeEntry(i, { module: '系统', message: `${SESSION_START_MARKER} now` })
      : makeEntry(i));
  controller.setEntries(entries);
  controller.setActive(true);

  const rowNodes = logsContent.children.filter(c => c.classList.contains('log-entry'));
  assert.ok(rowNodes.length > 0 && rowNodes.length <= 70, `expected <= 70 mounted rows, got ${rowNodes.length}`);
  assert.equal(countBadge.textContent, '5000 条');
  assert.equal(scrollButton.hidden, true, 'scroll capsule must stay hidden when pinned at bottom');

  const dividerNode = rowNodes.find(r => r.dataset.logIndex === '4990');
  assert.ok(dividerNode?.classList.contains('log-session-divider'));
  assert.equal(dividerNode?.dataset.logModule, '系统');

  // Scroll far up from bottom (> 64px) and dispatch scroll event:
  scroller.scrollTop = 100;
  scroller.dispatchEvent({ type: 'scroll' });
  controller.syncScrollButton(true);
  assert.equal(scrollButton.hidden, false, 'scroll capsule must show when scrolled away from bottom');
  assert.equal(controller.getStateSummary().pinnedToBottom, false);

  // Clicking scrollToBottom returns cleanly to bottom and hides capsule:
  controller.scrollToBottom({ behavior: 'instant' });
  assert.equal(controller.getStateSummary().pinnedToBottom, true);
  assert.equal(scrollButton.hidden, true);

  // Empty filter result clears DOM completely so :empty::before works:
  controller.setQuery('definitely-missing-needle-xyz', { immediate: true });
  assert.equal(logsContent.childElementCount, 0);
  assert.equal(countBadge.textContent, '0 / 5000 条');
  assert.equal(scrollButton.hidden, true);

  controller.destroy();
});
