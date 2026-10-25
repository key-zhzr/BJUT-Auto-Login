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

// 旧版备份路径（main.ts 在 v3/v4 信封走 Rust 之前才会调用到这里）。各种
// 无法识别/已损坏的输入在此给出可操作的中文诊断，而不是裸的解析器异常；
// 全程只读，不会改动任何已保存的凭据。
export async function decryptExport(
  value: string,
  passphrase: string,
): Promise<Record<string, unknown>> {
  const trimmed = value.trim();
  if (trimmed.startsWith('BJUT4:')) {
    throw new Error('这是新版应用导出的加密备份（v3/v4），请直接在应用内导入，无需输入旧版备份密码流程。');
  }
  let envelope: Record<string, unknown>;
  try {
    envelope = JSON.parse(trimmed) as Record<string, unknown>;
  } catch {
    throw new Error('备份内容不是有效的 JSON 文本，请确认复制的是完整的加密备份。');
  }
  if (!envelope || typeof envelope !== 'object' || Array.isArray(envelope)) {
    throw new Error('备份内容不是有效的配置对象。');
  }
  if (envelope.version === 3 || envelope.version === 4) {
    throw new Error('这是新版应用导出的加密备份（v3/v4），请直接在应用内导入，无需输入旧版备份密码流程。');
  }
  const { version, salt, iv, ciphertext } = envelope;
  if (typeof version !== 'number') {
    if ('accounts' in envelope || 'settings' in envelope || 'network_profiles' in envelope) {
      throw new Error('这是未加密的旧版明文备份。为保护账号密码，应用不再支持直接导入明文备份；请用当前版本重新导出加密备份。');
    }
    throw new Error('不是受支持的加密配置格式。');
  }
  if (version !== 2) {
    throw new Error(`不是受支持的加密配置格式（备份版本 v${version}，当前支持 v2）。`);
  }
  if (typeof salt !== 'string' || typeof iv !== 'string' || typeof ciphertext !== 'string') {
    throw new Error('加密备份缺少必要字段（salt/iv/ciphertext），内容可能不完整。');
  }
  let saltBytes: Uint8Array;
  let ivBytes: Uint8Array;
  let ciphertextBytes: Uint8Array;
  try {
    saltBytes = base64ToBytes(salt);
    ivBytes = base64ToBytes(iv);
    ciphertextBytes = base64ToBytes(ciphertext);
  } catch {
    throw new Error('备份中的加密字段不是有效的 Base64 数据，备份可能已损坏。');
  }
  const key = await deriveExportKey(passphrase, saltBytes);
  let plaintext: ArrayBuffer;
  try {
    plaintext = await crypto.subtle.decrypt(
      { name: 'AES-GCM', iv: ivBytes },
      key,
      ciphertextBytes,
    );
  } catch {
    throw new Error('解密失败：备份密码错误，或备份内容已损坏。');
  }
  const plaintextBytes = new Uint8Array(plaintext);
  const decoded = new TextDecoder().decode(plaintextBytes);
  plaintextBytes.fill(0);
  let result: unknown;
  try {
    result = JSON.parse(decoded);
  } catch {
    throw new Error('解密成功，但备份内容已损坏（不是有效的 JSON）。');
  }
  if (!result || typeof result !== 'object' || Array.isArray(result)) {
    throw new Error('加密配置内容不是有效对象');
  }
  return result as Record<string, unknown>;
}
