// Keep a working Releases link when the public API is unavailable. Never guess
// an architecture or auto-start a download on behalf of the visitor.
const controller = new AbortController();
const timeout = setTimeout(() => controller.abort(), 4000);
try {
  const response = await fetch('https://api.github.com/repos/key-zhzr/BJUT-Auto-Login/releases/latest', { signal: controller.signal, credentials: 'omit', headers: { Accept: 'application/vnd.github+json' } });
  if (response.ok) {
    const release = await response.json();
    const label = document.getElementById('release-version');
    if (label && typeof release.tag_name === 'string') label.textContent = `当前版本 ${release.tag_name.slice(0, 32)}`;
  }
} catch { /* Static download links remain available. */ }
finally { clearTimeout(timeout); }
