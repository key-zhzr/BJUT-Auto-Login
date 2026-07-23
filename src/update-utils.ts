import DOMPurify from 'dompurify';
import { marked } from 'marked';

import type { GitHubReleaseAsset, UpdateTarget } from './models';

export function isVersionNewer(current: string, latest: string): boolean {
  const parseVersion = (value: string) => {
    const withoutBuild = value.replace(/^v/i, '').split('+', 1)[0];
    const [core, prerelease = ''] = withoutBuild.split('-', 2);
    return {
      core: core.split('.').map(part => Number.parseInt(part, 10) || 0),
      prerelease: prerelease ? prerelease.split('.') : [],
    };
  };
  const currentVersion = parseVersion(current);
  const latestVersion = parseVersion(latest);
  for (
    let index = 0;
    index < Math.max(currentVersion.core.length, latestVersion.core.length);
    index += 1
  ) {
    const currentPart = currentVersion.core[index] || 0;
    const latestPart = latestVersion.core[index] || 0;
    if (latestPart > currentPart) return true;
    if (currentPart > latestPart) return false;
  }
  if (currentVersion.prerelease.length === 0) return false;
  if (latestVersion.prerelease.length === 0) return true;
  for (
    let index = 0;
    index < Math.max(currentVersion.prerelease.length, latestVersion.prerelease.length);
    index += 1
  ) {
    const currentPart = currentVersion.prerelease[index];
    const latestPart = latestVersion.prerelease[index];
    if (currentPart === undefined) return true;
    if (latestPart === undefined) return false;
    if (currentPart === latestPart) continue;
    const currentNumber = /^\d+$/.test(currentPart) ? Number(currentPart) : null;
    const latestNumber = /^\d+$/.test(latestPart) ? Number(latestPart) : null;
    if (currentNumber !== null && latestNumber !== null) return latestNumber > currentNumber;
    if (currentNumber !== null) return false;
    if (latestNumber !== null) return true;
    return latestPart.localeCompare(currentPart) > 0;
  }
  return false;
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
