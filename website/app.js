import release from './release-data.js';
import { REPOSITORY, PLATFORMS, releaseCatalog, newestCatalog, assetLabel, formatSize, readDevice, recommendDownload } from './downloads.js';
let catalog = releaseCatalog(release);
let device = {platform:null,arch:null,certain:false};

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
  const {asset,note} = recommendDownload(device,catalog);
  document.querySelectorAll('[data-smart-download]').forEach(link => {
    link.href = asset?.url || '#download';
    link.querySelector('[data-download-label]').textContent = asset ? `下载 ${PLATFORMS[asset.platform]} 版` : '选择安装包';
    link.dataset.asset = asset?.id || '';
  });
  document.querySelectorAll('[data-download-hint]').forEach(node => { node.textContent = note; });
  document.querySelectorAll('.download-card').forEach(card => card.classList.toggle('recommended', card.dataset.platform === device.platform));
}
function renderCatalog() {
  if (!catalog) return;
  const version = document.getElementById('release-version');
  if (version) version.textContent = `版本 ${catalog.tag}`;
  for (const card of document.querySelectorAll('.download-card')) {
    const links = card.querySelector('.download-options');
    const assets = catalog.assets.filter(asset => asset.platform === card.dataset.platform);
    links.replaceChildren(...assets.map(asset => {
      const link = document.createElement('a'); link.className='download-option'; link.href=asset.url; link.dataset.asset=asset.id;
      const title=document.createElement('span'); title.textContent=assetLabel(asset);
      const size=document.createElement('small'); size.textContent=`${formatSize(asset.size)} ↓`.trim();
      link.append(title,size); return link;
    }));
    if (!assets.length) { const empty=document.createElement('p'); empty.textContent='当前版本暂未提供'; links.append(empty); }
  }
  renderRecommendation();
}
renderRecommendation();
void readDevice(navigator, rendererHint).then(value => { device=value; renderRecommendation(); });

// Public metadata only. No device hints are sent to GitHub or retained locally.
const controller = new AbortController();
const timeout = setTimeout(() => controller.abort(), 4000);
void (async () => { try {
  const response = await fetch(`https://api.github.com/repos/${REPOSITORY}/releases?per_page=20`, {signal:controller.signal,credentials:'omit',headers:{Accept:'application/vnd.github+json'}});
  if (response.ok) { catalog=newestCatalog(await response.json(),catalog); renderCatalog(); }
} catch { /* Build-time, verified installer links remain usable. */ }
finally { clearTimeout(timeout); } })();

// Both documents render the client's own markup and CSS at real device sizes.
for (const viewport of document.querySelectorAll('.preview-viewport')) {
  const frame=viewport.querySelector('iframe');
  const resize=()=> { frame.style.transform=`scale(${viewport.clientWidth / Number(frame.width)})`; };
  new ResizeObserver(resize).observe(viewport); resize();
}
