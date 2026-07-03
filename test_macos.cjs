const fs = require('fs');

// 1. Update styles.css
let styles = fs.readFileSync('src/styles.css', 'utf-8');
if (!styles.includes('-webkit-app-region: drag;')) {
    styles = styles.replace('.titlebar {', '.titlebar {\n  -webkit-app-region: drag;');
}
fs.writeFileSync('src/styles.css', styles);

// 2. Update tauri.conf.json to set shadow: false
let tauriConf = fs.readFileSync('src-tauri/tauri.conf.json', 'utf-8');
if (!tauriConf.includes('"shadow"')) {
    tauriConf = tauriConf.replace('"transparent": true', '"transparent": true,\n        "shadow": false');
}
fs.writeFileSync('src-tauri/tauri.conf.json', tauriConf);
