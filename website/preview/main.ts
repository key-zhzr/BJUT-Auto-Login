// Render the checked-in application markup and styles with example data only.
// The production app entrypoint, Tauri bridge and account storage are not loaded.
import appHtml from '../../index.html?raw';
import '../../src/styles.css';
import '../../src/themes.css';
import '../../src/theme-apple.css';
import '../../src/theme-winui.css';
import '../../src/page-layout.css';
import { applyAppearance } from '../../src/appearance';
import { UI_TEXT } from '../../src/ui-text';
import { Activity, BarChart2, CheckCircle, ChevronDown, Clock, createIcons, FileText, Home, LayoutDashboard, LogIn, Minus, Power, ReceiptText, RefreshCw, Settings, ShieldCheck, Square, User, Users, WalletCards, X } from 'lucide';

const mobile = new URLSearchParams(location.search).get('layout') === 'mobile';
const source = new DOMParser().parseFromString(appHtml, 'text/html');
const app = source.getElementById('app')!;
const titlebar = source.querySelector<HTMLElement>('.titlebar')!;
app.querySelectorAll('.page:not(#dashboard)').forEach(page => page.remove());
// Unused account/login controls stay exactly as in the live dashboard: hidden.
app.querySelectorAll('script').forEach(script => script.remove());
document.body.replaceChildren(...(mobile ? [app] : [titlebar, app]));
document.body.classList.add(mobile ? 'is-android' : 'is-desktop');
applyAppearance('basic', 'blue', 'light');
const element = (id: string) => document.getElementById(id)!;
element('network-status').textContent = UI_TEXT.networkStatus.onlineTitle;
element('network-detail').textContent = UI_TEXT.networkStatus.onlineDetail;
element('network-icon').className = 'status-icon success';
element('network-icon').innerHTML = '<i data-lucide="check-circle"></i>';
element('btn-login').hidden = true;
element('btn-switch-account').hidden = false;
element('btn-logout-current').hidden = false;
element('info-account').textContent = '示例账号';
element('info-balance').textContent = '36.00 元';
element('info-flow').textContent = '82.4 GB';
element('update-timestamp').textContent = '12:30:00';
element('countdown-text').textContent = '15';
for (const [id,time] of [['ipv4-health','28'],['ipv6-health','36']]) {
  const node=element(id); node.className='family-status reachable';
  node.querySelector('strong')!.textContent=`${time} ms`;
  node.querySelector('.family-marker')!.textContent='✓';
}
createIcons({ icons: { Activity, BarChart2, CheckCircle, ChevronDown, Clock, FileText, Home, LayoutDashboard, LogIn, Minus, Power, ReceiptText, RefreshCw, Settings, ShieldCheck, Square, User, Users, WalletCards, X } });
// This is a non-interactive presentation, not an app sign-in surface.
app.inert = true; titlebar.inert = true;
document.documentElement.dataset.previewReady='true';
