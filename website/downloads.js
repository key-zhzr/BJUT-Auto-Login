export const REPOSITORY = 'key-zhzr/BJUT-Auto-Login';
export const OPENWRT_REPOSITORY = 'key-zhzr/BJUT-Auto-Login-OpenWrt';
export const PLATFORMS = { windows: 'Windows', macos: 'macOS', linux: 'Linux', android: 'Android' };
const filename = /^BJUT-Auto-Login_(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)_(Windows|macOS|Linux|Android)_(x64|x86_64|arm64)(_(Offline))?\.(exe|dmg|deb|AppImage|apk)$/;
const sha256Pattern = /^[0-9a-f]{64}$/;

function validGithubReleaseUrl(rawUrl, repo, tag, name) {
  let url;
  try { url = new URL(rawUrl); } catch { return null; }
  if (url.origin !== 'https://github.com' || url.username || url.password || url.search || url.hash
    || url.pathname !== `/${repo}/releases/download/${tag}/${name}`) return null;
  return url.href;
}

export function releaseCatalog(release, {channel = 'release'} = {}) {
  if (!release || release.draft || !!release.prerelease !== (channel === 'prerelease') || !/^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(release.tag_name)) return null;
  if (channel === 'release' && release.tag_name.includes('-')) return null;
  const version = release.tag_name.replace(/^v/, '');
  const assets = (Array.isArray(release.assets) ? release.assets : []).flatMap(asset => {
    const match = typeof asset.name === 'string' && asset.name.match(filename);
    if (!match || match[1] !== version) return [];
    const platform = match[2].toLowerCase();
    const format = match[6];
    if (!({windows:['exe'],macos:['dmg'],linux:['deb','AppImage'],android:['apk']}[platform]?.includes(format))) return [];
    if (match[5] && platform !== 'windows') return [];
    const href = validGithubReleaseUrl(asset.browser_download_url, REPOSITORY, release.tag_name, asset.name);
    if (!href) return [];
    const arch = match[3] === 'x86_64' ? 'x64' : match[3];
    const variant = match[5] ? 'offline' : format;
    return [{id:`${platform}-${arch}-${variant}`, platform, arch, format, offline:!!match[5], name:asset.name,
      url:href, size:Number.isSafeInteger(asset.size) && asset.size > 0 ? asset.size : null}];
  });
  if (!assets.length) return null;
  return {version, tag:release.tag_name, assets, prerelease:!!release.prerelease, publishedAt:release.published_at || ''};
}

export function releaseChannels(releases, fallback, latestTag = '') {
  const list = Array.isArray(releases) ? releases : [];
  const latestCatalog = latestTag && typeof latestTag === 'object' ? releaseCatalog(latestTag) : null;
  const resolvedTag = typeof latestTag === 'string' ? latestTag : (latestCatalog?.tag || '');
  const stable = list.map(value => releaseCatalog(value)).filter(Boolean);
  if (latestCatalog && !stable.some(item => item.tag === latestCatalog.tag)) stable.unshift(latestCatalog);
  const previewSeen = new Set();
  const preview = list.map(value => releaseCatalog(value, {channel:'prerelease'})).filter(item => {
    if (!item || previewSeen.has(item.tag)) return false;
    previewSeen.add(item.tag);
    return true;
  }).sort((a,b) => (Number.isFinite(Date.parse(b.publishedAt)) ? Date.parse(b.publishedAt) : 0) - (Number.isFinite(Date.parse(a.publishedAt)) ? Date.parse(a.publishedAt) : 0));
  const matchedLatest = resolvedTag ? (stable.find(value => value.tag === resolvedTag) || (fallback?.tag === resolvedTag ? fallback : null)) : null;
  return {release:matchedLatest || newestCatalog(list, fallback), prerelease:preview[0] || null};
}

export function newestCatalog(releases, fallback) {
  const candidates = (Array.isArray(releases) ? releases : [releases]).map(value => releaseCatalog(value)).filter(Boolean);
  if (fallback) candidates.push(fallback);
  return candidates.sort((a,b) => {
    const aa=a.version.split('.').map(Number), bb=b.version.split('.').map(Number);
    return bb[0]-aa[0] || bb[1]-aa[1] || bb[2]-aa[2];
  })[0] || null;
}

export function assetLabel(asset) {
  const arch = asset.arch === 'arm64' ? (asset.platform === 'macos' ? 'Apple 芯片 · ARM64' : 'ARM64')
    : (asset.platform === 'macos' ? 'Intel · x64' : asset.platform === 'android' ? 'x86_64' : 'x64');
  return `${arch} · ${asset.offline ? '离线安装包' : asset.format === 'AppImage' ? 'AppImage' : asset.format.toUpperCase()}`;
}

export function formatSize(size) {
  return size ? `${(size / 1024 / 1024).toFixed(1)} MB` : '';
}

export function formatPackageSize(size) {
  if (!Number.isSafeInteger(size) || size <= 0) return '';
  return size >= 1024 * 1024 ? `${(size / 1024 / 1024).toFixed(1)} MB` : `${(size / 1024).toFixed(1)} KB`;
}

export function detectDevice({ua='', platform='', hints={}, touches=0, renderer=''} = {}) {
  const osHint = String(hints.platform || '').toLowerCase();
  // iPad desktop UA says Macintosh/MacIntel; touch support distinguishes it.
  if (/iPhone|iPad|iPod/i.test(ua) || (/Mac/i.test(platform + ua) && touches > 1)) return {platform:'ios', arch:null, certain:true};
  if (/CrOS/i.test(ua) || osHint === 'chrome os') return {platform:'chromeos', arch:null, certain:true};
  // Via/WebView in "Desktop mode" on Android phones may spoof "X11; Linux x86_64"
  // while navigator.platform still exposes the ARM Linux ABI and touch points > 1.
  const androidDesktopSpoof = /X11;\s*Linux\s+x86_64/i.test(ua) && /armv8l|armv81|armv8\b|armv7l|aarch64|arm64/i.test(platform) && touches > 1;
  const os = osHint === 'android' || /Android|Adr\b/i.test(ua) || androidDesktopSpoof ? 'android'
    : osHint === 'windows' || /Windows/i.test(ua) || /^Win/i.test(platform) ? 'windows'
    : osHint === 'macos' || /Macintosh|Mac OS X/i.test(ua) || /^Mac/i.test(platform) ? 'macos'
    : osHint === 'linux' || /Linux|X11/i.test(ua + platform) ? 'linux' : null;
  let arch = null;
  const architecture = String(hints.architecture || '').toLowerCase();
  const bits = String(hints.bitness || '');
  if (/^(arm64|aarch64)$/.test(architecture)) arch = 'arm64';
  else if (architecture === 'arm') arch = bits === '32' ? (os === 'android' ? null : 'arm32') : bits === '64' ? 'arm64' : null;
  if (/^(x86_64|amd64|x64)$/.test(architecture)) arch = 'x64';
  else if (architecture === 'x86') arch = bits === '64' || hints.wow64 === true ? 'x64' : bits === '32' ? (os === 'android' ? null : 'x86') : null;
  if (!arch) {
    const tokens = androidDesktopSpoof ? platform : (ua + ' ' + platform);
    if (/aarch64|arm64/i.test(tokens)) arch = 'arm64';
    else if (os !== 'android' && /armv[5-8]l|armv7/i.test(tokens)) arch = 'arm32';
    else if (os === 'android' && /armv8l|armv81|armv8\b/i.test(platform)) return {platform:os, arch:'arm64', certain:false};
    else if (os !== 'macos' && /x86_64|x64|amd64|Win64|WOW64/i.test(tokens)) arch = 'x64';
    else if (os === 'linux' && /i[3-6]86/i.test(platform + ua)) arch = 'x86';
  }
  // MacIntel and "Intel Mac OS X" also appear on Apple Silicon. Only explicit
  // renderer families may refine the fallback; generic "Apple GPU" is unknown.
  if (!arch && os === 'macos') {
    if (/Apple\s+M\d+(?:\b|\s)/i.test(renderer)) arch = 'arm64';
    else if (/\bIntel\b|\bAMD\b|\bRadeon\b/i.test(renderer)) arch = 'x64';
  }
  // Via/WebView may expose ARMv8/ARMv7 or 32-bit x86 browser ABIs without the OS bitness.
  // Recommend the matching Android package family, but never label it a confirmed detection.
  if (!arch && os === 'android') {
    if (/armv8l|armv81|armv8\b|armv7l|armv7\b/i.test(platform + ' ' + ua) || architecture === 'arm') {
      return {platform:os, arch:'arm64', certain:false};
    }
    if (/\bi[3-6]86\b|\bx86\b/i.test(platform + ' ' + ua) || architecture === 'x86') {
      return {platform:os, arch:'x64', certain:false};
    }
  }
  return {platform:os, arch, certain:!!arch};
}

export function selectDeviceArch(device, manualArch = '') {
  if (manualArch === 'arm64' || manualArch === 'x64') {
    return {...device, platform:device?.platform || 'android', arch:manualArch, certain:true, manual:true};
  }
  return {...device, manual:false};
}

export function recommendDownload(device, catalog) {
  if (!PLATFORMS[device.platform]) return {asset:null, note:device.platform === 'ios' ? 'iPhone / iPad 暂无客户端，可查看其他平台。' : '请选择适合设备的安装包。'};
  // Unknown architecture stays explicit. Intel builds can run through Rosetta
  // on Apple Silicon; offer its native ARM64 option immediately alongside it.
  const arch = device.arch || (device.platform === 'android' ? 'arm64' : 'x64');
  const format = {windows:'exe',macos:'dmg',linux:'AppImage',android:'apk'}[device.platform];
  const asset = catalog?.assets.find(asset => asset.platform === device.platform && asset.arch === arch && asset.format === format && !asset.offline);
  if (!asset) return {asset:null, note:`当前版本暂无 ${PLATFORMS[device.platform]} ${arch === 'arm64' ? 'ARM64' : arch} 安装包。`};
  let note = `${PLATFORMS[device.platform]} · ${assetLabel(asset)}${asset.size ? ` · ${formatSize(asset.size)}` : ''}`;
  if (device.manual && device.platform === 'android') {
    note += arch === 'arm64'
      ? '。已手动选择 ARM64（普通手机）；模拟器或 Intel 设备可切换为 x86_64。'
      : '。已手动选择 x86_64（模拟器 / Intel 设备）；普通手机可切换为 ARM64。';
  } else if (!device.certain) {
    note += device.platform === 'macos'
      ? '。芯片未确认，Apple 芯片可选下方 ARM64 版。'
      : device.platform === 'android'
        ? (arch === 'x64'
          ? '。浏览器未提供精确位数，已按 x86 环境推荐 x86_64；普通手机请手动切换为 ARM64。'
          : '。浏览器未提供精确位数，推荐普通手机使用 ARM64；模拟器或 Intel 设备请选择 x86_64。')
        : '。架构未确认，可在下方切换。';
  }
  return {asset, note};
}

export async function readDevice(nav = navigator, readRenderer = () => '') {
  let hints = {platform:nav.userAgentData?.platform};
  let timer;
  try {
    const values = await Promise.race([
      nav.userAgentData?.getHighEntropyValues?.(['architecture','bitness','wow64','platform']),
      new Promise(resolve => { timer = setTimeout(() => resolve({}), 600); }),
    ]);
    hints = {...hints, ...values};
  } catch { /* UA/OS fallback remains available when hints are restricted. */ }
  finally { clearTimeout(timer); }
  const input = {ua:nav.userAgent, platform:nav.platform, touches:nav.maxTouchPoints, hints};
  const device = detectDevice(input);
  if (device.platform === 'macos' && !device.arch) {
    try { return detectDevice({...input, renderer:readRenderer()}); } catch { /* GPU access is optional. */ }
  }
  return device;
}

export function openwrtMatrix(config) {
  if (!config || !Array.isArray(config.releases) || !Array.isArray(config.platforms)) {
    throw new Error('Missing OpenWrt build-targets configuration');
  }
  const rows = [];
  const seen = new Set();
  for (const release of config.releases) {
    if (!release || !['apk', 'ipk'].includes(release.format) || !/^\d+\.\d+\.\d+$/.test(release.version)) {
      throw new Error('Invalid OpenWrt release entry');
    }
    for (const target of config.platforms) {
      if (!target || typeof target.target !== 'string' || typeof target.arch !== 'string') {
        throw new Error('Invalid OpenWrt platform entry');
      }
      const allowedFormats = Array.isArray(target.formats) ? target.formats : ['ipk', 'apk'];
      if (!allowedFormats.includes(release.format)) continue;
      const sdk = `${target.target.replace('/', '-')}-${release.version}`;
      const id = `openwrt-${sdk}-${release.format}`;
      if (seen.has(id)) throw new Error(`Duplicate OpenWrt SDK target: ${id}`);
      seen.add(id);
      rows.push({
        id,
        sdk,
        version: release.version,
        format: release.format,
        manager: release.manager || (release.format === 'apk' ? 'apk' : 'opkg'),
        target: target.target,
        arch: target.arch,
        devices: target.devices || '',
      });
    }
  }
  return {releases: config.releases, platforms: config.platforms, rows};
}

export function parseSha256Sums(text = '') {
  const sums = new Map();
  if (typeof text !== 'string') return sums;
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    const match = line.match(/^([0-9a-f]{64})\s+\*?([A-Za-z0-9_.-]+)$/);
    if (match) sums.set(match[2], match[1]);
  }
  return sums;
}

function validDigest(value) {
  if (typeof value !== 'string') return null;
  const normalized = value.startsWith('sha256:') ? value.slice(7) : value;
  return sha256Pattern.test(normalized) ? normalized : null;
}

function parseOpenwrtPackageAsset(row, assetName) {
  if (typeof assetName !== 'string' || !assetName.startsWith(`${row.id}__`)) return null;
  const rest = assetName.slice(row.id.length + 2);
  if (row.format === 'apk') {
    const match = rest.match(/^(bjut-autologin|luci-app-bjut-autologin)-(\d+\.\d+\.\d+-r\d+)\.apk$/);
    if (!match) return null;
    return {package: match[1], version: match[2], arch: row.arch};
  }
  const match = rest.match(/^(bjut-autologin|luci-app-bjut-autologin)_(\d+\.\d+\.\d+-(?:r)?\d+)_([A-Za-z0-9_.-]+)\.ipk$/);
  if (!match) return null;
  const expectedArch = match[1] === 'bjut-autologin' ? row.arch : 'all';
  if (match[3] !== expectedArch) return null;
  return {package: match[1], version: match[2], arch: match[3]};
}

function validateBuildRecord(record, row, repo, fallbackTag = '') {
  if (!record || record.id !== row.id || record.target !== row.target || record.arch !== row.arch
    || record.format !== row.format || record.version !== row.version || !Array.isArray(record.files)
    || record.files.length !== 2) return null;
  const tag = typeof record.tag === 'string' && /^openwrt-v\d+\.\d+\.\d+(?:-r\d+)?$/.test(record.tag) ? record.tag : fallbackTag;
  const parsedFiles = {};
  let pairVersion = '';
  for (const file of record.files) {
    if (!file || typeof file.name !== 'string') return null;
    const parsed = parseOpenwrtPackageAsset(row, file.name);
    const sha256 = validDigest(file.sha256);
    const size = Number.isSafeInteger(file.size) && file.size > 0 ? file.size : null;
    if (!parsed || parsed.package !== file.package || !sha256 || !size || parsedFiles[parsed.package]) return null;
    if (pairVersion && pairVersion !== parsed.version) return null;
    pairVersion = parsed.version;
    const url = file.url
      ? validGithubReleaseUrl(file.url, repo, tag, file.name)
      : (tag ? `https://github.com/${repo}/releases/download/${tag}/${file.name}` : null);
    parsedFiles[parsed.package] = {
      package: parsed.package,
      name: file.name,
      version: parsed.version,
      sha256,
      size,
      url,
    };
  }
  if (!parsedFiles['bjut-autologin'] || !parsedFiles['luci-app-bjut-autologin']) return null;
  return {
    tag: tag || null,
    packageVersion: record.package_version || pairVersion.replace(/-r?\d+$/, ''),
    packageRevision: pairVersion,
    sourceCommit: typeof record.source_commit === 'string' && /^[0-9a-f]{40}$/.test(record.source_commit) ? record.source_commit : null,
    sdkContainer: typeof record.sdk_container === 'string' ? record.sdk_container : `ghcr.io/openwrt/sdk:${row.sdk}`,
    files: [parsedFiles['bjut-autologin'], parsedFiles['luci-app-bjut-autologin']],
  };
}

export function openwrtCatalog(release, config, {builds = [], checksums = ''} = {}) {
  const matrix = openwrtMatrix(config);
  const repo = config.repository || OPENWRT_REPOSITORY;
  const sums = parseSha256Sums(checksums);
  const buildList = [
    ...(Array.isArray(builds) ? builds : []),
    ...(Array.isArray(config.verifiedBuilds) ? config.verifiedBuilds : []),
  ];

  let releaseTag = null;
  let publishedAt = '';
  let buildsJsonUrl = null;
  let sha256SumsUrl = null;
  const releaseAssetsByName = new Map();

  if (release && !release.draft && typeof release.tag_name === 'string' && /^openwrt-v\d+\.\d+\.\d+(?:-r\d+)?$/.test(release.tag_name)) {
    releaseTag = release.tag_name;
    publishedAt = release.published_at || '';
    for (const asset of Array.isArray(release.assets) ? release.assets : []) {
      if (!asset || typeof asset.name !== 'string') continue;
      const href = validGithubReleaseUrl(asset.browser_download_url, repo, releaseTag, asset.name);
      if (!href) continue;
      if (asset.name === 'builds.json') { buildsJsonUrl = href; continue; }
      if (asset.name === 'SHA256SUMS') { sha256SumsUrl = href; continue; }
      releaseAssetsByName.set(asset.name, {
        name: asset.name,
        url: href,
        size: Number.isSafeInteger(asset.size) && asset.size > 0 ? asset.size : null,
        sha256: validDigest(asset.digest) || sums.get(asset.name) || null,
      });
    }
  }

  const rows = matrix.rows.map(row => {
    // 1. Try matching pair from live GitHub Release assets
    if (releaseTag && releaseAssetsByName.size > 0) {
      const matched = {};
      let pairVersion = '';
      let validPair = true;
      for (const asset of releaseAssetsByName.values()) {
        if (!asset.name.startsWith(`${row.id}__`)) continue;
        const parsed = parseOpenwrtPackageAsset(row, asset.name);
        if (!parsed || matched[parsed.package]) { validPair = false; break; }
        if (pairVersion && pairVersion !== parsed.version) { validPair = false; break; }
        pairVersion = parsed.version;
        const buildEntry = buildList.find(b => b && b.id === row.id && Array.isArray(b.files) && b.files.some(f => f.name === asset.name));
        const buildFile = buildEntry?.files?.find(f => f.name === asset.name);
        const sha256 = asset.sha256 || validDigest(buildFile?.sha256) || sums.get(asset.name) || null;
        matched[parsed.package] = {
          package: parsed.package,
          name: asset.name,
          version: parsed.version,
          url: asset.url,
          size: asset.size || (Number.isSafeInteger(buildFile?.size) && buildFile.size > 0 ? buildFile.size : null),
          sha256,
        };
      }
      if (validPair && matched['bjut-autologin'] && matched['luci-app-bjut-autologin']) {
        const buildEntry = buildList.find(b => b && b.id === row.id && Array.isArray(b.files) && b.files.some(f => f.name === matched['bjut-autologin'].name));
        return {
          ...row,
          status: 'release',
          tag: releaseTag,
          packageRevision: pairVersion,
          sourceCommit: typeof buildEntry?.source_commit === 'string' && /^[0-9a-f]{40}$/.test(buildEntry.source_commit) ? buildEntry.source_commit : null,
          sdkContainer: `ghcr.io/openwrt/sdk:${row.sdk}`,
          files: [matched['bjut-autologin'], matched['luci-app-bjut-autologin']],
        };
      }
    }

    // 2. Fallback to verified read-only build metadata (if present for this SDK row)
    const verifiedRecords = buildList
      .map(record => validateBuildRecord(record, row, repo, releaseTag || ''))
      .filter(Boolean);
    if (verifiedRecords.length > 0) {
      const primary = verifiedRecords[0];
      const latestVerified = verifiedRecords[verifiedRecords.length - 1];
      return {
        ...row,
        status: primary.tag ? 'verified-release' : 'verified-build',
        tag: primary.tag,
        packageRevision: primary.packageRevision,
        sourceCommit: primary.sourceCommit,
        sdkContainer: primary.sdkContainer,
        files: primary.files,
        latestBuild: latestVerified !== primary ? latestVerified : null,
      };
    }

    // 3. Fallback to build-targets matrix specification without fabricating packages or hashes
    return {
      ...row,
      status: 'matrix-only',
      tag: config.publishedTag || null,
      packageRevision: config.packageVersion && config.sourceRevision ? `${config.packageVersion}-r${config.sourceRevision}` : null,
      sourceCommit: null,
      sdkContainer: `ghcr.io/openwrt/sdk:${row.sdk}`,
      files: [],
      latestBuild: null,
    };
  });

  return {
    repository: repo,
    packageVersion: config.packageVersion || '0.1.6',
    sourceRevision: config.sourceRevision || '18',
    publishedTag: releaseTag || config.publishedTag || 'openwrt-v0.1.6-r13',
    publishedAt,
    buildsJsonUrl: buildsJsonUrl || (config.publishedTag ? `https://github.com/${repo}/releases/download/${config.publishedTag}/builds.json` : null),
    sha256SumsUrl: sha256SumsUrl || (config.publishedTag ? `https://github.com/${repo}/releases/download/${config.publishedTag}/SHA256SUMS` : null),
    releasesUrl: `https://github.com/${repo}/releases`,
    workflowUrl: `https://github.com/${repo}/actions/workflows/openwrt-packages.yml`,
    releases: matrix.releases,
    platforms: matrix.platforms,
    rows,
  };
}
