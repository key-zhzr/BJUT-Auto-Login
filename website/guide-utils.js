import { marked } from 'marked';

const escapeHtml = value => String(value ?? '').replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));

export function stripHtml(html = '') {
  return String(html)
    .replace(/<[^>]*>/g, '')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&nbsp;/g, ' ')
    .trim();
}

function safeDecode(value = '') {
  try { return decodeURIComponent(value); } catch { return value; }
}

export function slugifyHeading(rawText = '', seen = null) {
  const clean = stripHtml(rawText)
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, '')
    .trim()
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '') || 'section';
  if (!seen) return clean;
  const count = seen.get(clean) ?? 0;
  seen.set(clean, count + 1);
  return count === 0 ? clean : `${clean}-${count}`;
}

export function rewriteGuideLinks(html = '', validSlugs = []) {
  const slugSet = new Set(validSlugs);
  const normalizeHash = hash => {
    if (!hash || hash === '#') return '';
    const slug = slugifyHeading(safeDecode(hash.slice(1)));
    return slug ? `#${slug}` : '';
  };
  return String(html)
    .replace(/href="(\.\.\/|\.\/)?Home(?:\.md)?(#[^"]*)?"/g, (_, _prefix, hash = '') => `href="/${normalizeHash(hash)}"`)
    .replace(/href="(\.\.\/|\.\/)?([A-Z][A-Za-z-]+)(?:\.md)?(#[^"]*)?"/g, (full, _prefix, page, hash = '') => {
      if (!slugSet.has(page)) return full;
      return `href="/guide/${page}/${normalizeHash(hash)}"`;
    })
    .replace(/href="#([^"]+)"/g, (_, rawAnchor) => `href="#${slugifyHeading(safeDecode(rawAnchor))}"`);
}

export function renderGuidePage(markdown, { slug, title, validSlugs = [] } = {}) {
  const rawHtml = marked.parse(markdown);
  const seenIds = new Map();
  const toc = [];
  const headings = [];

  const withHeadings = rawHtml.replace(/<h([1-4])>([\s\S]*?)<\/h\1>/g, (_, levelStr, inner) => {
    const level = Number(levelStr);
    const id = slugifyHeading(inner, seenIds);
    const text = stripHtml(inner);
    headings.push({ level, id, text });
    if (level === 2 || level === 3) toc.push({ level, id, text });
    return `<h${level} id="${escapeHtml(id)}">${inner}<a class="heading-anchor" href="#${escapeHtml(id)}" aria-label="${escapeHtml(text)} 章节锚点">#</a></h${level}>`;
  });

  const withCodeCopy = withHeadings.replace(/<pre><code([^>]*)>([\s\S]*?)<\/code><\/pre>/g, (_, attrs, code) => {
    return `<div class="code-block"><div class="code-block-bar"><button type="button" class="copy-code" data-copy-code aria-label="复制代码">复制代码</button></div><pre><code${attrs}>${code}</code></pre></div>`;
  });

  const content = rewriteGuideLinks(withCodeCopy, validSlugs);

  const tocHtml = toc.length > 0
    ? `<aside class="guide-toc" aria-label="本页目录"><p class="guide-toc-title">本页目录</p><ol>${toc.map(item => `<li class="toc-level-${item.level}"><a href="#${escapeHtml(item.id)}">${escapeHtml(item.text)}</a></li>`).join('')}</ol></aside>`
    : '';

  const sections = [];
  const parts = content.split(/(?=<h[1-4]\s+id="[^"]+">)/g);
  for (const part of parts) {
    const match = part.match(/^<h([1-4])\s+id="([^"]+)">([\s\S]*?)<\/h\1>([\s\S]*)$/);
    if (match) {
      const id = match[2];
      const heading = stripHtml(match[3].replace(/<a class="heading-anchor"[\s\S]*?<\/a>/, ''));
      const bodyText = stripHtml(match[4].replace(/<button[^>]*data-copy-code[^>]*>[\s\S]*?<\/button>/g, '')).replace(/\s+/g, ' ').trim();
      sections.push({
        slug,
        pageTitle: title,
        id,
        heading,
        text: bodyText.slice(0, 320),
      });
    }
  }
  if (!sections.length) {
    sections.push({
      slug,
      pageTitle: title,
      id: headings[0]?.id || '',
      heading: title,
      text: stripHtml(content).replace(/\s+/g, ' ').trim().slice(0, 320),
    });
  }

  return { content, toc, tocHtml, sections };
}

export function extractPageIds(html = '') {
  const ids = new Set();
  for (const match of String(html).matchAll(/\bid="([^"]+)"/g)) {
    ids.add(match[1]);
  }
  return ids;
}

export function checkSiteLinks(pagesMap, staticPaths = new Set()) {
  const errors = [];
  const idMap = new Map();
  for (const [route, html] of pagesMap.entries()) {
    idMap.set(route, extractPageIds(html));
  }

  for (const [route, html] of pagesMap.entries()) {
    const pageIds = idMap.get(route);
    for (const match of String(html).matchAll(/\b(href|src)="([^"]+)"/g)) {
      const attr = match[1];
      const target = match[2];
      if (!target || target.startsWith('data:')) continue;
      if (target.startsWith('http://')) {
        errors.push(`${route}: insecure ${attr}="${target}"`);
        continue;
      }
      if (target.startsWith('https://')) {
        try { new URL(target); } catch { errors.push(`${route}: invalid external URL ${attr}="${target}"`); }
        continue;
      }
      if (target.startsWith('#')) {
        const anchor = safeDecode(target.slice(1));
        if (!anchor || (!pageIds.has(anchor) && !pageIds.has(target.slice(1)))) {
          errors.push(`${route}: missing same-page anchor ${target}`);
        }
        continue;
      }
      if (!target.startsWith('/')) {
        errors.push(`${route}: unrewritten relative ${attr}="${target}"`);
        continue;
      }
      const [pathWithQuery, hash = ''] = target.split('#');
      const [pathname] = pathWithQuery.split('?');
      const existsAsPage = pagesMap.has(pathname);
      const existsAsAsset = staticPaths.has(pathname);
      if (!existsAsPage && !existsAsAsset) {
        errors.push(`${route}: broken internal path ${attr}="${target}"`);
        continue;
      }
      if (hash && existsAsPage) {
        const targetIds = idMap.get(pathname);
        const decodedHash = safeDecode(hash);
        if (!targetIds.has(decodedHash) && !targetIds.has(hash)) {
          errors.push(`${route}: missing cross-page anchor ${target} on ${pathname}`);
        }
      }
    }
  }

  return { ok: errors.length === 0, errors };
}

export function searchGuideIndex(index = [], rawQuery = '') {
  const query = String(rawQuery || '').trim().toLowerCase();
  if (!query) return [];
  const terms = query.split(/\s+/).filter(Boolean);
  const scored = [];
  for (const item of Array.isArray(index) ? index : []) {
    const titleText = String(item.pageTitle || '').toLowerCase();
    const headingText = String(item.heading || '').toLowerCase();
    const bodyText = String(item.text || '').toLowerCase();
    const combined = `${titleText} ${headingText} ${bodyText}`;
    if (!terms.every(term => combined.includes(term))) continue;
    let score = 0;
    for (const term of terms) {
      if (headingText.includes(term)) score += 5;
      if (titleText.includes(term)) score += 3;
      if (bodyText.includes(term)) score += 1;
    }
    scored.push({ ...item, score });
  }
  return scored.sort((a, b) => b.score - a.score).slice(0, 12);
}
