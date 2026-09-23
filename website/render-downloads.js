import {PLATFORMS,assetLabel,formatSize} from './downloads.js';
const escape = value => String(value).replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
export function downloadCards(catalog) {
  const icons={windows:'windows',macos:'apple',linux:'linux',android:'android'};
  return Object.entries(PLATFORMS).map(([platform,name])=> {
    const assets=catalog.assets.filter(asset=>asset.platform===platform);
    return `<article class="download-card" data-platform="${platform}"><div class="download-platform"><img class="platform-symbol" src="/icons/${icons[platform]}.svg" alt="" width="32" height="32"><h3>${name}</h3></div><div class="download-options">${assets.map(asset=>`<a class="download-option" data-asset="${asset.id}" href="${escape(asset.url)}"><span>${escape(assetLabel(asset))}</span><small>${formatSize(asset.size)} ↓</small></a>`).join('') || '<p>当前版本暂未提供</p>'}</div></article>`;
  }).join('\n');
}
