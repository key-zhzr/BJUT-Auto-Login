import { buildUsageChart, normalizeUsageReport, forecastUsage, flowLabel, usageChartCsv, usageInterfaces, usageRangeLabel, usageScales, type UsageChartOptions, type UsageReport, type UsageScope } from './device-usage-model';

export interface UsageSelect {
    element: HTMLElement;
    value: string;
    setOptions: (options: {value: string; text: string}[]) => void;
}
export interface UsageChartEnvironment {
    title?: string;
    localLabel?: string;
    select?: (label: string, options: {value: string; text: string}[], change: (value: string) => void) => UsageSelect;
    resize?: (element: HTMLElement, change: () => void) => void;
    configure: (enabled: boolean, scope: UsageScope) => Promise<void>;
    exportCsv?: (csv: string) => Promise<void>;
}
const usageColors = ['#1584e8', '#ef7c25', '#0b9d8a', '#a16add', '#db5376', '#7a9430', '#18a5bf', '#a07554'];
function usageElement<K extends keyof HTMLElementTagNameMap>(tag: K, className = '', text = '') {
    const node = document.createElement(tag); node.className = className; node.textContent = text; return node;
}
function usageNativeSelect(label: string, options: {value: string; text: string}[], change: (value: string) => void): UsageSelect {
    const element = usageElement('select'); element.setAttribute('aria-label', label);
    const control = { element, get value() { return element.value; }, set value(value: string) {element.value = value;}, setOptions(choices: {value: string; text: string}[]) {
        const value = element.value;
        element.replaceChildren(...choices.map(choice => {const option = usageElement('option', '', choice.text); option.value = choice.value; return option;}));
        element.value = choices.some(choice => choice.value === value) ? value : choices[0]?.value || '';
    }};
    control.setOptions(options); element.onchange = () => change(element.value); return control;
}

/** Shared DOM renderer: the App supplies its existing menus/motion; LuCI supplies its own controls. */
export class UsageChart {
    private data: UsageReport = {enabled: false, scope: 'local', accounts: [], history: [], series: []};
    private options: UsageChartOptions = {scale: 'minute', range: 21600, mode: 'delta', presentation: 'sum', interfaces: null, account: ''};
    private title = usageElement('h3', '', '流量趋势');
    private status = usageElement('p', 'traffic-note');
    private enabled = usageElement('button', 'btn btn-secondary btn-sm', '开启统计');
    private content = usageElement('div', 'traffic-content');
    private scopeLocal = usageElement('button', 'btn btn-secondary btn-sm', '仅本机设备');
    private scopeAll = usageElement('button', 'btn btn-secondary btn-sm', '全部在线设备');
    private interfaceChoices = usageElement('div', 'traffic-interfaces');
    private legend = usageElement('div', 'traffic-legend');
    private plot = usageElement('div', 'traffic-plot');
    private tooltip = usageElement('div', 'traffic-tooltip');
    private summary = usageElement('p', 'traffic-note');
    private devices = usageElement('div', 'traffic-device-list');
    private range: UsageSelect;
    private account: UsageSelect;
    private canvas: SVGSVGElement | null = null;
    private cursor: SVGLineElement | null = null;
    private chart = buildUsageChart([], this.options);
    private width = 1000;
    private hover = -1;
    private busy = false;
    private accountStamp = '';
    private interfacesStamp = '';
    private stamp = '';
    private observer?: ResizeObserver;
    constructor(private root: HTMLElement, private environment: UsageChartEnvironment) {
        root.classList.add('traffic-chart');
        this.title.textContent = environment.title || '流量趋势';
        this.scopeLocal.textContent = environment.localLabel || '仅本机设备';
        const header = usageElement('div', 'traffic-heading'), intro = usageElement('div'); intro.append(this.title, this.status);
        this.enabled.type = 'button'; this.enabled.onclick = () => {void this.configure(!this.data.enabled, this.data.scope);};
        header.append(intro, this.enabled);
        const scope = usageElement('div', 'traffic-scope'); scope.append(usageElement('span', '', '统计范围'), this.scopeLocal, this.scopeAll);
        this.scopeLocal.type = this.scopeAll.type = 'button';
        this.scopeLocal.onclick = () => {void this.configure(this.data.enabled, 'local');};
        this.scopeAll.onclick = () => {void this.configure(this.data.enabled, 'all');};
        const controls = usageElement('div', 'traffic-controls');
        const select = (label: string, choices: {value: string; text: string}[], change: (value: string) => void) => {
            const field = usageElement('div', 'traffic-field'), caption = usageElement('span', '', label);
            const control = (environment.select || usageNativeSelect)(label, choices, change);
            field.append(caption, control.element); controls.append(field); return control;
        };
        select('统计尺度', usageScales.map(item => ({value: item.value, text: item.text})), value => {
            this.options.scale = value as UsageChartOptions['scale'];
            const scale = usageScales.find(item => item.value === value)!;
            if (!(scale.ranges as readonly number[]).includes(this.options.range)) this.options.range = scale.ranges[scale.ranges.length > 1 ? 1 : 0];
            this.range.setOptions(scale.ranges.map(seconds => ({value: String(seconds), text: usageRangeLabel(seconds)})));
            this.range.value = String(this.options.range); this.draw();
        });
        this.range = select('时间范围', usageScales[0].ranges.map(seconds => ({value: String(seconds), text: usageRangeLabel(seconds)})), value => {this.options.range = Number(value); this.draw();});
        this.range.value = String(this.options.range);
        select('流量显示', [{value: 'delta', text: '每段新增流量'}, {value: 'cumulative', text: '所选范围累计'}], value => {this.options.mode = value as UsageChartOptions['mode']; this.draw();});
        select('曲线', [{value: 'sum', text: '所有接口合计'}, {value: 'interfaces', text: '按接口同时显示'}], value => {
            this.options.presentation = value as UsageChartOptions['presentation'];
            this.draw();
        });
        this.account = select('账号', [{value: '', text: '全部账号'}], value => {this.options.account = value; this.options.interfaces = null; this.draw();});
        const exportButton = usageElement('button', 'btn btn-secondary btn-sm', '导出 CSV'); exportButton.type = 'button';
        exportButton.onclick = () => {void this.export().catch(() => {this.status.textContent = '导出失败，请重试。';});};
        controls.append(exportButton); this.interfaceChoices.hidden = true;
        this.plot.tabIndex = 0; this.plot.setAttribute('role', 'group'); this.plot.setAttribute('aria-label', '流量趋势，使用左右方向键查看采样值');
        this.plot.onpointermove = event => {const bounds = this.plot.getBoundingClientRect(); this.showPoint(Math.round(((event.clientX - bounds.left) / bounds.width * this.width - 68) / (this.width - 86) * (this.chart.times.length - 1)));};
        this.plot.onpointerleave = () => {if (document.activeElement !== this.plot) this.hidePoint();};
        this.plot.onblur = () => this.hidePoint();
        this.plot.onkeydown = event => {
            if (!['ArrowLeft', 'ArrowRight', 'Home', 'End', 'Escape'].includes(event.key)) return;
            event.preventDefault();
            if (event.key === 'Escape') {this.hidePoint(); return;}
            const initial = this.chart.curves.flatMap(curve => curve.points.map((point, index) => point === null ? -1 : index)).filter(index => index >= 0);
            this.showPoint(event.key === 'Home' ? 0 : event.key === 'End' ? this.chart.times.length - 1 : this.hover < 0 ? Math.min(...initial, this.chart.times.length - 1) : this.hover + (event.key === 'ArrowLeft' ? -1 : 1));
        };
        const details = usageElement('details', 'traffic-devices'); details.append(usageElement('summary', '', '当前在线设备'), this.devices);
        const note = usageElement('p', 'traffic-note', '计费系统每分钟采样 · 空白表示无连续采样 · 累计仅包含所选范围内记录');
        this.content.append(scope, controls, this.interfaceChoices, this.legend, this.plot, this.summary, note, details);
        this.content.hidden = true; root.replaceChildren(header, this.content);
        if (typeof ResizeObserver !== 'undefined') {
            this.observer = new ResizeObserver(() => {
                const width = Math.max(300, Math.round(this.plot.clientWidth));
                if (width !== this.width && !this.content.hidden) {this.width = width; this.paint();}
            });
            this.observer.observe(this.plot);
        }
        this.update(this.data);
    }
    private resize(element: HTMLElement, change: () => void) {if (this.environment.resize) this.environment.resize(element, change); else change();}
    private async configure(enabled: boolean, scope: UsageScope) {
        if (this.busy || (enabled === this.data.enabled && scope === this.data.scope)) return;
        this.busy = true; this.enabled.disabled = this.scopeLocal.disabled = this.scopeAll.disabled = true;
        try {await this.environment.configure(enabled, scope);} catch {this.status.textContent = '设置未保存，请重试。';}
        finally {this.busy = false; this.enabled.disabled = this.scopeLocal.disabled = this.scopeAll.disabled = false;}
    }
    update(data: UsageReport, message = '') {
        const scoped = normalizeUsageReport(data);
        const next = {...scoped, enabled: data.enabled === true, scope: data.scope === 'all' ? 'all' as const : 'local' as const, accounts: scoped.accounts, history: scoped.history, series: scoped.series};
        const stamp = JSON.stringify([next, message]); if (stamp === this.stamp) return; this.stamp = stamp; this.data = next;
        this.enabled.textContent = next.enabled ? '暂停统计' : '开启统计'; this.enabled.setAttribute('aria-pressed', String(next.enabled));
        this.status.textContent = message || (next.enabled ? `计费系统数据 · 每分钟更新 · ${next.scope === 'local' ? this.scopeLocal.textContent : '全部在线设备'}` : '流量统计未开启，可手动开启。');
        this.resize(this.root, () => {
        this.content.hidden = !next.enabled;
        for (const [button, scope] of [[this.scopeLocal, 'local'], [this.scopeAll, 'all']] as const) {
            button.setAttribute('aria-pressed', String(next.scope === scope));
        }
        if (!next.enabled) return;
        const accounts = [...new Set([...next.accounts.map(item => item.account), ...next.series.map(item => item.account)])].sort();
        const accountStamp = JSON.stringify(accounts);
        if (accountStamp !== this.accountStamp) {
            this.accountStamp = accountStamp;
            this.account.setOptions([{value: '', text: '全部账号'}, ...accounts.map(value => ({value, text: value}))]);
            this.options.account = this.account.value;
        }
        this.draw(false); this.drawDevices();
        });
    }
    showError(message: string) {this.status.textContent = message;}
    destroy() {this.observer?.disconnect();}
    private updateInterfaces() {
        const choices = usageInterfaces(this.data.series, this.options.account), stamp = JSON.stringify([choices, this.options.interfaces]);
        if (stamp === this.interfacesStamp) return; this.interfacesStamp = stamp;
        this.interfaceChoices.replaceChildren();
        for (const choice of choices) {
            const label = usageElement('label'), checkbox = usageElement('input'); checkbox.type = 'checkbox'; checkbox.value = choice.id;
            checkbox.checked = this.options.interfaces === null || this.options.interfaces.includes(choice.id);
            checkbox.onchange = () => {
                this.options.interfaces = [...this.interfaceChoices.querySelectorAll<HTMLInputElement>('input:checked')].map(input => input.value);
                this.draw();
            };
            label.append(checkbox, document.createTextNode(choice.name)); this.interfaceChoices.append(label);
        }
        if (!choices.length) this.interfaceChoices.textContent = '采样后可选择接口';
    }
    private draw(animate = true) {
        if (animate) {this.resize(this.root, () => this.draw(false)); return;}
        this.interfaceChoices.hidden = this.options.presentation !== 'interfaces';
        this.updateInterfaces();
        this.chart = buildUsageChart(this.data.series, this.options); this.width = Math.max(300, this.plot.clientWidth || 1000);
        this.legend.replaceChildren(...this.chart.curves.map((curve, index) => {
            const item = usageElement('span'), dot = usageElement('i'); dot.style.background = usageColors[index % usageColors.length];
            item.append(dot, document.createTextNode(curve.name)); return item;
        }));
        this.summary.textContent = this.chart.samples ? `所选范围新增 ${flowLabel(this.chart.totalMb)} · ${this.chart.curves.length} 条曲线` : '所选范围暂无记录，连续两次采样后开始绘图。';
        this.paint();
    }
    private paint() {
        const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'), width = this.width, height = 270, left = 68, right = width - 18, bottom = height - 30, top = 20;
        svg.setAttribute('viewBox', `0 0 ${width} ${height}`); svg.setAttribute('aria-hidden', 'true');
        const node = (tag: string, attrs: Record<string, string>, text = '') => {
            const item = document.createElementNS(svg.namespaceURI, tag); for (const [key, value] of Object.entries(attrs)) item.setAttribute(key, value); item.textContent = text; svg.append(item); return item;
        };
        const maximum = Math.max(0.01, ...this.chart.curves.flatMap(curve => curve.points.filter((value): value is number => value !== null)));
        const power = 10 ** Math.floor(Math.log10(maximum)), max = Math.ceil(maximum / power / 0.5) * power * 0.5;
        const x = (index: number) => left + index / Math.max(1, this.chart.times.length - 1) * (right - left), y = (value: number) => bottom - value / max * (bottom - top);
        for (let i = 0; i <= 4; i++) {
            const value = max * i / 4, position = y(value);
            node('line', {x1: String(left), x2: String(right), y1: String(position), y2: String(position), class: 'traffic-grid'});
            node('text', {x: String(left - 10), y: String(position + 4), 'text-anchor': 'end', class: 'traffic-axis'}, flowLabel(value));
        }
        const labelCount = width < 500 ? 2 : 4;
        for (let i = 0; i < labelCount; i++) {
            const index = Math.round(i * (this.chart.times.length - 1) / (labelCount - 1));
            node('text', {x: String(x(index)), y: String(height - 7), 'text-anchor': i === 0 ? 'start' : i === labelCount - 1 ? 'end' : 'middle', class: 'traffic-axis'}, this.timeLabel(this.chart.times[index]));
        }
        this.chart.curves.forEach((curve, index) => {
            let path = '', connected = false;
            curve.points.forEach((point, i) => {
                if (point === null) {connected = false; return;}
                path += `${connected ? 'L' : 'M'}${x(i).toFixed(2)},${y(point).toFixed(2)} `; connected = true;
                if (curve.points[i - 1] == null && curve.points[i + 1] == null) node('circle', {cx: String(x(i)), cy: String(y(point)), r: '2.5', fill: usageColors[index % usageColors.length]});
            });
            node('path', {d: path, fill: 'none', stroke: usageColors[index % usageColors.length], 'stroke-width': '1.8', 'stroke-linejoin': 'round', class: 'traffic-line'});
        });
        this.cursor = node('line', {x1: '0', x2: '0', y1: String(top), y2: String(bottom), class: 'traffic-cursor', visibility: 'hidden'}) as SVGLineElement;
        this.canvas = svg; this.plot.replaceChildren(svg, this.tooltip); this.tooltip.hidden = true; this.hover = -1;
        if (!this.chart.samples) {
            const empty = usageElement('span', 'traffic-empty', '等待连续采样'); if (this.options.presentation === 'interfaces' && this.options.interfaces?.length === 0) empty.textContent = '请选择至少一个接口';
            this.plot.append(empty);
        }
    }
    private timeLabel(at: number, full = false) {
        const date = new Date((at + 28800) * 1000).toISOString();
        return full ? date.slice(0, 16).replace('T', ' ') : this.options.scale === 'month' ? date.slice(0, 7) : this.options.scale === 'day' || this.options.range > 86400 ? date.slice(5, 10) + (this.options.scale === 'hour' ? ' ' + date.slice(11, 16) : '') : date.slice(11, 16);
    }
    private showPoint(index: number) {
        if (!this.chart.samples || !this.canvas || !this.cursor) return;
        this.hover = Math.max(0, Math.min(this.chart.times.length - 1, index));
        const x = 68 + this.hover / Math.max(1, this.chart.times.length - 1) * (this.width - 86);
        this.cursor.setAttribute('x1', String(x)); this.cursor.setAttribute('x2', String(x)); this.cursor.setAttribute('visibility', 'visible');
        this.tooltip.replaceChildren(usageElement('strong', '', this.timeLabel(this.chart.times[this.hover], true)));
        this.chart.curves.forEach((curve, i) => {
            const item = usageElement('div'), label = usageElement('span', '', curve.name), value = curve.points[this.hover];
            label.style.color = usageColors[i % usageColors.length]; item.append(label, document.createTextNode(value === null ? ' —' : ' ' + flowLabel(value))); this.tooltip.append(item);
        });
        this.tooltip.hidden = false; this.tooltip.style.left = `${Math.max(8, Math.min(this.plot.clientWidth - this.tooltip.offsetWidth - 8, x + 12))}px`;
    }
    private hidePoint() {this.hover = -1; this.tooltip.hidden = true; this.cursor?.setAttribute('visibility', 'hidden');}
    private drawDevices() {
        this.resize(this.devices, () => {
            this.devices.replaceChildren();
            for (const account of this.data.accounts) {
                const section = usageElement('section'), title = usageElement('h4', '', account.account); section.append(title);
                section.append(usageElement('p', 'traffic-note', account.error || (account.updatedAt ? `更新于 ${new Date(account.updatedAt * 1000).toLocaleTimeString('zh-CN', {hour12: false, timeZone: 'Asia/Shanghai'})}` : '等待查询')));
                for (const device of account.devices) {
                    const row = usageElement('div', 'traffic-device'), identity = usageElement('span', '', device.interfaceName || device.mac || device.ip), value = Number(device.usedFlowMb);
                    identity.append(usageElement('small', '', [device.ip || device.ipv6, device.local ? '本机设备' : '其他设备'].filter(Boolean).join(' · ')));
                    row.append(identity, usageElement('strong', '', device.usedFlowMb !== '' && Number.isFinite(value) ? flowLabel(value) : '--')); section.append(row);
                }
                if (!account.devices.length) section.append(usageElement('p', 'traffic-note', '暂无符合范围的在线设备'));
                const prediction = forecastUsage(this.data.history.filter(row => row.account === account.account), account.remainingFlow || '');
                if (prediction) section.append(usageElement('p', 'traffic-note', `按近 7 天采样估算，月底前约需 ${flowLabel(prediction.projectedMb)}${prediction.shortfallMb > 0 ? `，可能超出余量 ${flowLabel(prediction.shortfallMb)}` : '，当前余量预计足够'}`));
                this.devices.append(section);
            }
            this.devices.append(usageElement('p', 'traffic-note', '分钟明细最多保留 48 小时，小时汇总 90 天，每日汇总 12 个月；设备较多时范围会缩短。'));
            this.devices.append(usageElement('p', 'traffic-note', '以上为本次在线会话已用流量。路由器上游的用量包含经该出口转发的流量。'));
        });
    }
    private async export() {
        const csv = usageChartCsv(this.chart, this.options.mode);
        if (this.environment.exportCsv) {await this.environment.exportCsv(csv); return;}
        const link = usageElement('a'); link.download = 'device-usage.csv'; link.href = URL.createObjectURL(new Blob([csv], {type: 'text/csv;charset=utf-8'}));
        link.click(); setTimeout(() => URL.revokeObjectURL(link.href), 1000);
    }
}
