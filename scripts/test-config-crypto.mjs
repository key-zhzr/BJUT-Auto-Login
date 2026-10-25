import assert from 'node:assert/strict';

// config-crypto.ts / config-transfer-dialog.ts 专项测试（任务 31）。
// 覆盖：v2 信封加解密回环、旧格式与损坏输入的中文诊断、备份密码强度提示。
// 运行：node --experimental-strip-types scripts/test-config-crypto.mjs

const { encryptExport, decryptExport } = await import('../src/config-crypto.ts');
const { passphraseStrengthHint } = await import('../src/config-transfer-dialog.ts');

const payload = {
  accounts: [
    { user: '2023021234', pass: '校园网p@ss word', isDefault: true },
    { user: '2023056789', pass: '', hasPassword: true },
  ],
  settings: { theme: 'basic', checkInterval: 15 },
  network_profiles: [{ id: 'dorm', name: '宿舍' }],
};

// —— v2 信封回环 ——
{
  const envelopeText = await encryptExport(payload, '正确的口令');
  const envelope = JSON.parse(envelopeText);
  assert.equal(envelope.version, 2);
  assert.equal(typeof envelope.salt, 'string');
  assert.equal(typeof envelope.iv, 'string');
  assert.equal(typeof envelope.ciphertext, 'string');
  assert.deepEqual(await decryptExport(envelopeText, '正确的口令'), payload);
  console.log('v2 envelope round-trip (Chinese passphrase, hasPassword accounts) passed');

  // 相同口令再次导出得到新的随机盐/IV，但内容一致
  const second = JSON.parse(await encryptExport(payload, '正确的口令'));
  assert.notEqual(second.salt, envelope.salt, '每次导出必须使用新的随机盐');
  assert.deepEqual(await decryptExport(JSON.stringify(second), '正确的口令'), payload);
  console.log('Fresh random salt/iv per export passed');
}

// —— 解密失败路径 ——
{
  const envelopeText = await encryptExport(payload, '正确的口令');
  await assert.rejects(
    () => decryptExport(envelopeText, '错误的口令'),
    /解密失败：备份密码错误，或备份内容已损坏/,
  );
  const envelope = JSON.parse(envelopeText);
  const flipped = envelope.ciphertext.slice(0, -4) + 'AAAA';
  await assert.rejects(
    () => decryptExport(JSON.stringify({ ...envelope, ciphertext: flipped }), '正确的口令'),
    /解密失败：备份密码错误，或备份内容已损坏/,
  );
  console.log('Wrong passphrase and tampered ciphertext rejected with readable message passed');
}

// —— 旧格式 / 无法识别输入的诊断 ——
{
  await assert.rejects(
    () => decryptExport('这不是 JSON', '任意口令'),
    /备份内容不是有效的 JSON 文本/,
  );
  await assert.rejects(
    () => decryptExport(JSON.stringify({ version: 3, salt: 'AA', iv: 'AA', ciphertext: 'AA' }), '任意口令'),
    /新版应用导出的加密备份（v3\/v4）/,
  );
  await assert.rejects(
    () => decryptExport('BJUT4:c2FtcGxl', '任意口令'),
    /新版应用导出的加密备份（v3\/v4）/,
  );
  await assert.rejects(
    () => decryptExport(JSON.stringify({ accounts: [{ user: '2023021234' }] }), '任意口令'),
    /未加密的旧版明文备份/,
  );
  await assert.rejects(
    () => decryptExport(JSON.stringify({ version: 1, salt: 'AA', iv: 'AA', ciphertext: 'AA' }), '任意口令'),
    /备份版本 v1/,
  );
  await assert.rejects(
    () => decryptExport(JSON.stringify({ foo: 1 }), '任意口令'),
    /不是受支持的加密配置格式/,
  );
  await assert.rejects(
    () => decryptExport(JSON.stringify({ version: 2 }), '任意口令'),
    /缺少必要字段/,
  );
  await assert.rejects(
    () => decryptExport(JSON.stringify({ version: 2, salt: '!!!', iv: 'AAAA', ciphertext: 'BBBB' }), '任意口令'),
    /不是有效的 Base64/,
  );
  console.log('Legacy plaintext / v1 / v3-v4 / truncated and corrupted inputs produce actionable messages passed');
}

// —— 解密成功但内容异常 ——
{
  await assert.rejects(
    async () => decryptExport(await encryptExport('纯字符串', '口令'), '口令'),
    /加密配置内容不是有效对象/,
  );
  console.log('Decrypted non-object payload rejected passed');
}

// —— 备份密码强度提示（3 位提交下限不变，提示仅供参考） ——
{
  assert.equal(passphraseStrengthHint(''), '');
  assert.match(passphraseStrengthHint('ab'), /至少需要 3 个字符/);
  assert.match(passphraseStrengthHint('1234567'), /强度偏低/);
  assert.match(passphraseStrengthHint('12345678'), /强度偏低/, '纯数字 8 位仍属偏低');
  assert.match(passphraseStrengthHint('aaaaaaaaaaaa'), /强度偏低/, '重复字符 12 位仍属偏低');
  assert.match(passphraseStrengthHint('abcdefgh'), /强度中等/);
  assert.match(passphraseStrengthHint('Str0ng!Backup2026'), /强度良好/);
  // 提示是纯函数，不可能改变提交校验：3 位口令不触发“至少 3 个字符”以外的阻断
  assert.doesNotMatch(passphraseStrengthHint('abc'), /至少需要 3 个字符/);
  console.log('Passphrase strength hints (3-char floor untouched) passed');
}

console.log('config-crypto / config-transfer dedicated tests: all passed');
