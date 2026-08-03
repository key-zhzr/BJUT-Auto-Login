const SESSION_TTL_SECONDS = 300;
const TOKEN_PATTERN = /^[0-9A-Za-z_-]{32}$/;
const ALLOWED_APP_ORIGINS = new Set([
  'tauri://localhost',
  'http://tauri.localhost',
  'https://tauri.localhost',
]);

const SECURITY_HEADERS = {
  'Cache-Control': 'no-store, max-age=0',
  'Referrer-Policy': 'no-referrer',
  'X-Content-Type-Options': 'nosniff',
  'X-Frame-Options': 'DENY',
  'Permissions-Policy': 'camera=(), microphone=(), geolocation=(), payment=()',
  'Cross-Origin-Resource-Policy': 'same-origin',
};

function json(value, status = 200, extraHeaders = {}) {
  return new Response(JSON.stringify(value), {
    status,
    headers: {
      ...SECURITY_HEADERS,
      ...extraHeaders,
      'Content-Type': 'application/json; charset=utf-8',
    },
  });
}

function allowedOrigin(request) {
  const origin = request.headers.get('Origin') || '';
  return ALLOWED_APP_ORIGINS.has(origin) ? origin : '';
}

function corsHeaders(request) {
  const origin = allowedOrigin(request);
  return origin ? {
    'Access-Control-Allow-Origin': origin,
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type',
    'Access-Control-Max-Age': '600',
    Vary: 'Origin',
  } : {};
}

function hasWechatPaymentMarkers(query) {
  const candidates = [query];
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      const decoded = decodeURIComponent(candidates[candidates.length - 1]);
      if (decoded === candidates[candidates.length - 1]) break;
      candidates.push(decoded);
    } catch {
      break;
    }
  }
  const candidatesAreSafe = candidates.every((candidate) => /^[\x21-\x7e]+$/.test(candidate)
    && !/[\x22\x27<>\\]/.test(candidate));
  return candidatesAreSafe && candidates.some((candidate) => {
    const lower = candidate.toLowerCase();
    return lower.includes('prepay')
      && lower.includes('package')
      && lower.includes('sign');
  });
}

function isTrustedWechatLaunchUrl(value) {
  if (typeof value !== 'string' || value.length > 4096) return false;
  try {
    const url = new URL(value);
    const query = url.search.slice(1);
    return url.protocol === 'weixin:'
      && url.hostname === 'wap'
      && url.pathname === '/pay'
      && !url.username
      && !url.password
      && !url.port
      && !url.hash
      && query.length >= 16
      && query.length <= 3072
      && /^[\x21-\x7e]+$/.test(query)
      && !/[\x22\x27<>\\]/.test(query)
      && hasWechatPaymentMarkers(query);
  } catch {
    return false;
  }
}

function randomToken() {
  const bytes = crypto.getRandomValues(new Uint8Array(24));
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
}

function relayPage() {
  return new Response(`<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>微信支付接力</title><link rel="stylesheet" href="/relay.css"></head>
<body><main><h1>微信支付</h1><p id="message">正在检查打开方式…</p><button id="continue" type="button" hidden>继续微信支付</button></main><script src="/relay.js"></script></body></html>`, {
    headers: {
      ...SECURITY_HEADERS,
      'Content-Type': 'text/html; charset=utf-8',
      'Content-Security-Policy': "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    },
  });
}

const RELAY_SCRIPT = `const message=document.querySelector('#message');const button=document.querySelector('#continue');const token=location.pathname.split('/').pop()||'';if(/MicroMessenger/i.test(navigator.userAgent)){message.textContent='请点击右上角“…”并选择“在浏览器中打开”。';}else{message.textContent='支付入口为短期凭据，请点击后立即完成支付。';button.hidden=false;button.onclick=async()=>{button.disabled=true;message.textContent='正在唤起微信支付…';try{const response=await fetch('/api/payment-sessions/'+encodeURIComponent(token)+'/resolve',{method:'POST',cache:'no-store',credentials:'omit'});const payload=await response.json();if(!response.ok||typeof payload.launchUrl!=='string')throw new Error(payload.error||'支付入口不可用');location.replace(payload.launchUrl);}catch(error){message.textContent='无法继续支付：'+String(error);button.disabled=false;}}}`;

const RELAY_STYLE = `:root{color-scheme:light dark;font-family:system-ui,sans-serif}body{margin:0;min-height:100vh;display:grid;place-items:center;background:#f3f6fb;color:#172033}main{width:min(28rem,calc(100% - 3rem));padding:2rem;border:1px solid #cbd5e1;border-radius:1rem;background:#fff;box-shadow:0 1rem 3rem #0f172a1f}button{width:100%;min-height:3rem;border:0;border-radius:.75rem;background:#1677ff;color:#fff;font:inherit;font-weight:650}@media(prefers-color-scheme:dark){body{background:#000;color:#f5f5f7}main{background:#151515;border-color:#333}}`;

async function createSession(request, env) {
  const origin = allowedOrigin(request);
  if (!origin) return json({ error: 'origin_not_allowed' }, 403);
  const contentLength = Number(request.headers.get('Content-Length') || 0);
  if (contentLength > 8192) return json({ error: 'request_too_large' }, 413, corsHeaders(request));
  let payload;
  try {
    payload = await request.json();
  } catch {
    return json({ error: 'invalid_json' }, 400, corsHeaders(request));
  }
  if (!isTrustedWechatLaunchUrl(payload?.launchUrl)) {
    return json({ error: 'invalid_launch_url' }, 400, corsHeaders(request));
  }
  const token = randomToken();
  await env.PAYMENT_SESSIONS.put(token, JSON.stringify({
    launchUrl: payload.launchUrl,
    resolves: 0,
    createdAt: Date.now(),
  }), { expirationTtl: SESSION_TTL_SECONDS });
  return json({ token, expiresIn: SESSION_TTL_SECONDS }, 201, corsHeaders(request));
}

async function resolveSession(token, env) {
  if (!TOKEN_PATTERN.test(token)) return json({ error: 'invalid_token' }, 404);
  const stored = await env.PAYMENT_SESSIONS.get(token, { type: 'json' });
  if (!stored || !isTrustedWechatLaunchUrl(stored.launchUrl)) {
    return json({ error: 'payment_session_expired' }, 410);
  }
  if (!Number.isInteger(stored.resolves) || stored.resolves >= 3) {
    await env.PAYMENT_SESSIONS.delete(token);
    return json({ error: 'payment_session_exhausted' }, 410);
  }
  stored.resolves += 1;
  await env.PAYMENT_SESSIONS.put(token, JSON.stringify(stored), {
    expirationTtl: SESSION_TTL_SECONDS,
  });
  return json({ launchUrl: stored.launchUrl });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === 'OPTIONS' && url.pathname === '/api/payment-sessions') {
      if (!allowedOrigin(request)) return new Response(null, { status: 403 });
      return new Response(null, { status: 204, headers: corsHeaders(request) });
    }
    if (request.method === 'POST' && url.pathname === '/api/payment-sessions') {
      return createSession(request, env);
    }
    const resolveMatch = url.pathname.match(/^\/api\/payment-sessions\/([0-9A-Za-z_-]{32})\/resolve$/);
    if (request.method === 'POST' && resolveMatch) {
      return resolveSession(resolveMatch[1], env);
    }
    if (request.method === 'GET' && /^\/p\/[0-9A-Za-z_-]{32}$/.test(url.pathname)) {
      return relayPage();
    }
    if (request.method === 'GET' && url.pathname === '/relay.js') {
      return new Response(RELAY_SCRIPT, {
        headers: { ...SECURITY_HEADERS, 'Content-Type': 'text/javascript; charset=utf-8' },
      });
    }
    if (request.method === 'GET' && url.pathname === '/relay.css') {
      return new Response(RELAY_STYLE, {
        headers: { ...SECURITY_HEADERS, 'Content-Type': 'text/css; charset=utf-8' },
      });
    }
    return json({ error: 'not_found' }, 404);
  },
};
