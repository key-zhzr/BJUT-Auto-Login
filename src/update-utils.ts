import DOMPurify from 'dompurify';
import { marked } from 'marked';

import type { GitHubReleaseAsset, UpdateTarget } from './models';

export function compareVersions(current: string, latest: string): -1 | 0 | 1 | null {
  const parseVersion = (value: string) => {
    const match = value.trim().match(
      /^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/,
    );
    if (!match) return null;
    const prerelease = match[4]?.split('.') ?? [];
    if (prerelease.some(identifier => /^\d+$/.test(identifier) && /^0\d+/.test(identifier))) {
      return null;
    }
    return {
      core: [BigInt(match[1]), BigInt(match[2]), BigInt(match[3])],
      prerelease,
    };
  };
  const currentVersion = parseVersion(current);
  const latestVersion = parseVersion(latest);
  if (!currentVersion || !latestVersion) return null;
  for (
    let index = 0;
    index < currentVersion.core.length;
    index += 1
  ) {
    const currentPart = currentVersion.core[index];
    const latestPart = latestVersion.core[index];
    if (latestPart > currentPart) return -1;
    if (currentPart > latestPart) return 1;
  }
  if (currentVersion.prerelease.length === 0) return latestVersion.prerelease.length === 0 ? 0 : 1;
  if (latestVersion.prerelease.length === 0) return -1;
  for (
    let index = 0;
    index < Math.max(currentVersion.prerelease.length, latestVersion.prerelease.length);
    index += 1
  ) {
    const currentPart = currentVersion.prerelease[index];
    const latestPart = latestVersion.prerelease[index];
    if (currentPart === undefined) return -1;
    if (latestPart === undefined) return 1;
    if (currentPart === latestPart) continue;
    const currentNumber = /^\d+$/.test(currentPart) ? BigInt(currentPart) : null;
    const latestNumber = /^\d+$/.test(latestPart) ? BigInt(latestPart) : null;
    if (currentNumber !== null && latestNumber !== null) return latestNumber > currentNumber ? -1 : 1;
    // SemVer 2.0.0: numeric identifiers always have lower precedence than
    // non-numeric identifiers.
    if (currentNumber !== null) return -1;
    if (latestNumber !== null) return 1;
    return latestPart > currentPart ? -1 : 1;
  }
  return 0;
}

export function isBuildTimestamp(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value)
    && value > 0 && value <= 253_402_300_799;
}

export function formatBuildTimestamp(timestamp: number): string {
  return new Intl.DateTimeFormat('zh-CN', {
    dateStyle: 'medium', timeStyle: 'medium', hourCycle: 'h23',
  }).format(timestamp * 1000);
}

export function selectUpdateAsset(
  assets: GitHubReleaseAsset[],
  target: UpdateTarget,
): GitHubReleaseAsset | undefined {
  const expectedSuffix = (() => {
    switch (target.platform) {
      case 'android': return `_Android_${target.arch}.apk`;
      case 'windows': return `_Windows_${target.arch}.exe`;
      case 'macos': return `_macOS_${target.arch}.dmg`;
      case 'linux': return `_Linux_${target.arch}.${target.format}`;
      default: return '';
    }
  })().toLowerCase();
  if (!expectedSuffix) return undefined;
  return assets.find(asset => asset.name.toLowerCase().endsWith(expectedSuffix));
}

export function buildExpectedUpdateAssetName(version: string, target: UpdateTarget): string {
  const normalizedVersion = version.replace(/^v/i, '');
  switch (target.platform) {
    case 'android': return `BJUT-Auto-Login_${normalizedVersion}_Android_${target.arch}.apk`;
    case 'windows': return `BJUT-Auto-Login_${normalizedVersion}_Windows_${target.arch}.exe`;
    case 'macos': return `BJUT-Auto-Login_${normalizedVersion}_macOS_${target.arch}.dmg`;
    case 'linux': return `BJUT-Auto-Login_${normalizedVersion}_Linux_${target.arch}.${target.format}`;
    default: return '';
  }
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '未知大小';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

export async function renderReleaseNotes(markdown: string): Promise<string> {
  const rendered = await marked.parse(markdown || '本次发布未提供更新说明。', {
    gfm: true,
    breaks: true,
  });
  return DOMPurify.sanitize(rendered, { USE_PROFILES: { html: true } });
}
