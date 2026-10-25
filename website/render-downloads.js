import {PLATFORMS,assetLabel,formatSize,formatPackageSize} from './downloads.js';
const escape = value => String(value ?? '').replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));

export function downloadCards(catalog) {
  const icons={windows:'windows',macos:'apple',linux:'linux',android:'android'};
  return Object.entries(PLATFORMS).map(([platform,name])=> {
    const assets=catalog.assets.filter(asset=>asset.platform===platform);
    return `<article class="download-card" data-platform="${platform}"><div class="download-platform"><img class="platform-symbol" src="/icons/${icons[platform]}.svg" alt="" width="32" height="32"><h3>${name}</h3></div><div class="download-options">${assets.map(asset=>`<a class="download-option" data-asset="${asset.id}" href="${escape(asset.url)}"><span>${escape(assetLabel(asset))}</span><small>${formatSize(asset.size)} ↓</small></a>`).join('') || '<p>当前版本暂未提供</p>'}</div></article>`;
  }).join('\n');
}

function renderOpenwrtCell(row, releasesUrl) {
  if (!row) {
    return `<div class="openwrt-cell-empty"><span class="openwrt-badge muted">无 IPK</span><p>OpenWrt 24.10.8 无官方 IPQ60xx SDK，仅提供 25.12.5 APK。</p></div>`;
  }
  const header = `<div class="openwrt-sdk-head"><span class="openwrt-badge ${row.format}">${escape(row.format.toUpperCase())} · ${escape(row.version)}</span><code>${escape(row.id)}</code></div>`;
  if (row.files && row.files.length === 2) {
    const items = row.files.map(file => {
      const label = file.package === 'bjut-autologin' ? '核心包 bjut-autologin' : '界面包 luci-app-bjut-autologin';
      const sizeText = formatPackageSize(file.size);
      const titleHtml = file.url
        ? `<a class="openwrt-pkg-link" href="${escape(file.url)}"><span>${escape(label)} (${escape(file.version)})</span><small>${escape(sizeText)} ↓</small></a>`
        : `<div class="openwrt-pkg-meta"><span>${escape(label)} (${escape(file.version)})</span><small>${escape(sizeText)}</small></div>`;
      const shaHtml = file.sha256
        ? `<div class="openwrt-sha-row"><span>SHA-256: <code class="openwrt-sha256">${escape(file.sha256)}</code></span></div>`
        : `<div class="openwrt-sha-row"><small>校验值请核对 Release 的 SHA256SUMS</small></div>`;
      return `<div class="openwrt-pkg-item">${titleHtml}${shaHtml}</div>`;
    }).join('');
    const latestHtml = row.latestBuild ? `<details class="openwrt-latest-build"><summary>已验证源码修订 ${escape(row.latestBuild.packageRevision)} 构建元数据</summary><div class="openwrt-latest-body"><p>源码提交：<code>${escape(row.latestBuild.sourceCommit || '')}</code> · SDK：<code>${escape(row.latestBuild.sdkContainer)}</code></p>${row.latestBuild.files.map(file => `<p><code>${escape(file.name)}</code> (${escape(formatPackageSize(file.size))})<br>SHA-256: <code class="openwrt-sha256">${escape(file.sha256)}</code></p>`).join('')}</div></details>` : '';
    return `<div class="openwrt-sdk-box" data-openwrt-id="${escape(row.id)}" data-openwrt-format="${escape(row.format)}">${header}${items}${latestHtml}</div>`;
  }
  return `<div class="openwrt-sdk-box matrix-fallback" data-openwrt-id="${escape(row.id)}" data-openwrt-format="${escape(row.format)}">${header}<p class="openwrt-fallback-note">配套包：<code>bjut-autologin</code> (${escape(row.arch)}) + <code>luci-app-bjut-autologin</code>${row.format === 'ipk' ? ' (all)' : ''} · <a href="${escape(releasesUrl)}">前往 Release 下载并核对 SHA256SUMS ↗</a></p></div>`;
}

export function openwrtMatrixMarkup(openwrt, {formatFilter = 'all', query = ''} = {}) {
  const normalizedQuery = String(query || '').trim().toLowerCase();
  const platforms = openwrt.platforms.filter(platform => {
    if (formatFilter !== 'all') {
      const formats = Array.isArray(platform.formats) ? platform.formats : ['apk', 'ipk'];
      if (!formats.includes(formatFilter)) return false;
    }
    if (!normalizedQuery) return true;
    const haystack = `${platform.target} ${platform.arch} ${platform.devices || ''}`.toLowerCase();
    return haystack.includes(normalizedQuery);
  });

  if (!platforms.length) {
    return `<p class="openwrt-empty">未找到匹配的目标平台或包架构，请尝试清空筛选条件。</p>`;
  }

  const rowsHtml = platforms.map(platform => {
    const apkRow = openwrt.rows.find(r => r.target === platform.target && r.format === 'apk') || null;
    const ipkRow = openwrt.rows.find(r => r.target === platform.target && r.format === 'ipk') || null;
    const showApk = formatFilter === 'all' || formatFilter === 'apk';
    const showIpk = formatFilter === 'all' || formatFilter === 'ipk';
    return `<tr data-openwrt-target="${escape(platform.target)}" data-openwrt-arch="${escape(platform.arch)}">
      <th scope="row" class="openwrt-target-col">
        <strong><code>${escape(platform.target)}</code></strong>
        <span class="openwrt-devices">${escape(platform.devices || '')}</span>
      </th>
      <td class="openwrt-arch-col"><code>${escape(platform.arch)}</code></td>
      ${showApk ? `<td class="openwrt-format-col">${renderOpenwrtCell(apkRow, openwrt.releasesUrl)}</td>` : ''}
      ${showIpk ? `<td class="openwrt-format-col">${renderOpenwrtCell(ipkRow, openwrt.releasesUrl)}</td>` : ''}
    </tr>`;
  }).join('\n');

  const showApk = formatFilter === 'all' || formatFilter === 'apk';
  const showIpk = formatFilter === 'all' || formatFilter === 'ipk';

  return `<div class="openwrt-table-wrap"><table class="openwrt-matrix-table">
    <thead>
      <tr>
        <th scope="col">Target 平台 / 常见设备</th>
        <th scope="col">包架构 (DISTRIB_ARCH)</th>
        ${showApk ? '<th scope="col">OpenWrt 25.12.5 · APK (apk)</th>' : ''}
        ${showIpk ? '<th scope="col">OpenWrt 24.10.8 · IPK (opkg)</th>' : ''}
      </tr>
    </thead>
    <tbody>${rowsHtml}</tbody>
  </table></div>`;
}
