import type {
  GitHubRelease,
  GitHubReleaseAsset,
  OfficialUpdateManifest,
  UpdateTarget,
} from './models.ts';
import { buildExpectedUpdateAssetName } from './update-utils.ts';

const RELEASES_API = 'https://api.github.com/repos/key-zhzr/BJUT-Auto-Login/releases';
const RELEASES_CACHE_KEY = 'bjut_github_releases_cache_v1';
const RELEASES_CACHE_TTL_MS = 30 * 60 * 1000;

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

interface ReleaseCache {
  fetchedAt: number;
  releases: GitHubRelease[];
}

export interface ReleaseLoadResult {
  releases: GitHubRelease[];
  source: 'api' | 'cache' | 'stale-cache';
  warning?: string;
}

function isReleaseAsset(value: unknown): value is GitHubReleaseAsset {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<GitHubReleaseAsset>;
  return typeof candidate.name === 'string'
    && typeof candidate.browser_download_url === 'string'
    && typeof candidate.size === 'number';
}

function isRelease(value: unknown): value is GitHubRelease {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<GitHubRelease>;
  return typeof candidate.tag_name === 'string'
    && (typeof candidate.name === 'string' || candidate.name === null)
    && (typeof candidate.body === 'string' || candidate.body === null)
    && typeof candidate.html_url === 'string'
    && typeof candidate.prerelease === 'boolean'
    && typeof candidate.draft === 'boolean'
    && Array.isArray(candidate.assets)
    && candidate.assets.every(isReleaseAsset);
}

function readCache(storage: StorageLike | null): ReleaseCache | null {
  if (!storage) return null;
  try {
    const parsed = JSON.parse(storage.getItem(RELEASES_CACHE_KEY) ?? 'null') as Partial<ReleaseCache> | null;
    if (!parsed || typeof parsed.fetchedAt !== 'number' || !Array.isArray(parsed.releases)) return null;
    const releases = parsed.releases.filter(isRelease);
    return releases.length > 0 ? { fetchedAt: parsed.fetchedAt, releases } : null;
  } catch {
    return null;
  }
}

function writeCache(storage: StorageLike | null, cache: ReleaseCache): void {
  if (!storage) return;
  try {
    storage.setItem(RELEASES_CACHE_KEY, JSON.stringify(cache));
  } catch {
    // Update checks remain available when WebView storage is unavailable.
  }
}

function defaultStorage(): StorageLike | null {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

function rateLimitMessage(response: Response): string {
  const remaining = response.headers.get('x-ratelimit-remaining');
  const resetSeconds = Number(response.headers.get('x-ratelimit-reset'));
  const reset = Number.isFinite(resetSeconds) && resetSeconds > 0
    ? new Date(resetSeconds * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    : '';
  if ((response.status === 403 || response.status === 429) && remaining === '0') {
    return `GitHub 公共 API 访问额度已用完（HTTP ${response.status}${reset ? `，约 ${reset} 恢复` : ''}）`;
  }
  return `GitHub API 返回 HTTP ${response.status}`;
}

export async function loadGitHubReleases(
  limit: number,
  options: {
    fetchImpl?: typeof fetch;
    storage?: StorageLike | null;
    now?: number;
  } = {},
): Promise<ReleaseLoadResult> {
  const fetchImpl = options.fetchImpl ?? fetch;
  const storage = options.storage === undefined ? defaultStorage() : options.storage;
  const now = options.now ?? Date.now();
  const cache = readCache(storage);
  if (cache && now - cache.fetchedAt <= RELEASES_CACHE_TTL_MS) {
    return { releases: cache.releases.slice(0, limit), source: 'cache' };
  }

  let failure = '';
  try {
    const response = await fetchImpl(`${RELEASES_API}?per_page=${Math.min(Math.max(limit, 1), 100)}`, {
      headers: {
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
      },
    });
    if (!response.ok) {
      failure = rateLimitMessage(response);
    } else {
      const parsed = await response.json() as unknown;
      const releases = Array.isArray(parsed) ? parsed.filter(isRelease) : [];
      if (releases.length === 0) throw new Error('GitHub Release 列表格式异常');
      writeCache(storage, { fetchedAt: now, releases });
      return { releases: releases.slice(0, limit), source: 'api' };
    }
  } catch (error) {
    failure ||= String(error);
  }

  if (cache) {
    return {
      releases: cache.releases.slice(0, limit),
      source: 'stale-cache',
      warning: `${failure}，已使用上次成功读取的发布信息`,
    };
  }
  throw new Error(failure || '无法读取 GitHub Release 列表');
}

function normalizeManifestVersion(version: string): string {
  const normalized = version.trim().replace(/^v/i, '');
  if (normalized.length > 64
    || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.test(normalized)) {
    throw new Error('官方更新清单中的版本号无效');
  }
  return normalized;
}

export function releaseFromOfficialManifest(
  manifest: OfficialUpdateManifest,
  target: UpdateTarget,
  manifestUrl: string,
): GitHubRelease {
  const version = normalizeManifestVersion(manifest.version);
  const tagName = `v${version}`;
  const assetName = buildExpectedUpdateAssetName(version, target);
  if (!assetName) throw new Error('当前平台没有可用的完整安装包格式');
  const releaseBase = `https://github.com/key-zhzr/BJUT-Auto-Login/releases/download/${encodeURIComponent(tagName)}`;
  return {
    tag_name: tagName,
    name: `BJUT-Auto-Login ${tagName}`,
    body: manifest.notes || 'GitHub API 暂不可用，已通过官方签名更新清单完成版本检查。',
    html_url: `https://github.com/key-zhzr/BJUT-Auto-Login/releases/tag/${encodeURIComponent(tagName)}`,
    prerelease: version.includes('-'),
    draft: false,
    assets: [
      {
        name: assetName,
        browser_download_url: `${releaseBase}/${encodeURIComponent(assetName)}`,
        size: 0,
      },
      {
        name: 'latest.json',
        browser_download_url: manifestUrl,
        size: 0,
      },
    ],
  };
}
