import release, { openwrtTargets } from './release-data.js';
import { REPOSITORY, OPENWRT_REPOSITORY, PLATFORMS, releaseCatalog, releaseChannels, assetLabel, formatSize, readDevice, selectDeviceArch, recommendDownload, openwrtCatalog } from './downloads.js';
import { openwrtMatrixMarkup } from './render-downloads.js';

let catalog = releaseCatalog(release);
let channels = {release:catalog, prerelease:null};
let channel = 'release';
let device = {platform:null, arch:null, certain:false};
let manualArch = '';
let openwrt = openwrtCatalog(null, openwrtTargets);
let openwrtFormat = 'all';
let openwrtQuery = '';

function rendererHint() {
  const canvas = document.createElement('canvas');
  const gl = canvas.getContext('webgl');
  if (!gl) return '';
  try {
    const extension = gl.getExtension('WEBGL_debug_renderer_info');
    return extension ? String(gl.getParameter(extension.UNMASKED_RENDERER_WEBGL)) : '';
  } finally { gl.getExtension('WEBGL_lose_context')?.loseContext(); }
}

function renderRecommendation() {
  const selected = selectDeviceArch(device, manualArch);
  const {asset, note} = catalog ? recommendDownload(selected, catalog) : {asset:null, note:'暂无可下载的预发布版，请选择正式版。'};
  document.querySelectorAll('[data-smart-download]').forEach(link => {
    link.href = asset?.url || '#download';
    link.querySelector('[data-download-label]').textContent = asset
      ? `下载 ${PLATFORMS[asset.platform]} ${asset.platform === 'android' ? (asset.arch === 'arm64' ? 'ARM64' : 'x86_64') : '版'}`
      : '选择安装包';
    link.dataset.asset = asset?.id || '';
  });
  document.querySelectorAll('[data-download-hint]').forEach(node => { node.textContent = note; });
  document.querySelectorAll('.download-card').forEach(card => card.classList.toggle('recommended', card.dataset.platform === selected.platform));
  document.querySelectorAll('.download-option').forEach(option => option.classList.toggle('recommended', !!asset && option.dataset.asset === asset.id));
  const archLabel = document.getElementById('android-architecture');
  if (archLabel) archLabel.hidden = device.platform !== 'android' && !manualArch;
}

function renderCatalog() {
  const version = document.getElementById('release-version');
  if (version) {
    version.textContent = catalog
      ? `${channel === 'prerelease' ? '预发布版' : '正式版'} ${catalog.tag}${catalog.publishedAt ? ' · ' + new Date(catalog.publishedAt).toLocaleDateString('zh-CN') : ''}`
      : '暂无预发布版';
  }
  for (const button of document.querySelectorAll('[data-release-channel]')) {
    button.setAttribute('aria-pressed', String(button.dataset.releaseChannel === channel));
  }
  for (const card of document.querySelectorAll('.download-card')) {
    const links = card.querySelector('.download-options');
    const assets = catalog?.assets.filter(asset => asset.platform === card.dataset.platform) || [];
    links.replaceChildren(...assets.map(asset => {
      const link = document.createElement('a');
      link.className = 'download-option';
      link.href = asset.url;
      link.dataset.asset = asset.id;
      const title = document.createElement('span');
      title.textContent = assetLabel(asset);
      const size = document.createElement('small');
      size.textContent = `${formatSize(asset.size)} ↓`.trim();
      link.append(title, size);
      return link;
    }));
    if (!assets.length) {
      const empty = document.createElement('p');
      empty.textContent = catalog ? '当前版本暂未提供' : '当前暂无预发布版安装包，请切换到正式版下载。';
      links.append(empty);
    }
  }
  renderRecommendation();
}

function renderOpenwrt() {
  const versionNode = document.getElementById('openwrt-release-version');
  if (versionNode) {
    versionNode.textContent = `最新构建标签 ${openwrt.publishedTag} · 源码修订 ${openwrt.packageVersion}-r${openwrt.sourceRevision}`;
  }
  const buildsLink = document.getElementById('openwrt-builds-link');
  if (buildsLink && openwrt.buildsJsonUrl) buildsLink.href = openwrt.buildsJsonUrl;
  const sumsLink = document.getElementById('openwrt-sums-link');
  if (sumsLink && openwrt.sha256SumsUrl) sumsLink.href = openwrt.sha256SumsUrl;
  for (const button of document.querySelectorAll('[data-openwrt-format]')) {
    button.setAttribute('aria-pressed', String(button.dataset.openwrtFormat === openwrtFormat));
  }
  const matrixNode = document.getElementById('openwrt-matrix');
  if (matrixNode) {
    matrixNode.innerHTML = openwrtMatrixMarkup(openwrt, {formatFilter: openwrtFormat, query: openwrtQuery});
  }
}

document.querySelectorAll('[data-release-channel]').forEach(button => button.addEventListener('click', () => {
  channel = button.dataset.releaseChannel;
  catalog = channels[channel];
  renderCatalog();
}));

document.getElementById('android-arch-choice')?.addEventListener('change', event => {
  manualArch = event.target.value;
  renderRecommendation();
});

document.querySelectorAll('[data-openwrt-format]').forEach(button => button.addEventListener('click', () => {
  openwrtFormat = button.dataset.openwrtFormat || 'all';
  renderOpenwrt();
}));

document.getElementById('openwrt-platform-filter')?.addEventListener('input', event => {
  openwrtQuery = event.target.value || '';
  renderOpenwrt();
});

document.addEventListener('click', event => {
  const button = event.target.closest?.('[data-copy-code]');
  if (!button) return;
  const code = button.closest('.code-block')?.querySelector('pre code') || button.closest('.code-block')?.querySelector('pre');
  if (!code) return;
  void navigator.clipboard?.writeText(code.textContent || '').then(() => {
    button.textContent = '已复制 ✓';
    button.dataset.copied = 'true';
    setTimeout(() => {
      button.textContent = '复制代码';
      delete button.dataset.copied;
    }, 1600);
  }).catch(() => {});
});

renderRecommendation();
void readDevice(navigator, rendererHint).then(value => { device = value; renderRecommendation(); });

// Public metadata only. No device hints are sent to GitHub or retained locally.
const controller = new AbortController();
const timeout = setTimeout(() => controller.abort(), 4000);
void (async () => {
  try {
    const read = async (repo, path) => {
      const response = await fetch(`https://api.github.com/repos/${repo}/${path}`, {
        signal: controller.signal,
        credentials: 'omit',
        headers: {Accept: 'application/vnd.github+json'},
      });
      if (!response.ok) throw new Error('版本信息暂不可用');
      return response.json();
    };
    const [all, latest, routerReleases] = await Promise.allSettled([
      read(REPOSITORY, 'releases?per_page=100'),
      read(REPOSITORY, 'releases/latest'),
      read(OPENWRT_REPOSITORY, 'releases?per_page=10'),
    ]);
    channels = releaseChannels(
      [...(all.status === 'fulfilled' ? all.value : []), ...(latest.status === 'fulfilled' ? [latest.value] : [])],
      channels.release,
      latest.status === 'fulfilled' ? latest.value.tag_name : '',
    );
    catalog = channels[channel];
    renderCatalog();
    if (routerReleases.status === 'fulfilled' && Array.isArray(routerReleases.value)) {
      const candidate = routerReleases.value.find(item => item && !item.draft && /^openwrt-v\d+\.\d+\.\d+(?:-r\d+)?$/.test(item.tag_name));
      if (candidate) {
        openwrt = openwrtCatalog(candidate, openwrtTargets);
        renderOpenwrt();
      }
    }
  } catch { /* Build-time, verified installer and OpenWrt matrix metadata remain usable. */ }
  finally { clearTimeout(timeout); }
})();

// Both documents render the client's own markup and CSS at real device sizes.
for (const viewport of document.querySelectorAll('.preview-viewport')) {
  const frame = viewport.querySelector('iframe');
  const resize = () => { frame.style.transform = `scale(${viewport.clientWidth / Number(frame.width)})`; };
  new ResizeObserver(resize).observe(viewport);
  resize();
}
