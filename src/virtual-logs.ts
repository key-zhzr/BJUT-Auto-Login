import type { AppLogEntry } from './models.ts';

export type { AppLogEntry };

export type LogLevelType = AppLogEntry['type'];
export type LogSessionFilter = 'all' | 'current';
export type LogLevelFilter = 'all' | LogLevelType;

export const SESSION_START_MARKER = '=== SESSION START ===';
export const DEFAULT_MAX_LOG_ENTRIES = 5000;
export const DEFAULT_SEARCH_DEBOUNCE_MS = 120;
export const DEFAULT_VIRTUAL_OVERSCAN = 12;
export const DEFAULT_PIN_THRESHOLD_PX = 48;
export const DEFAULT_SCROLL_FAB_THRESHOLD_PX = 64;

const MIN_ROW_HEIGHT_PX = 20;
const DEFAULT_CONTENT_WIDTH_PX = 680;
const RING_COMPACT_THRESHOLD = 2048;

export interface IndexedLogEntry extends AppLogEntry {
  readonly id: number;
  readonly searchText: string;
  readonly moduleLower: string;
  readonly isSessionStart: boolean;
  readonly displayMessage: string;
}

export interface VirtualRange {
  startIndex: number;
  endIndex: number;
  firstVisible: number;
  lastVisible: number;
  topOffset: number;
  bottomOffset: number;
  totalHeight: number;
}

export interface VirtualLogsStateSummary {
  totalCount: number;
  filteredCount: number;
  hasActiveFilter: boolean;
  hasActiveSearch: boolean;
  activeQuery: string;
  activeSession: LogSessionFilter;
  activeLevel: LogLevelFilter;
  activeModule: string;
  pinnedToBottom: boolean;
  distanceFromBottom: number;
}

export interface SelectControlLike {
  value: string;
  addEventListener: (
    event: 'change',
    callback: (event: { target?: { value?: string } } | Event) => void,
  ) => void;
  removeEventListener?: (
    event: 'change',
    callback: (event: { target?: { value?: string } } | Event) => void,
  ) => void;
}

export interface VirtualLogsModelOptions {
  maxEntries?: number;
  shouldAcceptEntry?: (entry: AppLogEntry) => boolean;
}

export interface VirtualLogsSetupOptions extends VirtualLogsModelOptions {
  logsContent?: HTMLElement | null;
  scroller?: HTMLElement | null;
  scrollButton?: HTMLElement | null;
  searchInput?: HTMLInputElement | null;
  sessionFilter?: SelectControlLike | HTMLSelectElement | null;
  levelFilter?: SelectControlLike | HTMLSelectElement | null;
  filterCountElement?: HTMLElement | null;
  loadHistory?: () => Promise<readonly AppLogEntry[]>;
  searchDebounceMs?: number;
  overscan?: number;
  pinThresholdPx?: number;
  showScrollButtonThresholdPx?: number;
  onStateChange?: (summary: VirtualLogsStateSummary) => void;
  onModuleFilter?: (module: string) => void;
}

export interface AppendLogInput {
  time?: string;
  module: string;
  message: string;
  type: LogLevelType;
}

interface RowHeightMetrics {
  charWidthPx: number;
  lineHeightPx: number;
  rowPaddingPx: number;
  dividerExtraPx: number;
  horizontalPaddingPx: number;
}

const DEFAULT_ROW_METRICS: RowHeightMetrics = {
  charWidthPx: 7.9,
  lineHeightPx: 20.4,
  rowPaddingPx: 14,
  dividerExtraPx: 12,
  horizontalPaddingPx: 16,
};

function normalizeLogInput(entry: AppendLogInput): AppLogEntry {
  return {
    time: entry.time || new Date().toLocaleString(),
    module: entry.module,
    message: entry.message,
    type: entry.type,
  };
}

function indexLogEntry(entry: AppLogEntry, id: number): IndexedLogEntry {
  const isSessionStart = entry.message.includes(SESSION_START_MARKER);
  const displayMessage = isSessionStart
    ? entry.message.replace(SESSION_START_MARKER, '启动会话')
    : entry.message;
  return {
    id,
    time: entry.time,
    module: entry.module,
    message: entry.message,
    type: entry.type,
    searchText: `${entry.time} ${entry.module} ${entry.message}`.toLowerCase(),
    moduleLower: entry.module.toLowerCase(),
    isSessionStart,
    displayMessage,
  };
}

function entriesBoundaryEqual(a: AppLogEntry, b: AppLogEntry): boolean {
  return a.time === b.time
    && a.module === b.module
    && a.message === b.message
    && a.type === b.type;
}

/**
 * Merges a backend snapshot (`initialLogs`) with live entries buffered before or
 * during the snapshot load, deduplicating boundary overlap in O(min(N, P)) time
 * via KMP prefix matching so duplicate identical live entries are never dropped.
 */
export function mergeLogSnapshots(
  initialLogs: readonly AppLogEntry[],
  pendingLiveEntries: readonly AppLogEntry[],
  maxEntries = DEFAULT_MAX_LOG_ENTRIES,
): AppLogEntry[] {
  const cap = Math.max(1, maxEntries);
  if (pendingLiveEntries.length === 0) {
    return initialLogs.length > cap
      ? initialLogs.slice(initialLogs.length - cap)
      : initialLogs.slice();
  }
  if (initialLogs.length === 0) {
    return pendingLiveEntries.length > cap
      ? pendingLiveEntries.slice(pendingLiveEntries.length - cap)
      : pendingLiveEntries.slice();
  }

  const m = pendingLiveEntries.length;
  const pi = new Int32Array(m);
  for (let i = 1; i < m; i += 1) {
    let q = pi[i - 1];
    while (q > 0 && !entriesBoundaryEqual(pendingLiveEntries[q], pendingLiveEntries[i])) {
      q = pi[q - 1];
    }
    if (entriesBoundaryEqual(pendingLiveEntries[q], pendingLiveEntries[i])) {
      q += 1;
    }
    pi[i] = q;
  }

  let matched = 0;
  const scanStart = Math.max(0, initialLogs.length - m);
  for (let i = scanStart; i < initialLogs.length; i += 1) {
    while (
      matched > 0
      && (matched === m || !entriesBoundaryEqual(pendingLiveEntries[matched], initialLogs[i]))
    ) {
      matched = pi[matched - 1];
    }
    if (matched < m && entriesBoundaryEqual(pendingLiveEntries[matched], initialLogs[i])) {
      matched += 1;
    }
  }

  const tailCount = m - matched;
  const total = initialLogs.length + tailCount;
  const skipFromInitial = Math.max(0, total - cap);
  const merged: AppLogEntry[] = [];

  for (let i = skipFromInitial; i < initialLogs.length; i += 1) {
    merged.push(initialLogs[i]);
  }
  const pendingStart = matched + Math.max(0, skipFromInitial - initialLogs.length);
  for (let i = pendingStart; i < pendingLiveEntries.length; i += 1) {
    merged.push(pendingLiveEntries[i]);
  }
  return merged;
}

export function estimateLogEntryHeight(
  entry: Pick<IndexedLogEntry, 'time' | 'module' | 'displayMessage' | 'isSessionStart'>,
  contentWidthPx: number,
  metrics: RowHeightMetrics = DEFAULT_ROW_METRICS,
): number {
  const usableWidth = Math.max(120, contentWidthPx - metrics.horizontalPaddingPx);
  const charsPerLine = Math.max(18, Math.floor(usableWidth / metrics.charWidthPx));
  const prefixChars = entry.time.length + entry.module.length + 6;
  const text = entry.displayMessage;

  let lines = 0;
  let segmentStart = 0;
  let isFirstSegment = true;
  for (let i = 0; i <= text.length; i += 1) {
    if (i === text.length || text.charCodeAt(i) === 10) {
      const segmentLen = (i - segmentStart) + (isFirstSegment ? prefixChars : 0);
      lines += Math.max(1, Math.ceil(segmentLen / charsPerLine));
      segmentStart = i + 1;
      isFirstSegment = false;
    }
  }
  const extra = entry.isSessionStart ? metrics.dividerExtraPx : 0;
  return Math.max(
    MIN_ROW_HEIGHT_PX,
    Math.ceil(metrics.rowPaddingPx + extra + lines * metrics.lineHeightPx),
  );
}

/**
 * Computes the virtualized window `[startIndex .. endIndex]` and spacer offsets
 * from a prefix-sum height array (`length >= totalCount + 1`).
 */
export function computeVirtualRange(
  prefixSums: ArrayLike<number>,
  totalCount: number,
  scrollTop: number,
  viewportHeight: number,
  overscan = DEFAULT_VIRTUAL_OVERSCAN,
): VirtualRange {
  if (totalCount <= 0) {
    return {
      startIndex: 0,
      endIndex: -1,
      firstVisible: 0,
      lastVisible: -1,
      topOffset: 0,
      bottomOffset: 0,
      totalHeight: 0,
    };
  }

  const totalHeight = Math.max(0, prefixSums[totalCount] ?? 0);
  const safeViewport = Math.max(1, viewportHeight);
  const maxScroll = Math.max(0, totalHeight - safeViewport);
  const clampedTop = Math.max(0, Math.min(scrollTop, maxScroll));
  const viewportBottom = clampedTop + safeViewport;

  // Binary search for first entry whose bottom edge extends past clampedTop.
  let low = 0;
  let high = totalCount - 1;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if ((prefixSums[mid + 1] ?? 0) > clampedTop) {
      high = mid;
    } else {
      low = mid + 1;
    }
  }
  const firstVisible = low;

  // Binary search for first entry whose bottom edge reaches viewportBottom.
  let bLow = firstVisible;
  let bHigh = totalCount - 1;
  while (bLow < bHigh) {
    const mid = (bLow + bHigh) >>> 1;
    if ((prefixSums[mid + 1] ?? 0) >= viewportBottom) {
      bHigh = mid;
    } else {
      bLow = mid + 1;
    }
  }
  const lastVisible = bLow;

  const safeOverscan = Math.max(0, overscan);
  const startIndex = Math.max(0, firstVisible - safeOverscan);
  const endIndex = Math.min(totalCount - 1, lastVisible + safeOverscan);
  const topOffset = Math.max(0, prefixSums[startIndex] ?? 0);
  const bottomEdge = prefixSums[endIndex + 1] ?? totalHeight;
  const bottomOffset = Math.max(0, totalHeight - bottomEdge);

  return {
    startIndex,
    endIndex,
    firstVisible,
    lastVisible,
    topOffset,
    bottomOffset,
    totalHeight,
  };
}

/**
 * Pure DOM-free state, pre-indexed cache, race-free history merger, and height prefix-sum store.
 * Exported directly so Node unit & performance tests can exercise every invariant without a DOM.
 */
export class VirtualLogModel {
  readonly maxEntries: number;
  private readonly shouldAcceptEntry?: (entry: AppLogEntry) => boolean;

  private ring: IndexedLogEntry[] = [];
  private head = 0;
  private nextEntryId = 1;
  private lastSessionStartId: number | null = null;
  private lastSessionStartIndex = -1;

  private sessionFilter: LogSessionFilter = 'all';
  private levelFilter: LogLevelFilter = 'all';
  private query = '';
  private queryLower = '';
  private moduleFilter = '';

  private filteredEntries: IndexedLogEntry[] = [];
  private pendingLiveEntries: AppLogEntry[] = [];
  private historyState: 'idle' | 'loading' | 'loaded' = 'idle';
  private clearedEpoch = 0;
  private inFlightLoad: Promise<boolean> | null = null;

  private measuredHeights = new Map<number, number>();
  private prefixSums = new Float64Array(1);
  private prefixWidth = DEFAULT_CONTENT_WIDTH_PX;
  private prefixDirtyFrom = 0;
  private rowMetrics: RowHeightMetrics = { ...DEFAULT_ROW_METRICS };

  constructor(options: VirtualLogsModelOptions = {}) {
    this.maxEntries = Math.max(1, options.maxEntries ?? DEFAULT_MAX_LOG_ENTRIES);
    this.shouldAcceptEntry = options.shouldAcceptEntry;
  }

  get totalCount(): number {
    return this.ring.length - this.head;
  }

  get filteredCount(): number {
    return this.filteredEntries.length;
  }

  getLastSessionStartIndex(): number {
    return this.lastSessionStartIndex;
  }

  getClearedEpoch(): number {
    return this.clearedEpoch;
  }

  getHistoryState(): 'idle' | 'loading' | 'loaded' {
    return this.historyState;
  }

  setRowMetrics(metrics: Partial<RowHeightMetrics>): void {
    this.rowMetrics = { ...this.rowMetrics, ...metrics };
    this.prefixDirtyFrom = 0;
  }

  private compactRingIfNeeded(): void {
    if (this.head >= RING_COMPACT_THRESHOLD || this.head >= this.maxEntries) {
      this.ring = this.ring.slice(this.head);
      this.head = 0;
    }
  }

  private matchesFilters(entry: IndexedLogEntry, logicalIndex?: number): boolean {
    if (
      this.sessionFilter === 'current'
      && this.lastSessionStartIndex >= 0
      && logicalIndex !== undefined
      && logicalIndex < this.lastSessionStartIndex
    ) {
      return false;
    }
    if (this.levelFilter !== 'all' && entry.type !== this.levelFilter) {
      return false;
    }
    if (this.moduleFilter && entry.module !== this.moduleFilter) {
      return false;
    }
    if (this.queryLower && !entry.searchText.includes(this.queryLower)) {
      return false;
    }
    return true;
  }

  private rebuildFilteredEntries(): void {
    const total = this.totalCount;
    const startLogical = this.sessionFilter === 'current' && this.lastSessionStartIndex >= 0
      ? this.lastSessionStartIndex
      : 0;

    if (
      this.levelFilter === 'all'
      && !this.moduleFilter
      && !this.queryLower
    ) {
      this.filteredEntries = startLogical === 0 && this.head === 0
        ? this.ring.slice()
        : this.ring.slice(this.head + startLogical);
    } else {
      const next: IndexedLogEntry[] = [];
      for (let i = startLogical; i < total; i += 1) {
        const entry = this.ring[this.head + i];
        if (this.levelFilter !== 'all' && entry.type !== this.levelFilter) continue;
        if (this.moduleFilter && entry.module !== this.moduleFilter) continue;
        if (this.queryLower && !entry.searchText.includes(this.queryLower)) continue;
        next.push(entry);
      }
      this.filteredEntries = next;
    }
    this.prefixDirtyFrom = 0;
  }

  private replaceRingFromRaw(entries: readonly AppLogEntry[]): void {
    const start = entries.length > this.maxEntries ? entries.length - this.maxEntries : 0;
    const count = entries.length - start;
    const nextRing = new Array<IndexedLogEntry>(count);
    let lastSessionId: number | null = null;
    let lastSessionIdx = -1;

    for (let i = 0; i < count; i += 1) {
      const raw = normalizeLogInput(entries[start + i]);
      const indexed = indexLogEntry(raw, this.nextEntryId++);
      nextRing[i] = indexed;
      if (indexed.isSessionStart) {
        lastSessionId = indexed.id;
        lastSessionIdx = i;
      }
    }

    this.ring = nextRing;
    this.head = 0;
    this.lastSessionStartId = lastSessionId;
    this.lastSessionStartIndex = lastSessionIdx;
    this.measuredHeights.clear();
    this.rebuildFilteredEntries();
  }

  setEntries(entries: readonly AppLogEntry[]): void {
    this.historyState = 'loaded';
    this.pendingLiveEntries = [];
    this.inFlightLoad = null;
    this.replaceRingFromRaw(entries);
  }

  append(input: AppendLogInput): {
    accepted: boolean;
    indexed: IndexedLogEntry | null;
    evicted: IndexedLogEntry | null;
    visibleChanged: boolean;
  } {
    const normalized = normalizeLogInput(input);
    if (this.shouldAcceptEntry && !this.shouldAcceptEntry(normalized)) {
      return { accepted: false, indexed: null, evicted: null, visibleChanged: false };
    }

    if (this.historyState !== 'loaded') {
      this.pendingLiveEntries.push(normalized);
      if (this.pendingLiveEntries.length > this.maxEntries) {
        this.pendingLiveEntries.splice(0, this.pendingLiveEntries.length - this.maxEntries);
      }
    }

    const indexed = indexLogEntry(normalized, this.nextEntryId++);
    const prevLength = this.totalCount;
    this.ring.push(indexed);

    if (indexed.isSessionStart) {
      this.lastSessionStartId = indexed.id;
      this.lastSessionStartIndex = prevLength;
    }

    let evicted: IndexedLogEntry | null = null;
    while (this.totalCount > this.maxEntries) {
      evicted = this.ring[this.head];
      (this.ring[this.head] as unknown) = undefined;
      this.head += 1;
      this.measuredHeights.delete(evicted.id);
      if (evicted.id === this.lastSessionStartId) {
        this.lastSessionStartId = null;
        this.lastSessionStartIndex = -1;
      } else if (this.lastSessionStartIndex > 0) {
        this.lastSessionStartIndex -= 1;
      }
    }
    this.compactRingIfNeeded();

    let visibleChanged = false;
    if (indexed.isSessionStart && this.sessionFilter === 'current') {
      const hadEntries = this.filteredEntries.length > 0;
      this.filteredEntries = this.matchesFilters(indexed) ? [indexed] : [];
      this.prefixDirtyFrom = 0;
      visibleChanged = hadEntries || this.filteredEntries.length > 0;
    } else {
      if (evicted && this.filteredEntries.length > 0 && this.filteredEntries[0].id === evicted.id) {
        this.filteredEntries.shift();
        this.prefixDirtyFrom = 0;
        visibleChanged = true;
      }
      if (this.matchesFilters(indexed)) {
        const prevFilteredLen = this.filteredEntries.length;
        this.filteredEntries.push(indexed);
        this.prefixDirtyFrom = Math.min(this.prefixDirtyFrom, prevFilteredLen);
        visibleChanged = true;
      }
    }

    return { accepted: true, indexed, evicted, visibleChanged };
  }

  /**
   * Race-free single-flight history loader.
   * - If `clear()` was called before loading began, refuses to resurrect stale history.
   * - If `clear()` is called while loading is in flight, discards resolved history cleanly.
   * - Otherwise merges `initialLogs` + `pendingLiveEntries` without losing or duplicating live logs.
   */
  async ensureLoaded(loader: () => Promise<readonly AppLogEntry[]>): Promise<boolean> {
    if (this.clearedEpoch > 0 || this.historyState === 'loaded') {
      return false;
    }
    if (this.inFlightLoad) {
      return this.inFlightLoad;
    }

    const startEpoch = this.clearedEpoch;
    this.historyState = 'loading';

    let task!: Promise<boolean>;
    task = (async (): Promise<boolean> => {
      try {
        const initialLogs = await loader();
        if (this.clearedEpoch !== startEpoch) {
          return false;
        }
        const acceptedInitial = this.shouldAcceptEntry
          ? initialLogs.filter(this.shouldAcceptEntry)
          : initialLogs;
        const merged = mergeLogSnapshots(
          acceptedInitial,
          this.pendingLiveEntries,
          this.maxEntries,
        );
        this.pendingLiveEntries = [];
        this.historyState = 'loaded';
        this.replaceRingFromRaw(merged);
        return true;
      } catch (error) {
        if (this.clearedEpoch === startEpoch) {
          this.historyState = 'idle';
        }
        throw error;
      } finally {
        if (this.inFlightLoad === task) {
          this.inFlightLoad = null;
        }
      }
    })();

    this.inFlightLoad = task;
    return task;
  }

  clear(): void {
    this.clearedEpoch += 1;
    this.inFlightLoad = null;
    this.historyState = 'loaded';
    this.pendingLiveEntries = [];
    this.ring = [];
    this.head = 0;
    this.lastSessionStartId = null;
    this.lastSessionStartIndex = -1;
    this.filteredEntries = [];
    this.measuredHeights.clear();
    this.prefixSums = new Float64Array(1);
    this.prefixDirtyFrom = 0;
  }

  setQuery(query: string): boolean {
    const normalized = query.trim();
    const lower = normalized.toLowerCase();
    if (normalized === this.query && lower === this.queryLower) {
      return false;
    }
    this.query = normalized;
    this.queryLower = lower;
    this.rebuildFilteredEntries();
    return true;
  }

  setSession(session: LogSessionFilter): boolean {
    if (session === this.sessionFilter) return false;
    this.sessionFilter = session;
    this.rebuildFilteredEntries();
    return true;
  }

  setLevel(level: LogLevelFilter): boolean {
    if (level === this.levelFilter) return false;
    this.levelFilter = level;
    this.rebuildFilteredEntries();
    return true;
  }

  setModuleFilter(module: string): boolean {
    const normalized = module.trim();
    if (normalized === this.moduleFilter) return false;
    this.moduleFilter = normalized;
    this.rebuildFilteredEntries();
    return true;
  }

  getEntries(): readonly IndexedLogEntry[] {
    return this.head === 0 ? this.ring.slice() : this.ring.slice(this.head);
  }

  getFilteredEntries(): readonly IndexedLogEntry[] {
    return this.filteredEntries;
  }

  getFilteredEntry(filteredIndex: number): IndexedLogEntry | null {
    if (filteredIndex < 0 || filteredIndex >= this.filteredEntries.length) {
      return null;
    }
    return this.filteredEntries[filteredIndex];
  }

  formatFilteredLogsText(): string {
    if (this.filteredEntries.length === 0) return '';
    const lines = new Array<string>(this.filteredEntries.length);
    for (let i = 0; i < this.filteredEntries.length; i += 1) {
      const item = this.filteredEntries[i];
      lines[i] = `[${item.time}] [${item.module}] ${item.displayMessage}`;
    }
    return lines.join('\n');
  }

  invalidateMeasuredHeights(): void {
    if (this.measuredHeights.size === 0) return;
    this.measuredHeights.clear();
    this.prefixDirtyFrom = 0;
  }

  setMeasuredHeight(entryId: number, height: number, filteredIndexHint?: number): boolean {
    if (!Number.isFinite(height) || height <= 0) return false;
    const clamped = Math.max(MIN_ROW_HEIGHT_PX, Math.round(height * 10) / 10);
    const prev = this.measuredHeights.get(entryId);
    if (prev !== undefined && Math.abs(prev - clamped) < 0.25) {
      return false;
    }
    this.measuredHeights.set(entryId, clamped);
    if (filteredIndexHint !== undefined && filteredIndexHint >= 0) {
      this.prefixDirtyFrom = Math.min(this.prefixDirtyFrom, filteredIndexHint);
    } else {
      this.prefixDirtyFrom = 0;
    }
    return true;
  }

  getPrefixSums(contentWidthPx: number): Float64Array {
    const safeWidth = Math.max(120, Math.round(contentWidthPx || DEFAULT_CONTENT_WIDTH_PX));
    if (Math.abs(safeWidth - this.prefixWidth) > 1) {
      this.prefixWidth = safeWidth;
      this.measuredHeights.clear();
      this.prefixDirtyFrom = 0;
    }

    const count = this.filteredEntries.length;
    const neededLength = count + 1;
    if (this.prefixSums.length < neededLength) {
      const grown = new Float64Array(neededLength);
      const preserveCount = Math.min(this.prefixSums.length, this.prefixDirtyFrom + 1);
      if (preserveCount > 0) {
        grown.set(this.prefixSums.subarray(0, preserveCount), 0);
      }
      this.prefixSums = grown;
    }

    if (count === 0) {
      this.prefixSums[0] = 0;
      this.prefixDirtyFrom = 0;
      return this.prefixSums;
    }

    const start = Math.max(0, Math.min(this.prefixDirtyFrom, count));
    if (start === 0) {
      this.prefixSums[0] = 0;
    }
    for (let i = start; i < count; i += 1) {
      const entry = this.filteredEntries[i];
      const h = this.measuredHeights.get(entry.id)
        ?? estimateLogEntryHeight(entry, safeWidth, this.rowMetrics);
      this.prefixSums[i + 1] = this.prefixSums[i] + h;
    }
    this.prefixDirtyFrom = count;
    return this.prefixSums;
  }

  computeRange(
    scrollTop: number,
    viewportHeight: number,
    contentWidthPx: number,
    overscan = DEFAULT_VIRTUAL_OVERSCAN,
  ): VirtualRange {
    const count = this.filteredEntries.length;
    const prefix = this.getPrefixSums(contentWidthPx);
    return computeVirtualRange(prefix, count, scrollTop, viewportHeight, overscan);
  }

  getAnchorForScrollTop(
    scrollTop: number,
    contentWidthPx: number,
  ): { index: number; offset: number } | null {
    const count = this.filteredEntries.length;
    if (count === 0) return null;
    const prefix = this.getPrefixSums(contentWidthPx);
    const total = prefix[count];
    const clampedTop = Math.max(0, Math.min(scrollTop, Math.max(0, total - 1)));
    let low = 0;
    let high = count - 1;
    while (low < high) {
      const mid = (low + high) >>> 1;
      if (prefix[mid + 1] > clampedTop) {
        high = mid;
      } else {
        low = mid + 1;
      }
    }
    return {
      index: low,
      offset: clampedTop - prefix[low],
    };
  }

  getScrollTopForAnchor(
    anchor: { index: number; offset: number },
    contentWidthPx: number,
  ): number {
    const count = this.filteredEntries.length;
    if (count === 0) return 0;
    const prefix = this.getPrefixSums(contentWidthPx);
    const idx = Math.max(0, Math.min(anchor.index, count - 1));
    return Math.max(0, prefix[idx] + anchor.offset);
  }

  getSummary(pinnedToBottom = true, distanceFromBottom = 0): VirtualLogsStateSummary {
    return {
      totalCount: this.totalCount,
      filteredCount: this.filteredEntries.length,
      hasActiveFilter:
        this.sessionFilter !== 'all'
        || this.levelFilter !== 'all'
        || this.moduleFilter !== '',
      hasActiveSearch: this.query !== '',
      activeQuery: this.query,
      activeSession: this.sessionFilter,
      activeLevel: this.levelFilter,
      activeModule: this.moduleFilter,
      pinnedToBottom,
      distanceFromBottom,
    };
  }
}

function normalizeSessionValue(value: string | undefined | null): LogSessionFilter {
  return value === 'current' ? 'current' : 'all';
}

function normalizeLevelValue(value: string | undefined | null): LogLevelFilter {
  if (value === 'error' || value === 'success' || value === 'info' || value === 'debug') {
    return value;
  }
  return 'all';
}

/**
 * Cross-platform DOM virtual list controller for `#logs`.
 * Works on Desktop (macOS/Windows/Linux) and Android WebView with:
 * - 5,000-entry O(1) ring cache + pre-indexed filtering + 120ms debounced search
 * - Race-free single-flight lazy loading + live event boundary merge + clear() guard
 * - Variable-height virtual DOM spacers + ResizeObserver measurement + zero-jitter anchoring
 * - Coalesced RAF batch rendering + sticky bottom auto-scroll + floating `#btn-scroll-logs` capsule
 */
export class VirtualLogsController {
  private readonly model: VirtualLogModel;
  private options: VirtualLogsSetupOptions;

  private logsContent: HTMLElement | null = null;
  private scroller: HTMLElement | null = null;
  private scrollButton: HTMLElement | null = null;
  private searchInput: HTMLInputElement | null = null;
  private sessionFilter: SelectControlLike | HTMLSelectElement | null = null;
  private levelFilter: SelectControlLike | HTMLSelectElement | null = null;
  private filterCountElement: HTMLElement | null = null;

  private topSpacer: HTMLDivElement | null = null;
  private bottomSpacer: HTMLDivElement | null = null;
  private mountedRows = new Map<number, HTMLDivElement>();
  private recycledPool: HTMLDivElement[] = [];

  private searchDebounceMs: number;
  private overscan: number;
  private pinThresholdPx: number;
  private showScrollButtonThresholdPx: number;

  private active = false;
  private dirty = false;
  private destroyed = false;
  private pinnedToBottom = true;
  private needsInitialBottomSnap = true;
  private lastObservedScrollTop = 0;
  private lastContentWidth = DEFAULT_CONTENT_WIDTH_PX;
  private lastViewportHeight = 520;
  private lastUserScrollIntentAt = 0;
  private programmaticScrollUntil = 0;
  private lastRenderedSignature = '';
  private lastRangeStart = 0;
  private lastRangeEnd = -1;

  private renderFrame: number | null = null;
  private scrollSyncFrame: number | null = null;
  private searchTimer: number | null = null;
  private scrollSettleTimer: number | null = null;
  private fabHideTimer: number | null = null;
  private fabVisibleState: boolean | null = null;

  private containerResizeObserver: ResizeObserver | null = null;
  private rowResizeObserver: ResizeObserver | null = null;

  private cleanupCallbacks: (() => void)[] = [];

  constructor(options: VirtualLogsSetupOptions = {}) {
    this.options = { ...options };
    this.model = new VirtualLogModel({
      maxEntries: options.maxEntries,
      shouldAcceptEntry: options.shouldAcceptEntry,
    });
    this.searchDebounceMs = options.searchDebounceMs ?? DEFAULT_SEARCH_DEBOUNCE_MS;
    this.overscan = options.overscan ?? DEFAULT_VIRTUAL_OVERSCAN;
    this.pinThresholdPx = options.pinThresholdPx ?? DEFAULT_PIN_THRESHOLD_PX;
    this.showScrollButtonThresholdPx = options.showScrollButtonThresholdPx ?? DEFAULT_SCROLL_FAB_THRESHOLD_PX;
  }

  setup(options: VirtualLogsSetupOptions = {}): void {
    this.teardownDomBindings();
    this.destroyed = false;
    this.options = { ...this.options, ...options };
    this.searchDebounceMs = this.options.searchDebounceMs ?? DEFAULT_SEARCH_DEBOUNCE_MS;
    this.overscan = this.options.overscan ?? DEFAULT_VIRTUAL_OVERSCAN;
    this.pinThresholdPx = this.options.pinThresholdPx ?? DEFAULT_PIN_THRESHOLD_PX;
    this.showScrollButtonThresholdPx = this.options.showScrollButtonThresholdPx ?? DEFAULT_SCROLL_FAB_THRESHOLD_PX;

    const doc = typeof document !== 'undefined' ? document : null;
    this.logsContent = this.options.logsContent ?? doc?.getElementById('logs-content') ?? null;
    this.scroller = this.options.scroller
      ?? this.logsContent?.closest<HTMLElement>('.logs-container')
      ?? this.logsContent?.parentElement
      ?? null;
    this.scrollButton = this.options.scrollButton ?? doc?.getElementById('btn-scroll-logs') ?? null;
    this.searchInput = this.options.searchInput ?? (doc?.getElementById('log-search') as HTMLInputElement | null) ?? null;
    this.sessionFilter = this.options.sessionFilter ?? null;
    this.levelFilter = this.options.levelFilter ?? null;
    this.filterCountElement = this.options.filterCountElement ?? doc?.getElementById('log-filter-count') ?? null;

    if (this.searchInput) {
      this.model.setQuery(this.searchInput.value || '');
    }
    if (this.sessionFilter) {
      this.model.setSession(normalizeSessionValue(this.sessionFilter.value));
    }
    if (this.levelFilter) {
      this.model.setLevel(normalizeLevelValue(this.levelFilter.value));
    }

    this.bindEvents();
    this.readRowTypographyMetrics();
    this.updateViewportMetrics();
    this.updateCountBadge();
    this.syncScrollButton(true);
    this.notifyStateChange();

    if (this.active) {
      this.dirty = true;
      this.renderNow();
    }
  }

  private now(): number {
    return typeof performance !== 'undefined' ? performance.now() : Date.now();
  }

  private scheduleRaf(cb: () => void): number {
    if (typeof requestAnimationFrame === 'function') {
      return requestAnimationFrame(cb);
    }
    return setTimeout(cb, 0) as unknown as number;
  }

  private cancelRaf(id: number | null): void {
    if (id === null) return;
    if (typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(id);
    } else {
      clearTimeout(id);
    }
  }

  private setTimeoutSafe(cb: () => void, ms: number): number {
    if (typeof window !== 'undefined' && typeof window.setTimeout === 'function') {
      return window.setTimeout(cb, ms);
    }
    return setTimeout(cb, ms) as unknown as number;
  }

  private clearTimeoutSafe(id: number | null): void {
    if (id === null) return;
    if (typeof window !== 'undefined' && typeof window.clearTimeout === 'function') {
      window.clearTimeout(id);
    } else {
      clearTimeout(id);
    }
  }

  private readRowTypographyMetrics(): void {
    if (!this.logsContent || typeof getComputedStyle !== 'function') return;
    const style = getComputedStyle(this.logsContent);
    const fontSize = Number.parseFloat(style.fontSize);
    const lineHeightRaw = Number.parseFloat(style.lineHeight);
    if (Number.isFinite(fontSize) && fontSize > 0) {
      const lineHeightPx = Number.isFinite(lineHeightRaw) && lineHeightRaw > 0
        ? lineHeightRaw
        : fontSize * 1.5;
      this.model.setRowMetrics({
        charWidthPx: Math.max(5.5, fontSize * 0.6),
        lineHeightPx,
      });
    }
  }

  private updateViewportMetrics(): { widthChanged: boolean; heightChanged: boolean } {
    const contentWidth = this.logsContent?.clientWidth
      || this.scroller?.clientWidth
      || this.lastContentWidth
      || DEFAULT_CONTENT_WIDTH_PX;
    const viewportHeight = this.scroller?.clientHeight
      || this.lastViewportHeight
      || 520;

    const widthChanged = contentWidth > 0 && Math.abs(contentWidth - this.lastContentWidth) > 1;
    const heightChanged = viewportHeight > 0 && Math.abs(viewportHeight - this.lastViewportHeight) > 1;

    if (widthChanged) {
      this.lastContentWidth = contentWidth;
      this.model.invalidateMeasuredHeights();
    }
    if (viewportHeight > 0) {
      this.lastViewportHeight = viewportHeight;
    }
    return { widthChanged, heightChanged };
  }

  private bindEvents(): void {
    if (this.scroller) {
      const scroller = this.scroller;
      const onScroll = () => {
        if (this.destroyed) return;
        const currentTop = scroller.scrollTop;
        const distance = this.computeDistanceFromBottom();
        const now = this.now();
        const scrolledUp = currentTop < this.lastObservedScrollTop - 1.5;
        const userIntentRecent = this.lastUserScrollIntentAt > 0 && now - this.lastUserScrollIntentAt < 650;

        if (distance <= this.pinThresholdPx) {
          this.pinnedToBottom = true;
        } else if (scrolledUp || (now > this.programmaticScrollUntil && userIntentRecent)) {
          this.pinnedToBottom = false;
        }
        this.lastObservedScrollTop = currentTop;

        if (this.scrollSyncFrame === null) {
          this.scrollSyncFrame = this.scheduleRaf(() => {
            this.scrollSyncFrame = null;
            if (this.destroyed) return;
            this.syncScrollButton();
            if (this.active) {
              this.renderWindowOnly();
            }
          });
        }
      };

      const markUserIntent = () => {
        this.lastUserScrollIntentAt = this.now();
      };

      const onKeyDown = (event: KeyboardEvent) => {
        if (['ArrowUp', 'PageUp', 'Home', 'ArrowDown', 'PageDown', 'End', ' '].includes(event.key)) {
          markUserIntent();
        }
      };

      scroller.addEventListener('scroll', onScroll, { passive: true });
      scroller.addEventListener('wheel', markUserIntent, { passive: true });
      scroller.addEventListener('touchstart', markUserIntent, { passive: true });
      scroller.addEventListener('pointerdown', markUserIntent, { passive: true });
      scroller.addEventListener('keydown', onKeyDown, { passive: true });

      this.cleanupCallbacks.push(() => {
        scroller.removeEventListener('scroll', onScroll);
        scroller.removeEventListener('wheel', markUserIntent);
        scroller.removeEventListener('touchstart', markUserIntent);
        scroller.removeEventListener('pointerdown', markUserIntent);
        scroller.removeEventListener('keydown', onKeyDown);
      });
    }

    if (this.searchInput) {
      const input = this.searchInput;
      const onInput = () => {
        if (this.destroyed) return;
        // User typing or clearing the search box overrides any exact module filter.
        this.model.setModuleFilter('');
        this.setQuery(input.value);
      };
      const onKeyDown = (event: KeyboardEvent) => {
        if (this.destroyed) return;
        if (event.key === 'Enter') {
          this.model.setModuleFilter('');
          this.setQuery(input.value, { immediate: true });
        }
      };
      const onSearch = () => {
        if (this.destroyed) return;
        if (!input.value) {
          this.model.setModuleFilter('');
          this.setQuery('', { immediate: true });
        }
      };
      input.addEventListener('input', onInput);
      input.addEventListener('keydown', onKeyDown);
      input.addEventListener('search', onSearch);
      this.cleanupCallbacks.push(() => {
        input.removeEventListener('input', onInput);
        input.removeEventListener('keydown', onKeyDown);
        input.removeEventListener('search', onSearch);
      });
    }

    if (this.sessionFilter) {
      const ctrl = this.sessionFilter;
      const onChange = (event: { target?: { value?: string } } | Event) => {
        if (this.destroyed) return;
        const target = event && 'target' in event ? (event.target as { value?: string } | null) : null;
        this.setSession(normalizeSessionValue(target?.value ?? ctrl.value));
      };
      ctrl.addEventListener('change', onChange);
      if (ctrl.removeEventListener) {
        this.cleanupCallbacks.push(() => ctrl.removeEventListener?.('change', onChange));
      }
    }

    if (this.levelFilter) {
      const ctrl = this.levelFilter;
      const onChange = (event: { target?: { value?: string } } | Event) => {
        if (this.destroyed) return;
        const target = event && 'target' in event ? (event.target as { value?: string } | null) : null;
        this.setLevel(normalizeLevelValue(target?.value ?? ctrl.value));
      };
      ctrl.addEventListener('change', onChange);
      if (ctrl.removeEventListener) {
        this.cleanupCallbacks.push(() => ctrl.removeEventListener?.('change', onChange));
      }
    }

    if (this.scrollButton) {
      const button = this.scrollButton;
      const onClick = () => {
        if (this.destroyed) return;
        this.scrollToBottom({ behavior: 'smooth' });
      };
      button.addEventListener('click', onClick);
      this.cleanupCallbacks.push(() => button.removeEventListener('click', onClick));
    }

    if (typeof ResizeObserver !== 'undefined') {
      const target = this.scroller ?? this.logsContent;
      if (target) {
        this.containerResizeObserver = new ResizeObserver(() => {
          if (this.destroyed || !this.active) return;
          const { widthChanged, heightChanged } = this.updateViewportMetrics();
          if (widthChanged || heightChanged) {
            this.dirty = true;
            this.scheduleRender();
          }
        });
        this.containerResizeObserver.observe(target);
      }

      this.rowResizeObserver = new ResizeObserver(entries => {
        if (this.destroyed || !this.active) return;
        let anyChanged = false;
        const anchor = !this.pinnedToBottom && this.scroller
          ? this.model.getAnchorForScrollTop(this.scroller.scrollTop, this.lastContentWidth)
          : null;

        for (const entry of entries) {
          const el = entry.target as HTMLElement;
          const logId = Number(el.dataset.logId);
          const logIdx = Number(el.dataset.logIndex);
          if (!Number.isFinite(logId)) continue;
          const h = entry.borderBoxSize?.[0]?.blockSize ?? el.getBoundingClientRect().height;
          if (h > 0 && this.model.setMeasuredHeight(logId, h, Number.isFinite(logIdx) ? logIdx : undefined)) {
            anyChanged = true;
          }
        }
        if (anyChanged) {
          this.reconcileScrollAndSpacersAfterMeasure(anchor);
        }
      });
    }
  }

  private teardownDomBindings(): void {
    this.cancelRaf(this.renderFrame);
    this.renderFrame = null;
    this.cancelRaf(this.scrollSyncFrame);
    this.scrollSyncFrame = null;
    this.clearTimeoutSafe(this.searchTimer);
    this.searchTimer = null;
    this.clearTimeoutSafe(this.scrollSettleTimer);
    this.scrollSettleTimer = null;
    this.clearTimeoutSafe(this.fabHideTimer);
    this.fabHideTimer = null;

    this.containerResizeObserver?.disconnect();
    this.containerResizeObserver = null;
    this.rowResizeObserver?.disconnect();
    this.rowResizeObserver = null;

    for (const cleanup of this.cleanupCallbacks) {
      cleanup();
    }
    this.cleanupCallbacks = [];
    this.mountedRows.clear();
  }

  destroy(): void {
    this.destroyed = true;
    this.teardownDomBindings();
    this.recycledPool = [];
    this.topSpacer = null;
    this.bottomSpacer = null;
  }

  setActive(active: boolean): void {
    if (this.active === active) {
      if (active && this.dirty) {
        this.scheduleRender();
      }
      return;
    }
    this.active = active;
    if (!active) {
      this.cancelRaf(this.renderFrame);
      this.renderFrame = null;
      return;
    }
    this.readRowTypographyMetrics();
    this.updateViewportMetrics();
    if (this.options.loadHistory && this.model.getHistoryState() === 'idle' && this.model.getClearedEpoch() === 0) {
      void this.ensureLoaded().catch(error => {
        console.error('Failed to lazily load log history:', error);
      });
    }
    if (this.dirty || this.needsInitialBottomSnap || this.mountedRows.size === 0) {
      this.renderNow();
    } else {
      this.syncScrollButton();
    }
  }

  async ensureLoaded(): Promise<void> {
    const loader = this.options.loadHistory;
    if (!loader) return;
    const loaded = await this.model.ensureLoaded(loader);
    if (!loaded || this.destroyed) return;
    this.dirty = true;
    this.lastRenderedSignature = '';
    this.updateCountBadge();
    this.notifyStateChange();
    if (this.active) {
      this.renderNow();
    }
  }

  ensureHistoryLoaded(): Promise<void> {
    return this.ensureLoaded();
  }

  setEntries(entries: readonly AppLogEntry[]): void {
    const filtered = this.options.shouldAcceptEntry
      ? entries.filter(this.options.shouldAcceptEntry)
      : entries;
    this.model.setEntries(filtered);
    this.dirty = true;
    this.lastRenderedSignature = '';
    this.updateCountBadge();
    this.notifyStateChange();
    if (this.active) {
      this.scheduleRender();
    } else {
      this.syncScrollButton();
    }
  }

  append(entry: AppendLogInput): void {
    const result = this.model.append(entry);
    if (!result.accepted) return;

    this.updateCountBadge();
    this.notifyStateChange();
    if (!result.visibleChanged) return;

    this.dirty = true;
    if (this.active && (this.pinnedToBottom || this.mountedRows.size < 80)) {
      this.scheduleRender();
    } else if (!this.active) {
      this.lastRenderedSignature = '';
    }
  }

  clear(): void {
    this.clearTimeoutSafe(this.searchTimer);
    this.searchTimer = null;
    this.clearTimeoutSafe(this.scrollSettleTimer);
    this.scrollSettleTimer = null;
    this.cancelRaf(this.renderFrame);
    this.renderFrame = null;

    this.model.clear();
    this.pinnedToBottom = true;
    this.needsInitialBottomSnap = true;
    this.lastObservedScrollTop = 0;
    this.lastRenderedSignature = '';
    this.dirty = false;

    this.clearMountedDom();
    if (this.scroller) {
      this.programmaticScrollUntil = this.now() + 80;
      this.scroller.scrollTop = 0;
    }
    this.updateCountBadge();
    this.syncScrollButton(true);
    this.notifyStateChange();
  }

  setQuery(query: string, options: { immediate?: boolean } = {}): void {
    this.clearTimeoutSafe(this.searchTimer);
    this.searchTimer = null;

    const apply = () => {
      this.searchTimer = null;
      if (this.destroyed) return;
      const changed = this.model.setQuery(query);
      if (!changed) return;
      this.onFilterMutated();
    };

    if (options.immediate || this.searchDebounceMs <= 0) {
      apply();
      return;
    }

    this.searchTimer = this.setTimeoutSafe(apply, this.searchDebounceMs);
  }

  setSession(session: LogSessionFilter): void {
    if (!this.model.setSession(session)) return;
    this.onFilterMutated();
  }

  setLevel(level: LogLevelFilter): void {
    if (!this.model.setLevel(level)) return;
    this.onFilterMutated();
  }

  filterByModule(module: string): void {
    this.clearTimeoutSafe(this.searchTimer);
    this.searchTimer = null;
    const trimmed = module.trim();
    if (this.searchInput) {
      this.searchInput.value = trimmed;
    }
    const queryChanged = this.model.setQuery('');
    const moduleChanged = this.model.setModuleFilter(trimmed);
    this.options.onModuleFilter?.(trimmed);
    if (typeof CustomEvent !== 'undefined' && this.logsContent) {
      this.logsContent.dispatchEvent(new CustomEvent('virtual-logs:module-filter', {
        bubbles: true,
        detail: { module: trimmed },
      }));
    }
    if (queryChanged || moduleChanged || this.dirty) {
      this.onFilterMutated();
    } else {
      this.updateCountBadge();
      this.notifyStateChange();
    }
  }

  private onFilterMutated(): void {
    this.dirty = true;
    this.lastRenderedSignature = '';
    this.needsInitialBottomSnap = this.pinnedToBottom;
    this.updateCountBadge();
    this.notifyStateChange();
    if (this.active) {
      this.renderNow();
    } else {
      this.syncScrollButton();
    }
  }

  scrollToBottom(options: { behavior?: ScrollBehavior } = {}): void {
    this.pinnedToBottom = true;
    this.needsInitialBottomSnap = false;
    this.clearTimeoutSafe(this.scrollSettleTimer);
    this.scrollSettleTimer = null;

    if (!this.scroller) {
      this.syncScrollButton(true);
      return;
    }
    if (this.dirty) {
      this.renderNow();
    }

    const scroller = this.scroller;
    const behavior = options.behavior ?? 'smooth';
    const maxScroll = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    const distance = maxScroll - scroller.scrollTop;
    this.programmaticScrollUntil = this.now() + 900;

    if (behavior === 'smooth' && distance > Math.max(320, this.lastViewportHeight * 1.5)) {
      const preJumpTop = Math.max(0, maxScroll - Math.min(240, this.lastViewportHeight * 0.55));
      scroller.scrollTop = preJumpTop;
      this.lastObservedScrollTop = scroller.scrollTop;
      this.renderWindowOnly();
    }

    const finalTarget = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    if (typeof scroller.scrollTo === 'function') {
      scroller.scrollTo({ top: finalTarget, behavior });
    } else {
      scroller.scrollTop = finalTarget;
    }
    this.lastObservedScrollTop = scroller.scrollTop;

    if (behavior === 'auto' || behavior === 'instant') {
      this.syncScrollButton(true);
      return;
    }

    const settle = () => {
      this.scrollSettleTimer = null;
      if (this.destroyed || !this.pinnedToBottom || !this.scroller) return;
      this.renderWindowOnly();
      const target = Math.max(0, this.scroller.scrollHeight - this.scroller.clientHeight);
      if (Math.abs(this.scroller.scrollTop - target) > 1.5) {
        this.programmaticScrollUntil = this.now() + 180;
        this.scroller.scrollTop = target;
        this.lastObservedScrollTop = this.scroller.scrollTop;
      }
      this.syncScrollButton();
    };
    this.scrollSettleTimer = this.setTimeoutSafe(settle, 240);
  }

  getFilteredEntries(): readonly IndexedLogEntry[] {
    return this.model.getFilteredEntries();
  }

  getEntryByIndex(filteredIndex: number): IndexedLogEntry | null {
    return this.model.getFilteredEntry(filteredIndex);
  }

  getFilteredEntry(filteredIndex: number): IndexedLogEntry | null {
    return this.model.getFilteredEntry(filteredIndex);
  }

  formatFilteredLogsText(): string {
    return this.model.formatFilteredLogsText();
  }

  getStateSummary(): VirtualLogsStateSummary {
    return this.model.getSummary(this.pinnedToBottom, this.computeDistanceFromBottom());
  }

  getCountBadgeText(): string {
    const summary = this.getStateSummary();
    if (!summary.hasActiveFilter && !summary.hasActiveSearch && summary.filteredCount === summary.totalCount) {
      return `${summary.totalCount} 条`;
    }
    return `${summary.filteredCount} / ${summary.totalCount} 条`;
  }

  private computeDistanceFromBottom(): number {
    if (!this.scroller) return 0;
    const maxScroll = Math.max(0, this.scroller.scrollHeight - this.scroller.clientHeight);
    return Math.max(0, maxScroll - this.scroller.scrollTop);
  }

  private updateCountBadge(): void {
    if (!this.filterCountElement) return;
    const nextText = this.getCountBadgeText();
    if (this.filterCountElement.textContent !== nextText) {
      this.filterCountElement.textContent = nextText;
    }
  }

  private notifyStateChange(): void {
    this.options.onStateChange?.(this.getStateSummary());
  }

  syncScrollButton(immediate = false): void {
    const button = this.scrollButton;
    if (!button) return;

    const filteredCount = this.model.filteredCount;
    const distance = this.computeDistanceFromBottom();
    const shouldShow = filteredCount > 0
      && !this.pinnedToBottom
      && distance > this.showScrollButtonThresholdPx;

    if (this.fabVisibleState === shouldShow && (!immediate || shouldShow === !button.hidden)) {
      return;
    }
    this.fabVisibleState = shouldShow;
    this.clearTimeoutSafe(this.fabHideTimer);
    this.fabHideTimer = null;

    if (shouldShow) {
      button.hidden = false;
      button.setAttribute('aria-hidden', 'false');
      if (immediate) {
        button.classList.add('is-visible');
      } else {
        this.scheduleRaf(() => {
          if (!this.destroyed && this.fabVisibleState === true) {
            button.classList.add('is-visible');
          }
        });
      }
      return;
    }

    button.classList.remove('is-visible');
    button.setAttribute('aria-hidden', 'true');
    const reducedMotion = typeof window !== 'undefined'
      && typeof window.matchMedia === 'function'
      && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    if (immediate || reducedMotion) {
      button.hidden = true;
    } else {
      this.fabHideTimer = this.setTimeoutSafe(() => {
        this.fabHideTimer = null;
        if (!this.destroyed && this.fabVisibleState === false && this.scrollButton) {
          this.scrollButton.hidden = true;
        }
      }, 190);
    }
  }

  private scheduleRender(): void {
    if (this.renderFrame !== null) return;
    this.renderFrame = this.scheduleRaf(() => {
      this.renderFrame = null;
      if (this.destroyed || !this.active) return;
      this.renderNow();
    });
  }

  private renderNow(): void {
    this.cancelRaf(this.renderFrame);
    this.renderFrame = null;
    this.dirty = false;
    if (!this.logsContent) return;
    this.updateViewportMetrics();
    this.renderWindowOnly(true);
    this.updateCountBadge();
    this.syncScrollButton();
  }

  private clearMountedDom(): void {
    for (const el of this.mountedRows.values()) {
      this.rowResizeObserver?.unobserve(el);
      if (this.recycledPool.length < 80) {
        this.recycledPool.push(el);
      }
    }
    this.mountedRows.clear();
    this.lastRangeStart = 0;
    this.lastRangeEnd = -1;
    if (this.logsContent && this.logsContent.childElementCount > 0) {
      this.logsContent.replaceChildren();
    }
  }

  private ensureSpacers(): { topSpacer: HTMLDivElement; bottomSpacer: HTMLDivElement } {
    const doc = this.logsContent!.ownerDocument;
    if (!this.topSpacer) {
      this.topSpacer = doc.createElement('div');
      this.topSpacer.className = 'virtual-logs-spacer virtual-logs-spacer-top';
      this.topSpacer.setAttribute('aria-hidden', 'true');
    }
    if (!this.bottomSpacer) {
      this.bottomSpacer = doc.createElement('div');
      this.bottomSpacer.className = 'virtual-logs-spacer virtual-logs-spacer-bottom';
      this.bottomSpacer.setAttribute('aria-hidden', 'true');
    }
    return { topSpacer: this.topSpacer, bottomSpacer: this.bottomSpacer };
  }

  private acquireRowElement(item: IndexedLogEntry, filteredIndex: number): HTMLDivElement {
    const existing = this.mountedRows.get(item.id);
    if (existing) {
      const idxStr = String(filteredIndex);
      if (existing.dataset.logIndex !== idxStr) {
        existing.dataset.logIndex = idxStr;
      }
      return existing;
    }

    const doc = this.logsContent!.ownerDocument;
    let el = this.recycledPool.pop();
    let timeEl: HTMLSpanElement;
    let msgEl: HTMLSpanElement;

    if (el && el.firstElementChild && el.lastElementChild && el.firstElementChild !== el.lastElementChild) {
      timeEl = el.firstElementChild as HTMLSpanElement;
      msgEl = el.lastElementChild as HTMLSpanElement;
    } else {
      el = doc.createElement('div');
      timeEl = doc.createElement('span');
      timeEl.className = 'log-time';
      msgEl = doc.createElement('span');
      el.append(timeEl, msgEl);
    }

    el.className = item.isSessionStart ? 'log-entry log-session-divider' : 'log-entry';
    el.dataset.logId = String(item.id);
    el.dataset.logIndex = String(filteredIndex);
    el.dataset.logModule = item.module;
    timeEl.textContent = `[${item.time}]`;
    msgEl.className = `log-${item.type}`;
    msgEl.textContent = `[${item.module}] ${item.displayMessage}`;

    this.mountedRows.set(item.id, el);
    this.rowResizeObserver?.observe(el);
    return el;
  }

  private renderWindowOnly(forceReconcile = false): void {
    const logsContent = this.logsContent;
    if (!logsContent) return;

    const filtered = this.model.getFilteredEntries();
    const total = filtered.length;
    if (total === 0) {
      this.clearMountedDom();
      this.lastRenderedSignature = 'empty';
      this.needsInitialBottomSnap = false;
      this.syncScrollButton(true);
      return;
    }

    const scroller = this.scroller;
    const viewportHeight = Math.max(120, scroller?.clientHeight || this.lastViewportHeight);
    const width = this.lastContentWidth;
    const prefix = this.model.getPrefixSums(width);
    const totalEstHeight = prefix[total];

    const targetScrollTop = (this.pinnedToBottom || this.needsInitialBottomSnap)
      ? Math.max(0, totalEstHeight - viewportHeight)
      : (scroller?.scrollTop ?? 0);

    const range = computeVirtualRange(prefix, total, targetScrollTop, viewportHeight, this.overscan);
    const signature = `${total}:${range.startIndex}:${range.endIndex}:${Math.round(range.topOffset)}:${Math.round(range.bottomOffset)}`;
    if (
      !forceReconcile
      && signature === this.lastRenderedSignature
      && range.startIndex === this.lastRangeStart
      && range.endIndex === this.lastRangeEnd
    ) {
      return;
    }

    const anchor = !this.pinnedToBottom && !this.needsInitialBottomSnap && scroller
      ? this.model.getAnchorForScrollTop(scroller.scrollTop, width)
      : null;

    const { topSpacer, bottomSpacer } = this.ensureSpacers();
    topSpacer.style.height = `${range.topOffset}px`;
    bottomSpacer.style.height = `${range.bottomOffset}px`;

    const keepIds = new Set<number>();
    for (let i = range.startIndex; i <= range.endIndex; i += 1) {
      keepIds.add(filtered[i].id);
    }

    for (const [id, el] of this.mountedRows.entries()) {
      if (!keepIds.has(id)) {
        this.rowResizeObserver?.unobserve(el);
        el.remove();
        this.mountedRows.delete(id);
        if (this.recycledPool.length < 80) {
          this.recycledPool.push(el);
        }
      }
    }

    if (topSpacer.parentElement !== logsContent || logsContent.firstElementChild !== topSpacer) {
      logsContent.prepend(topSpacer);
    }
    if (bottomSpacer.parentElement !== logsContent || logsContent.lastElementChild !== bottomSpacer) {
      logsContent.append(bottomSpacer);
    }

    let cursor: Element | null = topSpacer.nextElementSibling;
    for (let i = range.startIndex; i <= range.endIndex; i += 1) {
      const rowEl = this.acquireRowElement(filtered[i], i);
      if (cursor === rowEl) {
        cursor = cursor.nextElementSibling;
      } else {
        logsContent.insertBefore(rowEl, cursor ?? bottomSpacer);
      }
    }

    while (cursor && cursor !== bottomSpacer) {
      const next = cursor.nextElementSibling;
      cursor.remove();
      cursor = next;
    }

    this.lastRangeStart = range.startIndex;
    this.lastRangeEnd = range.endIndex;
    this.lastRenderedSignature = signature;

    // Measure newly mounted rows and adjust spacers/scrollTop synchronously to avoid visual jumps.
    let measuredChanged = false;
    for (let i = range.startIndex; i <= range.endIndex; i += 1) {
      const item = filtered[i];
      const rowEl = this.mountedRows.get(item.id);
      if (!rowEl) continue;
      const height = rowEl.getBoundingClientRect().height || rowEl.offsetHeight;
      if (height > 0 && this.model.setMeasuredHeight(item.id, height, i)) {
        measuredChanged = true;
      }
    }

    if (measuredChanged) {
      this.reconcileScrollAndSpacersAfterMeasure(anchor);
    } else if ((this.pinnedToBottom || this.needsInitialBottomSnap) && scroller) {
      const target = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
      if (Math.abs(scroller.scrollTop - target) > 1) {
        this.programmaticScrollUntil = this.now() + 140;
        scroller.scrollTop = target;
        this.lastObservedScrollTop = scroller.scrollTop;
      }
    }
    this.needsInitialBottomSnap = false;
    this.syncScrollButton();
  }

  private reconcileScrollAndSpacersAfterMeasure(
    anchor: { index: number; offset: number } | null,
  ): void {
    const scroller = this.scroller;
    const filtered = this.model.getFilteredEntries();
    const total = filtered.length;
    if (total === 0 || !this.topSpacer || !this.bottomSpacer) return;

    const width = this.lastContentWidth;
    const viewportHeight = Math.max(120, scroller?.clientHeight || this.lastViewportHeight);
    const prefix = this.model.getPrefixSums(width);
    const totalHeight = prefix[total];

    const start = Math.max(0, Math.min(this.lastRangeStart, total - 1));
    const end = Math.max(start, Math.min(this.lastRangeEnd, total - 1));
    const topOffset = Math.max(0, prefix[start] ?? 0);
    const bottomOffset = Math.max(0, totalHeight - (prefix[end + 1] ?? totalHeight));

    this.topSpacer.style.height = `${topOffset}px`;
    this.bottomSpacer.style.height = `${bottomOffset}px`;
    this.lastRenderedSignature = `${total}:${start}:${end}:${Math.round(topOffset)}:${Math.round(bottomOffset)}`;

    if (!scroller) return;
    if (this.pinnedToBottom || this.needsInitialBottomSnap) {
      const bottomTarget = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
      if (Math.abs(scroller.scrollTop - bottomTarget) > 1) {
        this.programmaticScrollUntil = this.now() + 140;
        scroller.scrollTop = bottomTarget;
        this.lastObservedScrollTop = scroller.scrollTop;
      }
    } else if (anchor) {
      const targetTop = Math.min(
        Math.max(0, scroller.scrollHeight - scroller.clientHeight),
        this.model.getScrollTopForAnchor(anchor, width),
      );
      if (Math.abs(scroller.scrollTop - targetTop) >= 0.5) {
        this.programmaticScrollUntil = this.now() + 140;
        scroller.scrollTop = targetTop;
        this.lastObservedScrollTop = scroller.scrollTop;
      }
    }
    // Check if the updated spacers still keep the visible viewport covered.
    const nextRange = computeVirtualRange(prefix, total, scroller.scrollTop, viewportHeight, this.overscan);
    if (nextRange.startIndex < this.lastRangeStart || nextRange.endIndex > this.lastRangeEnd) {
      this.scheduleRender();
    }
    this.syncScrollButton();
  }
}
