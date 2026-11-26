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

async function copyText(text) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  area.style.position = 'fixed';
  area.style.opacity = '0';
  document.body.append(area);
  area.select();
  document.execCommand('copy');
  area.remove();
}

if (typeof document !== 'undefined') {
  try {
    const savedTheme = localStorage.getItem('bjut_site_theme');
    if (savedTheme === 'light' || savedTheme === 'dark') {
      document.documentElement.dataset.theme = savedTheme;
    }
  } catch {}

  document.querySelectorAll('[data-site-theme]').forEach(button => {
    const current = document.documentElement.dataset.theme || 'system';
    button.setAttribute('aria-pressed', String(button.dataset.siteTheme === current));
    button.addEventListener('click', () => {
      const next = button.dataset.siteTheme || 'system';
      try { localStorage.setItem('bjut_site_theme', next); } catch {}
      if (next === 'system') delete document.documentElement.dataset.theme;
      else document.documentElement.dataset.theme = next;
      document.querySelectorAll('[data-site-theme]').forEach(other => {
        other.setAttribute('aria-pressed', String(other.dataset.siteTheme === next));
      });
    });
  });

  document.addEventListener('click', event => {
    const button = event.target.closest?.('[data-copy-code]');
    if (!button) return;
    const block = button.closest('.code-block');
    const code = block?.querySelector('pre code') || block?.querySelector('pre');
    if (!code) return;
    void copyText(code.textContent || '').then(() => {
      button.textContent = '已复制 ✓';
      button.dataset.copied = 'true';
      setTimeout(() => {
        button.textContent = '复制代码';
        delete button.dataset.copied;
      }, 1600);
    }).catch(() => {
      button.textContent = '请手动复制';
    });
  });

  const searchInput = document.getElementById('guide-search-input');
  const searchResults = document.getElementById('guide-search-results');

  if (searchInput && searchResults) {
    let indexPromise = null;
    const loadIndex = () => {
      if (!indexPromise) {
        indexPromise = fetch('/guide-index.json', { credentials: 'omit' })
          .then(res => (res.ok ? res.json() : []))
          .catch(() => []);
      }
      return indexPromise;
    };

    const renderResults = async () => {
      const query = searchInput.value.trim();
      if (!query) {
        searchResults.hidden = true;
        searchResults.replaceChildren();
        return;
      }
      const guideIndex = await loadIndex();
      if (searchInput.value.trim() !== query) return;
      const matches = searchGuideIndex(guideIndex, query);
      searchResults.hidden = false;
      if (!matches.length) {
        const empty = document.createElement('p');
        empty.className = 'guide-search-empty';
        empty.textContent = `未找到与“${query}”相关的章节。`;
        searchResults.replaceChildren(empty);
        return;
      }
      const items = matches.map(item => {
        const link = document.createElement('a');
        link.className = 'guide-search-item';
        link.href = `/guide/${item.slug}/${item.id ? `#${item.id}` : ''}`;
        const title = document.createElement('strong');
        title.textContent = item.heading === item.pageTitle ? item.pageTitle : `${item.pageTitle} · ${item.heading}`;
        const snippet = document.createElement('span');
        snippet.textContent = item.text ? item.text.slice(0, 96) + (item.text.length > 96 ? '…' : '') : '';
        link.append(title, snippet);
        return link;
      });
      searchResults.replaceChildren(...items);
    };

    searchInput.addEventListener('focus', () => { void loadIndex(); }, { once: true });
    searchInput.addEventListener('input', () => { void renderResults(); });
    searchInput.addEventListener('keydown', event => {
      if (event.key === 'Escape') {
        searchInput.value = '';
        void renderResults();
        searchInput.blur();
      }
    });

    document.addEventListener('keydown', event => {
      if (event.key !== '/' || event.ctrlKey || event.metaKey || event.altKey) return;
      const activeTag = document.activeElement?.tagName;
      if (activeTag === 'INPUT' || activeTag === 'TEXTAREA' || activeTag === 'SELECT') return;
      event.preventDefault();
      searchInput.focus();
    });
  }

  const tocLinks = Array.from(document.querySelectorAll('.guide-toc a[href^="#"]'));
  if (tocLinks.length && typeof IntersectionObserver !== 'undefined') {
    const headings = tocLinks
      .map(link => document.getElementById(decodeURIComponent(link.getAttribute('href').slice(1))))
      .filter(Boolean);
    const linkById = new Map(tocLinks.map(link => [decodeURIComponent(link.getAttribute('href').slice(1)), link]));
    const setActive = id => {
      tocLinks.forEach(link => link.classList.remove('is-active'));
      linkById.get(id)?.classList.add('is-active');
    };
    const observer = new IntersectionObserver(entries => {
      const visible = entries.filter(e => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
      if (visible[0]?.target.id) setActive(visible[0].target.id);
    }, { rootMargin: '-84px 0px -65% 0px', threshold: 0 });
    headings.forEach(h => observer.observe(h));
  }
}
