const WECHAT_PAYMENT_RELAY_ORIGIN = 'https://red.bjutdown.work/';
const RELAY_TOKEN_PATTERN = /^[0-9A-Za-z_-]{32,256}$/;

export interface WechatRelaySession {
  url: string;
  expiresIn: number;
}

export function isTrustedWechatLaunchUrl(value: string): boolean {
  try {
    const url = new URL(value);
    const decodedQuery = decodeURIComponent(url.search.slice(1));
    const parameters = new URLSearchParams(decodedQuery);
    const prepayId = parameters.get('prepayid') ?? '';
    const packageValue = parameters.get('package') ?? '';
    const nonce = parameters.get('noncestr') ?? '';
    const timestamp = parameters.get('timestamp') ?? '';
    const signature = parameters.get('sign') ?? '';
    return value.length <= 4096
      && url.protocol === 'weixin:'
      && url.hostname === 'wap'
      && url.pathname === '/pay'
      && !url.username
      && !url.password
      && !url.port
      && !url.hash
      && /^wx[0-9A-Za-z]+$/.test(prepayId)
      && /^[0-9A-Za-z_-]+$/.test(packageValue)
      && /^[0-9A-Za-z_-]+$/.test(nonce)
      && /^\d{9,13}$/.test(timestamp)
      && /^[0-9A-Za-z_-]{16,}$/.test(signature);
  } catch {
    return false;
  }
}

export function buildWechatPaymentRelayUrl(paymentUrl: string): string {
  if (!isTrustedWechatLaunchUrl(paymentUrl)) return '';
  const relay = new URL(WECHAT_PAYMENT_RELAY_ORIGIN);
  // The payment parameters stay in the browser-only fragment and are never
  // included in the HTTPS request, CDN logs, or referrer.
  relay.hash = encodeURIComponent(paymentUrl);
  return relay.toString();
}

/**
 * Create a short-lived opaque relay URL. Unlike the legacy fragment URL, the
 * /p/<token> path survives WeChat's “open in browser” hand-off. The relay must
 * validate the launch URL again, avoid request-body logging, and expire it in
 * no more than ten minutes.
 */
export async function createWechatPaymentRelaySession(
  paymentUrl: string,
  request: typeof fetch = fetch,
): Promise<WechatRelaySession> {
  if (!isTrustedWechatLaunchUrl(paymentUrl)) {
    throw new Error('微信支付地址未通过安全校验');
  }
  const endpoint = new URL('/api/payment-sessions', WECHAT_PAYMENT_RELAY_ORIGIN);
  const response = await request(endpoint, {
    method: 'POST',
    credentials: 'omit',
    cache: 'no-store',
    referrerPolicy: 'no-referrer',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ launchUrl: paymentUrl }),
  });
  if (!response.ok) throw new Error(`支付接力服务返回 HTTP ${response.status}`);
  const payload = await response.json() as { token?: unknown, expiresIn?: unknown };
  const token = typeof payload.token === 'string' ? payload.token : '';
  const expiresIn = Number(payload.expiresIn);
  if (!RELAY_TOKEN_PATTERN.test(token)
    || !Number.isFinite(expiresIn)
    || expiresIn < 30
    || expiresIn > 600) {
    throw new Error('支付接力服务返回了无效的短期会话');
  }
  const relay = new URL(`/p/${token}`, WECHAT_PAYMENT_RELAY_ORIGIN);
  return { url: relay.toString(), expiresIn };
}
