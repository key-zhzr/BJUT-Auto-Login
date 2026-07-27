const WECHAT_PAYMENT_RELAY_ORIGIN = 'https://red.bjutdown.work/';

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
