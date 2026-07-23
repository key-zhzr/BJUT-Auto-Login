export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  bytes.forEach(byte => { binary += String.fromCharCode(byte); });
  return btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
  return Uint8Array.from(atob(value), char => char.charCodeAt(0));
}

async function deriveExportKey(passphrase: string, salt: Uint8Array): Promise<CryptoKey> {
  const material = await crypto.subtle.importKey(
    'raw',
    new TextEncoder().encode(passphrase),
    'PBKDF2',
    false,
    ['deriveKey'],
  );
  return crypto.subtle.deriveKey(
    { name: 'PBKDF2', salt, iterations: 250000, hash: 'SHA-256' },
    material,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt'],
  );
}

export async function encryptExport(data: unknown, passphrase: string): Promise<string> {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveExportKey(passphrase, salt);
  const plaintext = new TextEncoder().encode(JSON.stringify(data));
  const ciphertext = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, plaintext);
  plaintext.fill(0);
  return JSON.stringify({
    version: 2,
    salt: bytesToBase64(salt),
    iv: bytesToBase64(iv),
    ciphertext: bytesToBase64(new Uint8Array(ciphertext)),
  });
}

export async function decryptExport(
  value: string,
  passphrase: string,
): Promise<Record<string, unknown>> {
  const envelope = JSON.parse(value) as Record<string, unknown>;
  const { salt, iv, ciphertext } = envelope;
  if (
    envelope.version !== 2
    || typeof salt !== 'string'
    || typeof iv !== 'string'
    || typeof ciphertext !== 'string'
  ) {
    throw new Error('不是受支持的加密配置格式');
  }
  const key = await deriveExportKey(passphrase, base64ToBytes(salt));
  const plaintext = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: base64ToBytes(iv) },
    key,
    base64ToBytes(ciphertext),
  );
  const plaintextBytes = new Uint8Array(plaintext);
  const decoded = new TextDecoder().decode(plaintextBytes);
  plaintextBytes.fill(0);
  const result: unknown = JSON.parse(decoded);
  if (!result || typeof result !== 'object' || Array.isArray(result)) {
    throw new Error('加密配置内容不是有效对象');
  }
  return result as Record<string, unknown>;
}
