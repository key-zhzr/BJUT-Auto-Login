import { readFile, writeFile } from 'node:fs/promises';
import { REPOSITORY, releaseCatalog, releaseChannels } from './downloads.js';

const path = new URL('./release.json', import.meta.url);
const previous = JSON.parse(await readFile(path, 'utf8'));
const headers = { Accept: 'application/vnd.github+json' };

const [listRes, latestRes] = await Promise.all([
  fetch(`https://api.github.com/repos/${REPOSITORY}/releases?per_page=100`, { headers, signal: AbortSignal.timeout(10000) }),
  fetch(`https://api.github.com/repos/${REPOSITORY}/releases/latest`, { headers, signal: AbortSignal.timeout(10000) }),
]);
if (!listRes.ok && !latestRes.ok) {
  throw new Error(`GitHub returned HTTP ${latestRes.status || listRes.status}; existing download links are unchanged.`);
}
const releases = listRes.ok ? await listRes.json() : [];
const latest = latestRes.ok ? await latestRes.json() : null;
const all = [...(Array.isArray(releases) ? releases : []), ...(latest ? [latest] : [])];
const channels = releaseChannels(all, releaseCatalog(previous), latest?.tag_name || '');
const selected = all.find(release => release.tag_name === channels.release?.tag) || previous;
if (!releaseCatalog(selected)) throw new Error('No public installer assets found');
const { tag_name, draft, prerelease, published_at, html_url } = selected;
const assets = selected.assets.map(({ name, size, browser_download_url }) => ({ name, size, browser_download_url }));
await writeFile(path, JSON.stringify({ tag_name, draft, prerelease, published_at, html_url, assets }, null, 2) + '\n');
console.log(`Verified installer metadata for ${tag_name}; run npm run site:build to update the static download links.`);
