export type LiquidGlassVariantId = 'v1' | 'v2' | 'v3' | 'v4';

interface HistoricalProfile {
  bevel: number;
  entryStop: number;
  rimPower: number;
  scaleR: number;
  scaleG: number;
  scaleB: number;
}

interface VariantSpec {
  id: LiquidGlassVariantId;
  badge: string;
  label: string;
  summary: string;
  rasterScale: 1 | 2;
  useMaskAndMerge: boolean;
  fixedPeak: boolean;
  bar: HistoricalProfile;
  droplet: HistoricalProfile;
}

const SVG_NS = 'http://www.w3.org/2000/svg';
const CHANNELS = ['red', 'green', 'blue'] as const;

export const LIQUID_GLASS_VARIANTS: Record<LiquidGlassVariantId, VariantSpec> = {
  v1: {
    id: 'v1',
    badge: 'V1',
    label: '初版连续 Snell (1× + Mask)',
    summary: '1× 分辨率 · Mask 混合 · 水滴 30 / 23 / 16',
    rasterScale: 1,
    useMaskAndMerge: true,
    fixedPeak: true,
    bar: { bevel: 9, entryStop: 0.14, rimPower: 1.75, scaleR: 20, scaleG: 13, scaleB: 5 },
    droplet: { bevel: 15, entryStop: 0.14, rimPower: 1.55, scaleR: 30, scaleG: 23, scaleB: 16 },
  },
  v2: {
    id: 'v2',
    badge: 'V2',
    label: '2× 超采样 + 缓入波峰',
    summary: '2× 分辨率 · Mask 混合 · 水滴 21.5 / 19 / 16.5',
    rasterScale: 2,
    useMaskAndMerge: true,
    fixedPeak: false,
    bar: { bevel: 9, entryStop: 0.16, rimPower: 1.75, scaleR: 19, scaleG: 13, scaleB: 6 },
    droplet: { bevel: 15, entryStop: 0.28, rimPower: 1.4, scaleR: 21.5, scaleG: 19, scaleB: 16.5 },
  },
  v3: {
    id: 'v3',
    badge: 'V3（当前）',
    label: '2× 去重影直出通道',
    summary: '2× 分辨率 · 无 Mask 直出 · 水滴 17.5 / 15.5 / 13.5',
    rasterScale: 2,
    useMaskAndMerge: false,
    fixedPeak: false,
    bar: { bevel: 9, entryStop: 0.14, rimPower: 1.75, scaleR: 20, scaleG: 13, scaleB: 5 },
    droplet: { bevel: 14, entryStop: 0.34, rimPower: 1.5, scaleR: 17.5, scaleG: 15.5, scaleB: 13.5 },
  },
  v4: {
    id: 'v4',
    badge: 'V4',
    label: '较弱折射对照',
    summary: '2× 分辨率 · 无 Mask 直出 · 水滴 12.4 / 10.6 / 8.8',
    rasterScale: 2,
    useMaskAndMerge: false,
    fixedPeak: false,
    bar: { bevel: 9, entryStop: 0.14, rimPower: 1.75, scaleR: 20, scaleG: 13, scaleB: 5 },
    droplet: { bevel: 13, entryStop: 0.22, rimPower: 1.65, scaleR: 12.4, scaleG: 10.6, scaleB: 8.8 },
  },
};

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attributes: Record<string, string | number>) {
  const element = document.createElementNS(SVG_NS, tag);
  for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, String(value));
  return element;
}

function smoothstep(start: number, end: number, value: number) {
  const t = Math.max(0, Math.min(1, (value - start) / (end - start)));
  return t * t * (3 - 2 * t);
}

function buildLensMaps(
  width: number,
  height: number,
  profile: HistoricalProfile,
  rasterScale: 1 | 2,
  useMask: boolean,
  fixedPeak: boolean,
) {
  const cssW = Math.max(2, Math.round(width));
  const cssH = Math.max(2, Math.round(height));
  const rw = cssW * rasterScale;
  const rh = cssH * rasterScale;

  const normalCanvas = document.createElement('canvas');
  normalCanvas.width = rw;
  normalCanvas.height = rh;
  const normalCtx = normalCanvas.getContext('2d');
  if (!normalCtx) return null;

  let maskCanvas: HTMLCanvasElement | null = null;
  let maskCtx: CanvasRenderingContext2D | null = null;
  let maskImg: ImageData | null = null;
  if (useMask) {
    maskCanvas = document.createElement('canvas');
    maskCanvas.width = rw;
    maskCanvas.height = rh;
    maskCtx = maskCanvas.getContext('2d');
    if (!maskCtx) return null;
    maskImg = maskCtx.createImageData(rw, rh);
  }

  const normalImg = normalCtx.createImageData(rw, rh);
  const nd = normalImg.data;
  const md = maskImg?.data ?? null;

  for (let i = 0; i < nd.length; i += 4) {
    nd[i] = 128;
    nd[i + 1] = 128;
    nd[i + 2] = 128;
    nd[i + 3] = 255;
  }

  const radius = Math.min(cssW, cssH) / 2;
  const straightX = cssW / 2 - radius;
  const straightY = cssH / 2 - radius;
  const bevel = Math.min(profile.bevel, radius * 0.36);

  let peak: number;
  if (fixedPeak) {
    peak = Math.pow(1 - 0.14, profile.rimPower) || 1;
  } else {
    peak = 0;
    for (let s = 1; s < 100; s++) {
      const st = s / 100;
      peak = Math.max(peak, smoothstep(0, profile.entryStop, st) * Math.pow(1 - st, profile.rimPower));
    }
    peak ||= 1;
  }

  for (let y = 0; y < rh; y++) {
    const py = (y + 0.5) / rasterScale - cssH / 2;
    for (let x = 0; x < rw; x++) {
      const px = (x + 0.5) / rasterScale - cssW / 2;
      const nx = Math.sign(px) * Math.max(0, Math.abs(px) - straightX);
      const ny = Math.sign(py) * Math.max(0, Math.abs(py) - straightY);
      const dist = Math.hypot(nx, ny);
      const depth = radius - dist;
      if (depth <= 0 || depth >= bevel || dist <= 1e-4) continue;

      const t = depth / bevel;
      const ix = -nx / dist;
      const iy = -ny / dist;
      const strength = Math.min(
        1,
        (smoothstep(0, profile.entryStop, t) * Math.pow(1 - t, profile.rimPower)) / peak,
      );

      const idx = (y * rw + x) * 4;
      nd[idx] = Math.max(0, Math.min(255, Math.round(128 + ix * strength * 127)));
      nd[idx + 1] = Math.max(0, Math.min(255, Math.round(128 + iy * strength * 127)));

      if (md) {
        const maskAlpha = smoothstep(0, 0.08, t) * (1 - smoothstep(0.72, 1, t));
        md[idx] = 255;
        md[idx + 1] = 255;
        md[idx + 2] = 255;
        md[idx + 3] = Math.max(0, Math.min(255, Math.round(255 * maskAlpha)));
      }
    }
  }

  normalCtx.putImageData(normalImg, 0, 0);
  if (maskCanvas && maskCtx && maskImg) maskCtx.putImageData(maskImg, 0, 0);

  return {
    normalHref: normalCanvas.toDataURL(),
    maskHref: maskCanvas ? maskCanvas.toDataURL() : null,
  };
}

function createVariantUpdater(spec: VariantSpec) {
  let filterDefs: SVGDefsElement | undefined;

  const updateLens = (id: string, width: number, height: number, profile: HistoricalProfile) => {
    const w = Math.round(width);
    const h = Math.round(height);
    if (w < 2 || h < 2) return false;
    const geometry = `${w}x${h}`;
    const previous = document.getElementById(id);
    if (previous?.getAttribute('data-geometry') === geometry) return true;

    const maps = buildLensMaps(w, h, profile, spec.rasterScale, spec.useMaskAndMerge, spec.fixedPeak);
    if (!maps) return false;
    if (!filterDefs) {
      const holder = svg('svg', {
        width: 0,
        height: 0,
        'aria-hidden': 'true',
        style: 'position:absolute;pointer-events:none',
      });
      filterDefs = svg('defs', {});
      holder.append(filterDefs);
      document.body.append(holder);
    }

    const filter = svg('filter', {
      id,
      'data-geometry': geometry,
      x: '0%',
      y: '0%',
      width: '100%',
      height: '100%',
      'color-interpolation-filters': 'sRGB',
    });

    filter.append(
      svg('feImage', {
        href: maps.normalHref,
        x: 0,
        y: 0,
        width: w,
        height: h,
        preserveAspectRatio: 'none',
        result: 'map',
      }),
    );

    if (maps.maskHref) {
      filter.append(
        svg('feImage', {
          href: maps.maskHref,
          x: 0,
          y: 0,
          width: w,
          height: h,
          preserveAspectRatio: 'none',
          result: 'mask',
        }),
      );
    }

    const scales = [profile.scaleR, profile.scaleG, profile.scaleB];
    CHANNELS.forEach((channel, index) => {
      filter.append(
        svg('feDisplacementMap', {
          in: 'SourceGraphic',
          in2: 'map',
          scale: scales[index],
          xChannelSelector: 'R',
          yChannelSelector: 'G',
          result: `${channel}Displaced`,
        }),
      );

      const matrix = new Array<number>(20).fill(0);
      matrix[index * 6] = 1;
      matrix[18] = 1;
      filter.append(
        svg('feColorMatrix', {
          in: `${channel}Displaced`,
          type: 'matrix',
          values: matrix.join(' '),
          result: `${channel}Channel`,
        }),
      );
    });

    if (maps.maskHref) {
      filter.append(
        svg('feComposite', {
          in: 'redChannel',
          in2: 'greenChannel',
          operator: 'arithmetic',
          k2: 1,
          k3: 1,
          result: 'redGreen',
        }),
        svg('feComposite', {
          in: 'redGreen',
          in2: 'blueChannel',
          operator: 'arithmetic',
          k2: 1,
          k3: 1,
          result: 'refracted',
        }),
        svg('feComposite', {
          in: 'refracted',
          in2: 'mask',
          operator: 'in',
          result: 'rim',
        }),
      );
      const merge = svg('feMerge', {});
      merge.append(svg('feMergeNode', { in: 'SourceGraphic' }), svg('feMergeNode', { in: 'rim' }));
      filter.append(merge);
    } else {
      filter.append(
        svg('feComposite', {
          in: 'redChannel',
          in2: 'greenChannel',
          operator: 'arithmetic',
          k2: 1,
          k3: 1,
          result: 'redGreen',
        }),
        svg('feComposite', {
          in: 'redGreen',
          in2: 'blueChannel',
          operator: 'arithmetic',
          k2: 1,
          k3: 1,
        }),
      );
    }

    if (previous) previous.replaceWith(filter);
    else filterDefs.append(filter);
    return true;
  };

  return (host: HTMLElement, dock: DOMRect, lensWidth: number, lensHeight: number) => {
    const radius = Math.min(lensWidth, lensHeight) / 2;
    const bevel = Math.min(spec.droplet.bevel, radius * 0.36);
    host.style.setProperty('--dock-lens-bevel', `${bevel}px`);
    if (!/Chrome|Chromium|Edg\//.test(navigator.userAgent)
      || !CSS.supports('backdrop-filter', 'url("#dock-lens")')) return;

    const name = host.id === 'nav' ? 'main' : 'billing';
    const surface = `dock-${name}-glass`;
    const lens = `dock-${name}-lens`;
    if (updateLens(surface, dock.width, dock.height, spec.bar)) {
      host.style.setProperty('--dock-backdrop', `url("#${surface}") blur(1.5px) saturate(1.28)`);
    }
    if (updateLens(lens, lensWidth, lensHeight, spec.droplet)) {
      host.style.setProperty('--dock-lens-backdrop', `url("#${lens}")`);
    }
  };
}

function setColorScheme(scheme: 'light' | 'dark') {
  document.documentElement.dataset.colorScheme = scheme;
  document.documentElement.style.colorScheme = scheme;
}

function hideRegressionPanel() {
  const aside = document.getElementById('qa-run')?.closest('aside');
  if (aside) aside.hidden = true;
}

function mountVariantBanner(spec: VariantSpec) {
  const bar = document.createElement('div');
  bar.id = 'liquid-variant-banner';
  bar.style.cssText = [
    'position:fixed',
    'top:max(6px,env(safe-area-inset-top))',
    'left:0',
    'right:0',
    'z-index:25000',
    'display:flex',
    'justify-content:center',
    'padding:0 8px',
    'pointer-events:none',
  ].join(';');

  const pill = document.createElement('div');
  pill.style.cssText = [
    'pointer-events:auto',
    'display:flex',
    'flex-wrap:nowrap',
    'align-items:center',
    'gap:4px',
    'max-width:100%',
    'overflow-x:auto',
    'scrollbar-width:none',
    'padding:3px 7px',
    'border-radius:999px',
    'border:1px solid rgba(15,23,42,0.14)',
    'background:rgba(255,255,255,0.88)',
    'backdrop-filter:blur(12px)',
    '-webkit-backdrop-filter:blur(12px)',
    'box-shadow:0 4px 14px rgba(15,23,42,0.10)',
    'color:#0f172a',
    'font:11px/1.2 system-ui,-apple-system,sans-serif',
    'white-space:nowrap',
  ].join(';');

  const badge = document.createElement('strong');
  badge.textContent = spec.badge;
  badge.title = `${spec.label} · ${spec.summary}`;
  badge.style.cssText = 'color:#2563eb;font-weight:700;margin-right:1px;';
  pill.append(badge);

  (['v1', 'v2', 'v3', 'v4'] as const).forEach(id => {
    const link = document.createElement('a');
    link.href = `/scripts/liquid-glass-${id}.html`;
    link.textContent = id.toUpperCase();
    const active = id === spec.id;
    link.style.cssText = [
      'padding:2px 6px',
      'border-radius:999px',
      'text-decoration:none',
      'font-weight:600',
      active
        ? 'background:#2563eb;color:#fff;'
        : 'background:rgba(15,23,42,0.07);color:#334155;',
    ].join(';');
    pill.append(link);
  });

  const compareLink = document.createElement('a');
  compareLink.href = '/scripts/liquid-glass-compare.html';
  compareLink.textContent = '同屏';
  compareLink.style.cssText = 'padding:2px 7px;border-radius:999px;text-decoration:none;background:rgba(37,99,235,0.1);color:#1d4ed8;font-weight:600;';
  pill.append(compareLink);

  const btnStyle = 'border:0;border-radius:999px;padding:2px 7px;cursor:pointer;font:inherit;font-weight:600;background:rgba(15,23,42,0.08);color:#1e293b;';

  const themeBtn = document.createElement('button');
  themeBtn.type = 'button';
  themeBtn.textContent = '明暗';
  themeBtn.style.cssText = btnStyle;
  themeBtn.addEventListener('click', () => {
    const next = document.documentElement.dataset.colorScheme === 'dark' ? 'light' : 'dark';
    setColorScheme(next);
  });
  pill.append(themeBtn);

  const scrollBtn = document.createElement('button');
  scrollBtn.type = 'button';
  scrollBtn.textContent = '滚背景';
  scrollBtn.style.cssText = btnStyle;
  scrollBtn.addEventListener('click', () => {
    const main = document.getElementById('main-content');
    if (!main) return;
    main.scrollTop = main.scrollTop > 40 ? 0 : 140;
  });
  pill.append(scrollBtn);

  const closeBtn = document.createElement('button');
  closeBtn.type = 'button';
  closeBtn.setAttribute('aria-label', '收起版本栏');
  closeBtn.textContent = '×';
  closeBtn.style.cssText = 'border:0;background:transparent;color:#64748b;cursor:pointer;font-size:14px;line-height:1;padding:0 2px;';
  closeBtn.addEventListener('click', () => bar.remove());
  pill.append(closeBtn);

  bar.append(pill);
  document.body.append(bar);
}

export async function bootLiquidGlassVariant(variant: LiquidGlassVariantId) {
  const url = new URL(location.href);
  if (!url.searchParams.has('platform')) {
    url.searchParams.set('platform', 'android');
    history.replaceState(null, '', url.toString());
  }

  const spec = LIQUID_GLASS_VARIANTS[variant];
  document.documentElement.dataset.liquidVariant = variant;

  if (variant !== 'v3') {
    (window as Window & {
      __LIQUID_GLASS_UPDATER__?: (
        host: HTMLElement,
        dock: DOMRect,
        lensWidth: number,
        lensHeight: number,
      ) => void;
    }).__LIQUID_GLASS_UPDATER__ = createVariantUpdater(spec);
  }

  window.addEventListener('message', event => {
    const data = event.data as { type?: string; colorScheme?: 'light' | 'dark'; scrollTop?: number } | null;
    if (!data || data.type !== 'liquid-compare') return;
    if (data.colorScheme === 'light' || data.colorScheme === 'dark') {
      setColorScheme(data.colorScheme);
    }
    if (typeof data.scrollTop === 'number') {
      const main = document.getElementById('main-content');
      if (main) main.scrollTop = data.scrollTop;
    }
  });

  await import('./app-regression.ts');
  hideRegressionPanel();

  if (!url.searchParams.has('embed')) {
    mountVariantBanner(spec);
  }
}
