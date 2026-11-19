import { invoke } from '@tauri-apps/api/core';

export interface MacosLiquidGlassCapabilities {
  supported: boolean;
  engine: 'ns-glass-effect' | 'ns-visual-effect' | 'none';
  interactiveSupported: boolean;
}

export interface MacosLiquidGlassSlot {
  id: string;
  placement: 'underlay' | 'overlay';
  x: number;
  y: number;
  width: number;
  height: number;
  cornerRadius: number;
  style: 'regular' | 'clear';
  visible: boolean;
  interactive?: boolean;
  tintRgba?: [number, number, number, number];
}

const DISABLED_CAPS: MacosLiquidGlassCapabilities = {
  supported: false,
  engine: 'none',
  interactiveSupported: false,
};

const reducedTransparency = window.matchMedia('(prefers-reduced-transparency: reduce)');
const slots = new Map<string, MacosLiquidGlassSlot>();

let capabilities: MacosLiquidGlassCapabilities = DISABLED_CAPS;
let initialized = false;
let initPromise: Promise<MacosLiquidGlassCapabilities> | null = null;
let pendingFrame = 0;
let lastPayloadSignature = '';

function isMacosRuntime(): boolean {
  return Boolean((window as Window & { __TAURI__?: unknown }).__TAURI__)
    && (document.body.classList.contains('is-macos') || navigator.userAgent.includes('Mac OS X'));
}

function applyDatasetState() {
  if (capabilities.supported && !reducedTransparency.matches) {
    document.documentElement.dataset.macosNativeGlass = capabilities.engine;
  } else {
    delete document.documentElement.dataset.macosNativeGlass;
  }
}

export function isMacosNativeGlassActive(): boolean {
  return Boolean(document.documentElement.dataset.macosNativeGlass);
}

export function getMacosNativeGlassEngine(): MacosLiquidGlassCapabilities['engine'] {
  return isMacosNativeGlassActive() ? capabilities.engine : 'none';
}

export function hasBlockingSurfaceOpen(): boolean {
  if (document.querySelector('.modal.show, .image-captcha-overlay:not(.hidden), .custom-select.open')) {
    return true;
  }
  const onboarding = document.getElementById('onboarding-overlay');
  if (onboarding && !onboarding.hidden && getComputedStyle(onboarding).display !== 'none') {
    return true;
  }
  return false;
}

function serializeSlots(payload: MacosLiquidGlassSlot[]): string {
  return payload
    .map(slot => [
      slot.id,
      slot.placement,
      slot.visible ? 1 : 0,
      slot.style,
      slot.interactive ? 1 : 0,
      Math.round(slot.x * 4),
      Math.round(slot.y * 4),
      Math.round(slot.width * 4),
      Math.round(slot.height * 4),
      Math.round(slot.cornerRadius * 4),
      slot.tintRgba ? slot.tintRgba.map(v => Math.round(v * 255)).join(',') : '',
    ].join(':'))
    .join('|');
}

function dispatchSync() {
  pendingFrame = 0;
  if (!isMacosRuntime()) return;
  if (!capabilities.supported || reducedTransparency.matches) {
    if (lastPayloadSignature !== '') {
      lastPayloadSignature = '';
      void invoke('sync_macos_liquid_glass', { slots: [] }).catch(() => {});
    }
    return;
  }

  const blocked = hasBlockingSurfaceOpen();
  const payload = Array.from(slots.values()).map(slot => (
    blocked && slot.placement === 'overlay'
      ? { ...slot, visible: false }
      : slot
  ));
  const signature = serializeSlots(payload);
  if (signature === lastPayloadSignature) return;
  lastPayloadSignature = signature;
  void invoke('sync_macos_liquid_glass', { slots: payload }).catch(() => {});
}

export function flushMacosLiquidGlass(immediate = false) {
  if (!isMacosRuntime() || !capabilities.supported) return;
  if (immediate) {
    if (pendingFrame) {
      cancelAnimationFrame(pendingFrame);
      pendingFrame = 0;
    }
    dispatchSync();
    return;
  }
  if (!pendingFrame) {
    pendingFrame = requestAnimationFrame(dispatchSync);
  }
}

export function setMacosGlassSlot(id: string, slot: MacosLiquidGlassSlot | null, immediate = false) {
  if (!slot) {
    if (!slots.has(id)) return;
    slots.delete(id);
  } else {
    slots.set(id, { ...slot, id });
  }
  flushMacosLiquidGlass(immediate);
}

export async function initMacosLiquidGlass(): Promise<MacosLiquidGlassCapabilities> {
  if (initPromise) return initPromise;
  initPromise = (async () => {
    if (!isMacosRuntime()) {
      capabilities = DISABLED_CAPS;
      applyDatasetState();
      return capabilities;
    }
    try {
      const caps = await invoke<MacosLiquidGlassCapabilities>('get_macos_liquid_glass_capabilities');
      capabilities = caps?.supported ? caps : DISABLED_CAPS;
    } catch {
      capabilities = DISABLED_CAPS;
    }
    applyDatasetState();
    if (!initialized) {
      initialized = true;
      reducedTransparency.addEventListener('change', () => {
        applyDatasetState();
        flushMacosLiquidGlass(true);
      });
    }
    return capabilities;
  })();
  return initPromise;
}
