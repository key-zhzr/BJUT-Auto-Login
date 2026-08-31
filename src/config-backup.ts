interface ImportedCredentialSummary {
  user: string;
  pass: string;
  hasPassword: boolean;
}

interface SavedCredentialSummary {
  user: string;
  hasPassword: boolean;
}

export function missingImportedCredentialUsers(
  imported: ImportedCredentialSummary[],
  current: SavedCredentialSummary[],
): string[] {
  const currentlySaved = new Set(
    current.filter(account => account.hasPassword).map(account => account.user),
  );
  return imported
    .filter(account => account.hasPassword && !account.pass && !currentlySaved.has(account.user))
    .map(account => account.user);
}
