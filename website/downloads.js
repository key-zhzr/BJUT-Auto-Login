export const REPOSITORY = 'key-zhzr/BJUT-Auto-Login';
export const PLATFORMS = { windows: 'Windows', macos: 'macOS', linux: 'Linux', android: 'Android' };
const filename = /^BJUT-Auto-Login_(\d+\.\d+\.\d+)_(Windows|macOS|Linux|Android)_(x64|x86_64|arm64)(_(Offline))?\.(exe|dmg|deb|AppImage|apk)$/;

export function releaseCatalog(release) {
  if (!release || release.draft || release.prerelease || !/^v?\d+\.\d+\.\d+$/.test(release.tag_name)) return null;
  const version = release.tag_name.replace(/^v/, '');
  const assets = (Array.isArray(release.assets) ? release.assets : []).flatMap(asset => {
    const match = typeof asset.name === 'string' && asset.name.match(filename);
    if (!match || match[1] !== version) return [];
    const platform = match[2].toLowerCase();
    const format = match[6];
    if (!({windows:['exe'],macos:['dmg'],linux:['deb','AppImage'],android:['apk']}[platform]?.includes(format))) return [];
    if (match[5] && platform !== 'windows') return [];
    let url;
    try { url = new URL(asset.browser_download_url); } catch { return []; }
    if (url.origin !== 'https://github.com' || url.username || url.password || url.search || url.hash
      || url.pathname !== `/${REPOSITORY}/releases/download/${release.tag_name}/${asset.name}`) return [];
    const arch = match[3] === 'x86_64' ? 'x64' : match[3];
    const variant = match[5] ? 'offline' : format;
    return [{id:`${platform}-${arch}-${variant}`, platform, arch, format, offline:!!match[5], name:asset.name,
      url:url.href, size:Number.isSafeInteger(asset.size) && asset.size > 0 ? asset.size : null}];
  });
  if (!assets.length) return null;
  return {version, tag:release.tag_name, assets};
}

export function newestCatalog(releases, fallback) {
  const candidates = (Array.isArray(releases) ? releases : [releases]).map(releaseCatalog).filter(Boolean);
  if (fallback) candidates.push(fallback);
  return candidates.sort((a,b) => {
    const aa=a.version.split('.').map(Number), bb=b.version.split('.').map(Number);
    return bb[0]-aa[0] || bb[1]-aa[1] || bb[2]-aa[2];
  })[0] || null;
}

export function assetLabel(asset) {
  const arch = asset.arch === 'arm64' ? (asset.platform === 'macos' ? 'Apple 芯片 · ARM64' : 'ARM64')
    : (asset.platform === 'macos' ? 'Intel · x64' : 'x64');
  return `${arch} · ${asset.offline ? '离线安装包' : asset.format === 'AppImage' ? 'AppImage' : asset.format.toUpperCase()}`;
}
export function formatSize(size) {
  return size ? `${(size / 1024 / 1024).toFixed(1)} MB` : '';
}

export function detectDevice({ua='', platform='', hints={}, touches=0, renderer=''} = {}) {
  const osHint = String(hints.platform || '').toLowerCase();
  // iPad desktop UA says Macintosh/MacIntel; touch support distinguishes it.
  if (/iPhone|iPad|iPod/i.test(ua) || (/Mac/i.test(platform + ua) && touches > 1)) return {platform:'ios', arch:null, certain:true};
  if (/CrOS/i.test(ua) || osHint === 'chrome os') return {platform:'chromeos', arch:null, certain:true};
  const os = osHint === 'android' || /Android/i.test(ua) ? 'android'
    : osHint === 'windows' || /Windows/i.test(ua) || /^Win/i.test(platform) ? 'windows'
    : osHint === 'macos' || /Macintosh|Mac OS X/i.test(ua) || /^Mac/i.test(platform) ? 'macos'
    : osHint === 'linux' || /Linux|X11/i.test(ua + platform) ? 'linux' : null;
  let arch = null;
  const architecture = String(hints.architecture || '').toLowerCase();
  const bits = String(hints.bitness || '');
  if (architecture === 'arm') arch = bits === '32' ? 'arm32' : bits === '64' ? 'arm64' : null;
  if (architecture === 'x86') arch = bits === '64' || hints.wow64 === true ? 'x64' : bits === '32' ? 'x86' : null;
  if (!arch) {
    if (/aarch64|arm64/i.test(ua + ' ' + platform)) arch = 'arm64';
    else if (os !== 'android' && /armv[5-8]l|armv7/i.test(ua + ' ' + platform)) arch = 'arm32';
    else if (os !== 'macos' && /x86_64|x64|amd64|Win64|WOW64/i.test(ua + ' ' + platform)) arch = 'x64';
    else if (os === 'linux' && /i[3-6]86/i.test(platform + ua)) arch = 'x86';
  }
  // MacIntel and "Intel Mac OS X" also appear on Apple Silicon. Only explicit
  // renderer families may refine the fallback; generic "Apple GPU" is unknown.
  if (!arch && os === 'macos') {
    if (/Apple\s+M\d+(?:\b|\s)/i.test(renderer)) arch = 'arm64';
    else if (/\bIntel\b|\bAMD\b|\bRadeon\b/i.test(renderer)) arch = 'x64';
  }
  return {platform:os, arch, certain:!!arch};
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
  if (!device.certain) note += device.platform === 'macos' ? '。芯片未确认，Apple 芯片可选下方 ARM64 版。' : '。架构未确认，可在下方切换。';
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
