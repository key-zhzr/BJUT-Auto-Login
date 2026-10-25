import { invoke } from '@tauri-apps/api/core';
import { animateResize } from './layout-motion';
import { flowLabel, forecastUsage, usageCsv, usageDelta, usageIdentity, type AccountUsage, type UsageSample, type DailyUsage } from './device-usage-model';
import { presentationDialog } from './presentation-dialog';
export class DeviceUsageView {
    private samples = new Map<string, UsageSample>();
    private deltas = new Map<string, number | null>();
    private stamp = '';
    private days = 30;
    private monthly = false;
    private accountFilter = '';
    private accounts: AccountUsage[] = [];
    private history: DailyUsage[] = [];
    constructor(private root: HTMLElement, private source: string) { }
    render(accounts: AccountUsage[], message = '', history: DailyUsage[] = this.history) {
        this.accounts = accounts;
        this.history = history;
        const stamp = JSON.stringify([accounts, message, history, this.days, this.monthly, this.accountFilter, accounts.map(a => Date.now() / 1000 - a.updatedAt > 180)]);
        if (stamp === this.stamp)
            return;
        this.stamp = stamp;
        const now = Date.now() / 1000, seen = new Set<string>();
        const fragment = document.createDocumentFragment();
        const title = document.createElement('h3');
        title.textContent = this.source === 'router' ? '路由器设备流量' : '设备流量';
        fragment.append(title);
        const note = document.createElement('p');
        note.className = 'device-usage-note';
        note.textContent = '计费系统数据 · 每分钟更新 · 已用流量为本次在线会话累计';
        fragment.append(note);
        if (!accounts.length) {
            const empty = document.createElement('p');
            empty.textContent = message || '等待查询，添加计费账号后自动更新。';
            fragment.append(empty);
        }
        for (const account of accounts.filter(a => !this.accountFilter || a.account === this.accountFilter)) {
            const section = document.createElement('section');
            section.className = 'device-usage-account';
            const heading = document.createElement('h4');
            heading.textContent = account.account;
            section.append(heading);
            const status = document.createElement('p');
            status.className = 'device-usage-note';
            const stale = !account.updatedAt || now - account.updatedAt > 180 || !!account.error;
            status.textContent = account.updatedAt ? `更新于 ${new Date(account.updatedAt * 1000).toLocaleTimeString('zh-CN', { hour12: false })}${stale ? ' · 上次成功数据' : ''}` : '尚未取得数据';
            section.append(status);
            if (account.error) {
                const error = document.createElement('p');
                error.className = 'device-usage-note';
                error.textContent = `暂未更新：${account.error}`;
                section.append(error);
            }
            const prediction = forecastUsage(history.filter(row => row.account === account.account), account.remainingFlow || '');
            const estimate = document.createElement('p');
            estimate.className = 'device-usage-note';
            estimate.textContent = prediction ? `按近 7 天采样估算，月底前约需 ${flowLabel(prediction.projectedMb)}${prediction.shortfallMb > 0 ? `，可能超出余量 ${flowLabel(prediction.shortfallMb)}` : '，当前余量预计足够'}。` : '积累至少两天数据后显示用量预测。';
            section.append(estimate);
            if (account.monthlyFee != null && account.balance) {
                const balance = Number(account.balance.replace(/[元¥￥\s]/g, ''));
                if (Number.isFinite(balance)) {
                    const fee = document.createElement('p');
                    fee.className = 'device-usage-note';
                    fee.textContent = `按当前套餐月费 ${account.monthlyFee.toFixed(2)} 元估算 · ${balance >= account.monthlyFee ? '当前余额足够' : '当前余额不足'}（以计费结算为准）`;
                    section.append(fee);
                }
            }
            const list = document.createElement('div');
            list.className = 'device-usage-list';
            for (const device of account.devices) {
                const id = usageIdentity(this.source, account.account, device);
                seen.add(id);
                const previous = this.samples.get(id);
                if (!previous || previous.at !== account.updatedAt) {
                    this.deltas.set(id, usageDelta(previous, account.updatedAt, device.usedFlowMb));
                    if (/^\d+(\.\d+)?$/.test(device.usedFlowMb))
                        this.samples.set(id, { at: account.updatedAt, used: Number(device.usedFlowMb) });
                }
                const row = document.createElement('article');
                row.className = 'device-usage-row';
                const identity = document.createElement('div'), name = document.createElement('strong'), address = document.createElement('small');
                name.textContent = device.mac || device.ip || '在线设备';
                address.textContent = [device.ip, device.ipv6].filter(Boolean).join(' · ');
                identity.append(name, address);
                const amount = document.createElement('div'), used = document.createElement('strong'), delta = document.createElement('small');
                used.textContent = /^\d+(\.\d+)?$/.test(device.usedFlowMb) ? flowLabel(Number(device.usedFlowMb)) : '--';
                const difference = this.deltas.get(id);
                delta.textContent = !stale && difference != null ? `本次更新 +${flowLabel(difference)}` : '等待下一次更新';
                amount.append(used, delta);
                row.append(identity, amount);
                list.append(row);
            }
            if (!account.devices.length && account.updatedAt && !account.error) {
                const empty = document.createElement('p');
                empty.textContent = '当前没有在线设备';
                list.append(empty);
            }
            section.append(list);
            fragment.append(section);
        }
        const controls = document.createElement('div');
        controls.className = 'device-usage-filters';
        const button = (text: string, selected: boolean, action: () => void) => { const btn = document.createElement('button'); btn.type = 'button'; btn.className = `btn ${selected ? 'btn-primary' : 'btn-secondary'} btn-sm`; btn.textContent = text; btn.setAttribute('aria-pressed', String(selected)); btn.onclick = action; controls.append(btn); };
        for (const days of [7, 30, 90])
            button(`${days} 天`, this.days === days, () => { this.days = days; this.render(this.accounts, '', this.history); });
        for (const monthly of [false, true])
            button(monthly ? '按月' : '按天', this.monthly === monthly, () => { this.monthly = monthly; this.render(this.accounts, '', this.history); });
        for (const account of ['', ...new Set(accounts.map(a => a.account))])
            button(account || '全部账号', account === this.accountFilter, () => { this.accountFilter = account; this.render(this.accounts, '', this.history); });
        const businessNow = Date.now() + 8 * 3600000;
        const earliest = new Date(businessNow - (this.days - 1) * 86400000).toISOString().slice(0, 10);
        const today = new Date(businessNow).toISOString().slice(0, 10);
        const rows = history.filter(row => row.day >= earliest && row.day <= today && (!this.accountFilter || row.account === this.accountFilter));
        const totals = new Map<string, number>();
        for (const row of rows) {
            const period = this.monthly ? row.day.slice(0, 7) : row.day;
            totals.set(period, (totals.get(period) || 0) + row.usedMb);
        }
        const chart = document.createElement('div');
        chart.className = 'usage-history-chart';
        const max = Math.max(1, ...totals.values());
        for (const [day, amount] of [...totals].sort(([a], [b]) => a.localeCompare(b))) {
            const bar = document.createElement('div');
            bar.className = 'usage-history-bar';
            const label = document.createElement('span'), fill = document.createElement('span'), number = document.createElement('strong');
            label.textContent = this.monthly ? day : day.slice(5);
            fill.style.setProperty('--usage-width', `${amount / max * 100}%`);
            number.textContent = flowLabel(amount);
            bar.append(label, fill, number);
            chart.append(bar);
        }
        button('导出 CSV', false, () => { void (async () => { const csv = usageCsv(rows); if (window.__TAURI__) {
            const destination = await invoke<string>('export_billing_csv', { kind: 'usage', csv });
            if (window.AndroidBridge?.shareExportFile) {
                window.AndroidBridge.shareExportFile(destination, '设备流量统计');
                return;
            }
            const { form, close } = presentationDialog('usage-export-dialog', '已导出');
            const path = document.createElement('p');
            path.textContent = destination;
            const done = document.createElement('button');
            done.type = 'button';
            done.className = 'btn btn-primary';
            done.textContent = '完成';
            done.onclick = close;
            form.append(path, done);
        }
        else {
            const link = document.createElement('a');
            link.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }));
            link.download = 'device-usage.csv';
            link.click();
            setTimeout(() => URL.revokeObjectURL(link.href), 1000);
        } })().catch(() => { }); });
        const historyNote = document.createElement('p');
        historyNote.className = 'device-usage-note';
        historyNote.textContent = `近 ${this.days} 天采样新增 ${flowLabel(rows.reduce((sum, row) => sum + row.usedMb, 0))}。仅累计连续采样区间，未采集期间以学校账单为准。`;
        fragment.append(controls, historyNote, chart);
        // Keep baselines for filtered-out accounts; prune only removed sessions.
        const live = new Set(accounts.flatMap(a => a.devices.map(d => usageIdentity(this.source, a.account, d))));
        for (const key of this.samples.keys())
            if (!live.has(key)) {
                this.samples.delete(key);
                this.deltas.delete(key);
            }
        animateResize(this.root, () => this.root.replaceChildren(fragment));
    }
}
export function setupDeviceUsage() {
    const view = new DeviceUsageView(document.getElementById('device-usage-panel')!, 'local');
    let busy = false;
    const refresh = async () => {
        if (busy || document.hidden)
            return;
        busy = true;
        try {
            if (localStorage.getItem('bjut_traffic_monitor_enabled') === 'false') {
                view.render([], '已暂停统计，可在设置中开启。');
                return;
            }
            const [data, history] = await Promise.all([invoke<AccountUsage[]>('get_device_usage'), invoke<DailyUsage[]>('get_usage_history')]);
            view.render(data, '', history || []);
        }
        catch {
            view.render([], '暂时无法读取设备流量。');
        }
        finally {
            busy = false;
        }
    };
    // Reading the local cache is cheap; the backend owns the 60-second schedule.
    window.setInterval(() => { void refresh(); }, 5000);
    window.addEventListener('focus', () => { void refresh(); });
    void refresh();
}
