import type { DualStackReport } from './models';
import { animateResize } from './layout-motion';
import { isPermissionGranted, sendNotification } from '@tauri-apps/plugin-notification';
import { qualitySummary, type QualitySample } from './network-quality-model';
const key = 'bjut_network_quality_v1';
let rows: QualitySample[] = [];
try {
    const data = JSON.parse(localStorage.getItem(key) || '[]');
    if (Array.isArray(data))
        rows = data.filter(v => v && Number.isFinite(v.at) && typeof v.scope === 'string' && ['IPv4', 'IPv6'].includes(v.family) && typeof v.ok === 'boolean' && (v.ms === null || Number.isFinite(v.ms))).slice(-2000);
}
catch { }
const alarms = new Map<string, {
    notified: boolean;
    last: number;
}>();
try {
    for (const [id, value] of JSON.parse(localStorage.getItem('bjut_quality_alarms') || '[]'))
        if (typeof id === 'string' && typeof value.notified === 'boolean' && Number.isFinite(value.last))
            alarms.set(id, value);
}
catch { }
async function alertQuality(scope: string, family: string, now: number, generation: number) {
    if (localStorage.getItem('bjut_quality_alerts') !== 'true')
        return;
    const group = rows.filter(x => x.scope === scope && x.family === family && x.generation === generation && now - x.at >= 0 && now - x.at < 600000).slice(-5);
    const id = `${scope}|${family}`, state = alarms.get(id) || { notified: false, last: 0 };
    const failed = group.length === 5 && group.every(x => !x.ok || (x.ms !== null && x.ms > 1500)) && group[4].at - group[0].at >= 120000;
    const recovered = state.notified && group.slice(-2).length === 2 && group.slice(-2).every(x => x.ok && (x.ms === null || x.ms <= 1500));
    if ((failed && !state.notified && now - state.last >= 3600000) || recovered) {
        if (!await isPermissionGranted()) return;
        state.notified = failed;
        state.last = now;
        alarms.set(id, state);
        try {
            localStorage.setItem('bjut_quality_alarms', JSON.stringify([...alarms].slice(-40)));
        }
        catch { }
        if (await isPermissionGranted())
            sendNotification({ title: recovered ? '网络已恢复' : '网络检查持续异常或响应较慢', body: `${scope} ${family}：${recovered ? '最近两次检查已通过。' : '请打开网络诊断查看认证和路由。'}` });
    }
}
export function recordRouterQuality(source: string, at: number, ipv4?: string, ipv6?: string) {
    if (!Number.isFinite(at) || at <= 0 || Date.now() - at > 7 * 86400000 || at - Date.now() > 600000) return;
    const scope = `路由器 ${source}`;
    let changed = false;
    for (const [family, status] of [['IPv4', ipv4], ['IPv6', ipv6]]) {
        if (!['reachable', 'unreachable'].includes(status || '') || !Number.isFinite(at) || at <= 0 || rows.some(x => x.scope === scope && x.family === family && Math.abs(x.at - at) < 30000))
            continue;
        rows.push({ at, scope, family: family!, ok: status === 'reachable', ms: null, generation: 0 });
        changed = true;
        void alertQuality(scope, family!, at, 0).catch(() => { });
    }
    if (changed) {
        rows = rows.filter(x => Date.now() - x.at <= 7 * 86400000).slice(-2000);
        try {
            localStorage.setItem(key, JSON.stringify(rows));
        }
        catch { }
        renderQuality();
    }
}
export function recordQuality(report: DualStackReport) {
    const at = Date.parse(report.checkedAt);
    if (!Number.isFinite(at))
        return;
    const scope = `${report.scope === 'system' ? '当前网络' : '认证网卡'} ${report.interfaceName || ''}`.trim();
    let changed = false;
    for (const [family, value] of [['IPv4', report.ipv4], ['IPv6', report.ipv6]] as const) {
        if (['checking', 'unknown', 'not_configured', 'unavailable'].includes(value.status))
            continue;
        if (rows.some(x => x.scope === scope && x.family === family && Math.abs(x.at - at) < 30000))
            continue;
        const generation = report.generation || 0;
        rows.push({ at, scope, family, ok: value.status === 'reachable', ms: Number(value.durationMs) || 0, generation });
        changed = true;
        void alertQuality(scope, family, at, generation).catch(() => { });
    }
    if (changed) {
        rows = rows.filter(x => at - x.at <= 7 * 86400000).slice(-2000);
        try {
            localStorage.setItem(key, JSON.stringify(rows));
        }
        catch { }
        renderQuality();
    }
}
function renderQuality() {
    const node = document.getElementById('network-quality-content');
    if (!node)
        return;
    const recent = rows.filter(x => Date.now() - x.at < 86400000), groups = [...new Set(recent.map(x => `${x.scope}|${x.family}`))];
    animateResize(node, () => {
        node.replaceChildren();
        if (!groups.length) {
            node.textContent = '等待网络检测记录。仅记录已完成的检查，未通过比例不等于丢包率。';
            return;
        }
        for (const id of groups) {
            const samples = recent.filter(x => `${x.scope}|${x.family}` === id), stats = qualitySummary(samples), p = document.createElement('p');
            p.textContent = `${id.replace('|', ' · ')}：${stats.count} 次 · 未通过 ${Math.round(stats.failure * 100)}% · 中位 ${stats.p50 ?? '--'} ms · P95 ${stats.p95 ?? '--'} ms`;
            node.append(p);
            const values = samples.filter(x => x.ms !== null).slice(-60), max = Math.max(1, ...values.filter(x => x.ok).map(x => x.ms!));
            const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
            svg.setAttribute('viewBox', '0 0 400 64');
            svg.setAttribute('role', 'img');
            svg.setAttribute('aria-label', '最近60次检查响应时间，空缺表示未通过');
            svg.style.cssText = 'width:100%;height:64px;color:var(--accent-color)';
            let previous: QualitySample | undefined;
            values.forEach((sample, index) => { if (sample.ok && previous?.ok) {
                const line = document.createElementNS(svg.namespaceURI, 'line');
                line.setAttribute('x1', String((index - 1) * 400 / Math.max(1, values.length - 1)));
                line.setAttribute('y1', String(60 - previous.ms! / max * 56));
                line.setAttribute('x2', String(index * 400 / Math.max(1, values.length - 1)));
                line.setAttribute('y2', String(60 - sample.ms! / max * 56));
                line.setAttribute('stroke', 'currentColor');
                line.setAttribute('stroke-width', '2');
                svg.append(line);
            } previous = sample; });
            if (values.length)
                node.append(svg);
        }
    });
}
export function setupNetworkQuality() { window.addEventListener('network-quality-sample', event => recordQuality((event as CustomEvent<DualStackReport>).detail)); renderQuality(); const toggle = document.getElementById('quality-alerts') as HTMLInputElement | null; if (toggle) {
    toggle.checked = localStorage.getItem('bjut_quality_alerts') === 'true';
    toggle.onchange = () => localStorage.setItem('bjut_quality_alerts', String(toggle.checked));
} }
