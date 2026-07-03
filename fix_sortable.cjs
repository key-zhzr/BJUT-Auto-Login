const fs = require('fs');
let code = fs.readFileSync('src/main.ts', 'utf8');

// Replace the entire Sortable.create block to be clean
const sortableRegex = /Sortable\.create\(accountsList, \{[\s\S]*?\}\);/m;
const cleanSortable = \`Sortable.create(accountsList, {
    handle: '.drag-handle',
    animation: 300,
    easing: "cubic-bezier(0.25, 1, 0.5, 1)",
    ghostClass: 'dragging',
    forceFallback: true,
    fallbackClass: 'dragging-fallback',
    fallbackOnBody: true,
    onEnd: (evt) => {
      const { oldIndex, newIndex } = evt;
      if (oldIndex !== undefined && newIndex !== undefined && oldIndex !== newIndex) {
        const accounts = getAccounts();
        const item = accounts.splice(oldIndex, 1)[0];
        accounts.splice(newIndex, 0, item);
        accounts.forEach((a, i) => a.isDefault = (i === 0));
        saveAccounts(accounts);
        renderAccounts(); // just re-render to avoid DOM desync
        log('账号管理', '账号顺序已更新，最高优先级将作为默认账号');
      }
    }
  });\`;

code = code.replace(sortableRegex, cleanSortable);
fs.writeFileSync('src/main.ts', code, 'utf8');
