import type {
  BillingActionRequest,
  BillingActionResult,
  BillingCenterData,
  BillingRecordQuery,
  BillingRecordResult,
} from './models';

export type BillingSource = 'local' | 'router';

export interface RouterBillingBridge {
  invalidate(): void;
  accounts(): { value: string; text: string }[];
  currentAccount(): string;
  selectAccount(value: string): Promise<void>;
  refresh(): Promise<void>;
  refreshSummary(): Promise<void>;
  queryRecords(query: BillingRecordQuery): Promise<BillingRecordResult>;
  performAction(request: BillingActionRequest): Promise<BillingActionResult>;
  disconnectSession(sessionId: string, ip: string, mac: string): Promise<string>;
  setMauth(enabled: boolean): Promise<string>;
}

let currentSource: BillingSource = 'local';
let currentBridge: RouterBillingBridge | null = null;
let dataListener: ((data: BillingCenterData) => void) | null = null;
const sourceListeners = new Set<(source: BillingSource) => void>();

export function getBillingSource(): BillingSource {
  return currentSource;
}

export function setBillingSource(source: BillingSource): void {
  if (currentSource === source) return;
  if (currentSource === 'router') currentBridge?.invalidate();
  currentSource = source;
  sourceListeners.forEach(listener => {
    try { listener(source); } catch (error) { console.error('Billing source listener failed:', error); }
  });
}

export function onBillingSourceChange(listener: (source: BillingSource) => void): () => void {
  sourceListeners.add(listener);
  return () => sourceListeners.delete(listener);
}

export function setRouterBillingBridge(bridge: RouterBillingBridge | null): void {
  currentBridge = bridge;
}

export function getRouterBillingBridge(): RouterBillingBridge | null {
  return currentBridge;
}

export function setRouterBillingDataListener(listener: ((data: BillingCenterData) => void) | null): void {
  dataListener = listener;
}

export function publishRouterBillingData(data: BillingCenterData): void {
  if (!dataListener) return;
  try { dataListener(data); } catch (error) { console.error('Router billing data listener failed:', error); }
}

let progressListener: ((percent: number, message: string) => void) | null = null;
export function setRouterBillingProgressListener(listener: (percent: number, message: string) => void): void { progressListener = listener; }
export function publishRouterBillingProgress(percent: number, message: string): void {
  if (currentSource === 'router') progressListener?.(Math.min(99, Math.max(2, percent)), message);
}
