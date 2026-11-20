const SVG_NS = 'http://www.w3.org/2000/svg';
const CHANNELS = ['red', 'green', 'blue'] as const;

interface LensProfile {
  bevel: number;
  entryStop: number;
  rimPower: number;
  scaleR: number;
  scaleG: number;
  scaleB: number;
}

const barProfile: LensProfile = {
  bevel: 9,
  entryStop: 0.14,
  rimPower: 1.75,
  scaleR: 20,
  scaleG: 13,
  scaleB: 5,
};

const dropletProfile: LensProfile = {
  bevel: 14,
  entryStop: 0.34,
  rimPower: 1.5,
  scaleR: 17.5,
  scaleG: 15.5,
  scaleB: 13.5,
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

function createNormalMap(width: number, height: number, profile: LensProfile) {
  const cssW = Math.max(2, Math.round(width));
  const cssH = Math.max(2, Math.round(height));
  // Supersample 2x so curved gradients remain free of staircase quantization on HiDPI screens.
  const rw = cssW * 2;
  const rh = cssH * 2;

  const canvas = document.createElement('canvas');
  canvas.width = rw;
  canvas.height = rh;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;

  const img = ctx.createImageData(rw, rh);
  const nd = img.data;

  // Fill neutral (128, 128, 128, 255) so clear interior pixels pass through without distortion.
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

  let peak = 0;
  for (let s = 1; s < 100; s++) {
    const st = s / 100;
    peak = Math.max(peak, smoothstep(0, profile.entryStop, st) * Math.pow(1 - st, profile.rimPower));
  }
  peak ||= 1;

  for (let y = 0; y < rh; y++) {
    const py = (y + 0.5) / 2 - cssH / 2;
    for (let x = 0; x < rw; x++) {
      const px = (x + 0.5) / 2 - cssW / 2;
      const nx = Math.sign(px) * Math.max(0, Math.abs(px) - straightX);
      const ny = Math.sign(py) * Math.max(0, Math.abs(py) - straightY);
      const dist = Math.hypot(nx, ny);
      const depth = radius - dist;
      if (depth <= 0 || depth >= bevel || dist <= 1e-4) continue;

      const t = depth / bevel;
      // Inward radial unit vector keeps feDisplacementMap samples inside the glass backdrop box.
      const ix = -nx / dist;
      const iy = -ny / dist;
      const strength = Math.min(
        1,
        (smoothstep(0, profile.entryStop, t) * Math.pow(1 - t, profile.rimPower)) / peak,
      );

      const idx = (y * rw + x) * 4;
      nd[idx] = Math.max(0, Math.min(255, Math.round(128 + ix * strength * 127)));
      nd[idx + 1] = Math.max(0, Math.min(255, Math.round(128 + iy * strength * 127)));
    }
  }

  ctx.putImageData(img, 0, 0);
  return {
    normalHref: canvas.toDataURL(),
    bevel,
  };
}

let filterDefs: SVGDefsElement | undefined;
const prewarmProbes = new Map<string, HTMLElement>();

function prewarmBackdropFilter(filterId: string, filterUrl: string) {
  const previous = prewarmProbes.get(filterId);
  if (previous) {
    previous.remove();
    prewarmProbes.delete(filterId);
  }
  const probe = document.createElement('div');
  probe.setAttribute('aria-hidden', 'true');
  probe.style.cssText = 'position:fixed;left:0;top:0;width:4px;height:4px;border-radius:2px;opacity:0.001;pointer-events:none;z-index:2147483647';
  probe.style.setProperty('-webkit-backdrop-filter', filterUrl);
  probe.style.backdropFilter = filterUrl;
  document.body.append(probe);
  prewarmProbes.set(filterId, probe);
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (probe.isConnected) probe.remove();
        if (prewarmProbes.get(filterId) === probe) prewarmProbes.delete(filterId);
      });
    });
  });
}

function updateLens(id: string, width: number, height: number, profile: LensProfile): 'updated' | 'cached' | 'failed' {
  const w = Math.round(width);
  const h = Math.round(height);
  if (w < 2 || h < 2) return 'failed';
  const geometry = `${w}x${h}`;
  const previous = document.getElementById(id);
  if (previous?.getAttribute('data-geometry') === geometry) return 'cached';

  const map = createNormalMap(w, h, profile);
  if (!map) return 'failed';
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
      href: map.normalHref,
      x: 0,
      y: 0,
      width: w,
      height: h,
      preserveAspectRatio: 'none',
      result: 'map',
    }),
  );

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

  if (previous) previous.replaceWith(filter);
  else filterDefs.append(filter);
  return 'updated';
}

export function updateDockGlass(host: HTMLElement, dock: DOMRect, lensWidth: number, lensHeight: number) {
  const override = (window as Window & {
    __LIQUID_GLASS_UPDATER__?: typeof updateDockGlass;
  }).__LIQUID_GLASS_UPDATER__;
  if (override) {
    override(host, dock, lensWidth, lensHeight);
    return;
  }

  const radius = Math.min(lensWidth, lensHeight) / 2;
  const bevel = Math.min(dropletProfile.bevel, radius * 0.36);
  host.style.setProperty('--dock-lens-bevel', `${bevel}px`);
  // Syntax support alone does not guarantee SVG backdrop compositing.
  if (!/Chrome|Chromium|Edg\//.test(navigator.userAgent)
    || !CSS.supports('backdrop-filter', 'url("#dock-lens")')) return;

  const name = host.id === 'nav' ? 'main' : 'billing';
  const surface = `dock-${name}-glass`;
  const lens = `dock-${name}-lens`;
  const surfaceResult = updateLens(surface, dock.width, dock.height, barProfile);
  if (surfaceResult !== 'failed') {
    host.style.setProperty('--dock-backdrop', `url("#${surface}") blur(1.5px) saturate(1.28)`);
  }
  const lensResult = updateLens(lens, lensWidth, lensHeight, dropletProfile);
  if (lensResult !== 'failed') {
    const lensUrl = `url("#${lens}")`;
    host.style.setProperty('--dock-lens-backdrop', lensUrl);
    if (lensResult === 'updated') {
      prewarmBackdropFilter(lens, lensUrl);
    }
  }
}
