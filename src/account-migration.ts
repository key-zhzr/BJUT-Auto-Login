import { bytesToBase64 } from './config-crypto';
import type { AccountView, LegacyAccount } from './models';

export const LEGACY_ACCOUNTS_KEY = 'bjut_accounts';
export const LEGACY_MIGRATION_PENDING_KEY = 'bjut_accounts_migration_pending';
const LEGACY_XOR_KEY = 'bjut-al-secret-key-2026';

export function readLegacyAccounts(): LegacyAccount[] | null {
  const raw = localStorage.getItem(LEGACY_ACCOUNTS_KEY);
  if (raw === null) return null;
  try {
    const json = raw.trim().startsWith('[')
      ? raw
      : Array.from(atob(raw), (character, index) => String.fromCharCode(
          character.charCodeAt(0) ^ LEGACY_XOR_KEY.charCodeAt(index % LEGACY_XOR_KEY.length),
        )).join('');
    const parsed: unknown = JSON.parse(json);
    if (!Array.isArray(parsed)) return null;
    return parsed
      .map(account => ({
        ...account,
        user: account?.user ?? account?.username,
        pass: account?.pass ?? account?.password,
      }))
      .filter(account => account && typeof account.user === 'string' && typeof account.pass === 'string')
      .map((account, index) => ({
        user: account.user,
        pass: account.pass,
        isDefault: account.isDefault ?? account.is_default ?? index === 0,
        isDisabled: account.isDisabled ?? account.is_disabled ?? false,
      }));
  } catch (error) {
    console.warn('Unable to decode legacy account storage:', error);
    return null;
  }
}

export function mergeLegacyAccounts(
  current: AccountView[],
  legacy: LegacyAccount[],
): { accounts: AccountView[], changed: boolean } {
  let changed = false;
  const accounts = current.map(account => ({ ...account }));
  legacy.forEach(legacyAccount => {
    const currentAccount = accounts.find(account => account.user === legacyAccount.user);
    if (!currentAccount) {
      accounts.push({
        ...legacyAccount,
        hasPassword: Boolean(legacyAccount.pass),
        isDefault: accounts.some(account => account.isDefault) ? false : legacyAccount.isDefault,
      });
      changed = true;
    } else if (!currentAccount.hasPassword && !currentAccount.pass && legacyAccount.pass) {
      currentAccount.pass = legacyAccount.pass;
      changed = true;
    }
  });
  if (accounts.length > 0 && !accounts.some(account => account.isDefault)) {
    accounts[0].isDefault = true;
    changed = true;
  }
  return { accounts, changed };
}

export function hasLegacyCredentialConflict(
  current: AccountView[],
  legacy: LegacyAccount[],
): boolean {
  return legacy.some(legacyAccount => {
    if (!legacyAccount.pass) return false;
    const currentAccount = current.find(account => account.user === legacyAccount.user);
    return currentAccount?.hasPassword === true
      || (currentAccount?.pass ? currentAccount.pass !== legacyAccount.pass : false);
  });
}

export async function credentialSnapshotFingerprint(
  accounts: Pick<AccountView, 'user' | 'pass'>[],
): Promise<string> {
  const snapshot = accounts.map(account => ({
    user: String(account.user ?? ''),
    pass: String(account.pass ?? ''),
  }));
  const digest = await crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(JSON.stringify(snapshot)),
  );
  return bytesToBase64(new Uint8Array(digest));
}
