export interface DeviceUsage {
    loginAt: string;
    ip: string;
    ipv6?: string;
    mac: string;
    usedFlowMb: string;
    sessionId?: string;
    interfaceId?: string;
    interfaceName?: string;
    local?: boolean;
}
export interface AccountUsage {
    account: string;
    updatedAt: number;
    devices: DeviceUsage[];
    error?: string | null;
    balance?: string;
    remainingFlow?: string;
    monthlyFee?: number | null;
}
export interface DailyUsage {
    day: string;
    account: string;
    device: string;
    label: string;
    usedMb: number;
    sampledSeconds: number;
    local?: boolean;
}
export type UsageScope = 'local' | 'all';
export interface UsagePoint {
    at: number;
    resolution: number;
    account: string;
    device: string;
    interfaceId: string;
    interfaceName: string;
    local: boolean;
    usedMb: number;
    sampledSeconds: number;
    startedAt?: number;
    endedAt?: number;
}
export interface UsageRetention { minuteHours: number; hourDays: number; dayDays: number; storageMb?: number; }
export const defaultUsageRetention: UsageRetention = {minuteHours: 48, hourDays: 90, dayDays: 366,storageMb:64};
export function usageStorageEstimate(devices: number, retention: UsageRetention): number {
    // Includes the minute/hour/day rows and an atomic checkpoint copy.
    return Math.ceil(Math.max(1, devices) * (retention.minuteHours * 60 + retention.hourDays * 24 + retention.dayDays) * 180 * 2);
}
export interface UsageSwitch { at: number; interfaceId: string; interfaceName: string; from: string; to: string; reason?: string; }
export function usageTooltipPosition(width:number,height:number,boxWidth:number,boxHeight:number,x:number,y:number) {
    const gap=32,pad=8,clamp=(value:number,maximum:number)=>Math.max(pad,Math.min(Math.max(pad,maximum-pad),value));
    // Prefer a side that leaves the pointer's whole column unobstructed.
    const side=x+gap+boxWidth<=width-pad?x+gap:x-gap-boxWidth>=pad?x-gap-boxWidth:null;
    const left=side??clamp(x-boxWidth/2,width-boxWidth);
    // Narrow plots may have no room on either side. Let the tooltip extend
    // above the plot rather than covering the sample beneath the pointer.
    const top=y+gap+boxHeight<=height-pad?y+gap:y-gap-boxHeight>=pad?y-gap-boxHeight:side===null?y-gap-boxHeight:clamp(y-boxHeight/2,height-boxHeight);
    return {left,top};
}
export interface UsageQuery {start:number;end:number;resolution:number;monthly:boolean;}
export interface UsageReport {
    enabled: boolean;
    scope: UsageScope;
    accounts: AccountUsage[];
    history: DailyUsage[];
    series: UsagePoint[];
    metric?: string;
    retention?: UsageRetention;
    switches?: UsageSwitch[];
    timeline?: {
        channels: Omit<UsagePoint, 'at' | 'resolution' | 'usedMb' | 'sampledSeconds'>[];
        buckets?: number[][];
        base?: number;
        resolution?: number;
        rows?: string;
    };
}
type RouterUsagePart = 'summary' | 'series' | 'compact';
type RouterUsageRequest = UsageQuery & {part?: RouterUsagePart};
type RouterUsageResponse = UsageReport & {part?: RouterUsagePart; seriesEncoding?: string; state?: string};
export interface UsageLoadProgress {completed: number; total: number;}

function requireUsageReport(report: RouterUsageResponse) {
    if (typeof report?.enabled !== 'boolean' || (report.scope !== 'local' && report.scope !== 'all')) {
        throw new Error('路由器未返回流量统计状态');
    }
}

function routerUsageWindows(query: UsageQuery, compact: boolean): UsageQuery[] {
    const windows: UsageQuery[] = [];
    const add = (start: number, end: number, resolution: number) => {
        const days = resolution === 60 ? (compact ? 3 : 1) : resolution === 3600 ? 7 : 90;
        const span = days * 86400;
        while (start <= end) {
            const boundary = (Math.floor((start + 28800) / span) + 1) * span - 28800;
            const last = Math.min(end, boundary - 1);
            windows.push({start, end: last, resolution, monthly: false});
            start = boundary;
        }
    };
    const start = Math.max(0, Math.floor((query.start + 28800) / query.resolution) * query.resolution - 28800);
    if (query.monthly) {
        const day = usageBucket(start, 'day'), hour = usageBucket(start, 'hour');
        add(Math.max(0, usageMonthStart(start)), day - 1, 86400);
        add(Math.max(0, day), hour - 1, 3600);
        add(Math.max(0, hour), start - 1, 60);
    }
    add(start, query.end, query.resolution);
    return windows;
}

/** Load closed time windows once and refresh the recent tail as samples arrive. */
export class RouterUsageReader {
    private pages = new Map<string, {expires: number; rows: UsagePoint[]}>();
    private cachedPoints = 0;
    private generation = 0;

    clear() { this.generation++; this.pages.clear(); this.cachedPoints = 0; }

    private remember(key: string, expires: number, rows: UsagePoint[]) {
        this.cachedPoints -= this.pages.get(key)?.rows.length ?? 0;
        this.pages.delete(key);
        this.pages.set(key, {expires, rows});
        this.cachedPoints += rows.length;
        for (const [oldKey, old] of this.pages) {
            if (this.pages.size <= 48 && this.cachedPoints <= 240_000) break;
            this.cachedPoints -= old.rows.length;
            this.pages.delete(oldKey);
        }
    }

    async read(
        request: (query: RouterUsageRequest) => Promise<RouterUsageResponse>,
        query: UsageQuery,
        current: () => boolean = () => true,
        progress?: (value: UsageLoadProgress) => void,
    ): Promise<UsageReport | null> {
        const now = Date.now();
        const generation = this.generation;
        let stopped = false;
        const active = () => !stopped && generation === this.generation && current();
        // A short range also works on routers that ignore the optional part.
        const at = Math.floor(now / 1000);
        const summary = await request({part: 'summary', start: at, end: at, resolution: 86400, monthly: false});
        if (!active()) return null;
        requireUsageReport(summary);
        if (!summary.enabled) {
            this.clear();
            return summary;
        }
        const paged = summary.part === 'summary';
        const compact = paged && summary.seriesEncoding === 'compact-v1';
        const part = compact ? 'compact' : paged ? 'series' : undefined;
        const load = async (window: UsageQuery): Promise<UsagePoint[] | null> => {
            if (!active()) return null;
            const page = await request(part ? {...window, part} : window);
            if (!active()) return null;
            if (page?.state === 'split') {
                const steps = Math.floor((window.end - window.start) / window.resolution);
                if (steps < 1) throw new Error('此时段的流量数据过多');
                const boundary = window.start + Math.ceil(steps / 2) * window.resolution;
                const left = await load({...window, end: boundary - 1});
                if (!left) return null;
                const right = await load({...window, start: boundary});
                return right ? left.concat(right) : null;
            }
            requireUsageReport(page);
            if (!page.enabled || page.scope !== summary.scope) return null;
            if (compact && (page.part !== 'compact' || typeof page.timeline?.rows !== 'string'
                || page.timeline.resolution !== window.resolution || !Number.isSafeInteger(page.timeline.base)
                || !Array.isArray(page.timeline.channels))) throw new Error('路由器返回的流量分段不完整');
            return normalizeUsageReport(page).series.filter(row =>
                row.resolution === window.resolution && row.at >= window.start && row.at <= window.end);
        };
        const windows = routerUsageWindows(query, compact);
        const rows = new Array<UsagePoint[]>(windows.length);
        const pending: {index: number; key: string; window: UsageQuery}[] = [];
        for (const [index, window] of windows.entries()) {
            const key = JSON.stringify([part, summary.scope, window.start, window.end, window.resolution]);
            const cached = this.pages.get(key);
            if (cached && cached.expires > now) rows[index] = cached.rows;
            else pending.push({index, key, window});
        }
        let completed = windows.length - pending.length, cursor = 0;
        // Periodic refreshes of a single tail segment do not flash the bar.
        const reportProgress = () => {
            if (pending.length > 1) progress?.({completed, total: windows.length});
        };
        reportProgress();
        const worker = async () => {
            try {
                while (active() && cursor < pending.length) {
                    const {index, key, window} = pending[cursor++];
                    const page = await load(window);
                    if (!page) { stopped = true; return; }
                    if (window.end < at - 180) this.remember(key, now + 5 * 60_000, page);
                    rows[index] = page;
                    completed++;
                    reportProgress();
                }
            } catch (error) {
                stopped = true;
                throw error;
            }
        };
        // Bound peak work on the router and drain in-flight pages on a range change.
        const results = await Promise.allSettled(Array.from({length: Math.min(2, pending.length)}, worker));
        for (const result of results) if (result.status === 'rejected') throw result.reason;
        if (!active()) return null;
        const series: UsagePoint[] = [];
        for (const page of rows) for (const point of page) series.push(point);
        return {...summary, timeline: undefined, series};
    }
}

export function normalizeUsageReport(report: UsageReport): UsageReport {
    const timeline = report.timeline;
    const series = timeline ? [...(report.series || [])] : report.series || [];
    for (const row of timeline?.buckets || []) {
        const channel = timeline?.channels[row[0]];
        if (!channel || ![5,7].includes(row.length) || !row.every(Number.isFinite) || ![60, 3600, 86400].includes(row[1]) || row[3] < 0 || row[4] <= 0) continue;
        series.push({...channel, resolution: row[1], at: row[2], usedMb: row[3], sampledSeconds: row[4], ...(row.length === 7 ? {startedAt:row[5],endedAt:row[6]} : {})});
    }
    if (timeline && typeof timeline.rows === 'string' && timeline.base !== undefined && timeline.resolution !== undefined) {
        const {base, resolution, channels, rows} = timeline;
        if (Number.isSafeInteger(base) && [60, 3600, 86400].includes(resolution)) for (const line of rows.split('\n')) {
            if (!line) continue;
            const point = line.split(',').map(Number);
            const [index, offset, usedMb, sampledSeconds, start, end] = point;
            const channel = channels[index];
            if (!channel || (point.length !== 4 && point.length !== 6) || !point.every(Number.isFinite)
                || !Number.isSafeInteger(offset) || offset < 0 || usedMb < 0 || sampledSeconds <= 0) continue;
            const at = base + offset;
            series.push({...channel, at, resolution, usedMb, sampledSeconds,
                ...(point.length === 6 ? {startedAt: at + start, endedAt: at + end} : {})});
        }
    }
    // Scope is enforced by the sampler/API and again here for cached data from
    // an earlier all-device session. Unclassified legacy rows are not local.
    const all = report.scope === 'all';
    return {...report, timeline: undefined, accounts: (report.accounts || []).map(account => ({...account, devices: (account.devices || []).filter(device => all || device.local === true)})), history: (report.history || []).filter(row => all || row.local === true), series: !all && series.some(row => row.local !== true) ? series.filter(row => row.local === true) : series};
}
export const usageScales = [
    { value: 'minute', text: '每分钟', seconds: 60, ranges: [3600, 21600, 86400, 172800] },
    { value: '5minutes', text: '每 5 分钟', seconds: 300, ranges: [21600, 86400] },
    { value: '15minutes', text: '每 15 分钟', seconds: 900, ranges: [21600, 86400] },
    { value: 'hour', text: '每小时', seconds: 3600, ranges: [86400, 604800, 2592000] },
    { value: 'day', text: '每天', seconds: 86400, ranges: [604800, 2592000, 7776000, 31536000] },
    { value: 'month', text: '每月', seconds: 0, ranges: [15552000, 31536000] },
] as const;
export type UsageScale = typeof usageScales[number]['value'];
export interface UsageChartOptions {
    scale: UsageScale;
    range: number;
    mode: 'delta' | 'cumulative' | 'monthly';
    presentation: 'sum' | 'interfaces';
    interfaces: string[] | null;
    account: string;
    now?: number;
    start?: number;
    end?: number;
}
export interface UsageCurve { id: string; name: string; points: (number | null)[]; totalMb: number; joinPrevious?: boolean[]; }
export function usageBucket(at: number, scale: UsageScale): number {
    if (scale === 'month') {
        const date = new Date((at + 28800) * 1000);
        return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), 1) / 1000 - 28800;
    }
    const seconds = usageScales.find(item => item.value === scale)!.seconds;
    return Math.floor((at + 28800) / seconds) * seconds - 28800;
}
export function usageRangeLabel(seconds: number): string {
    return seconds < 86400 ? `近 ${seconds / 3600} 小时` : seconds < 15552000 ? `近 ${seconds / 86400} 天` : `近 ${seconds === 31536000 ? 12 : 6} 个月`;
}
export function usageInterfaces(points: UsagePoint[], account = ''): {id: string; name: string}[] {
    const interfaces = new Map<string, string>();
    for (const point of points) if (!account || point.account === account) interfaces.set(point.interfaceId, point.interfaceName);
    return [...interfaces].map(([id, name]) => ({id, name})).sort((a, b) => a.name.localeCompare(b.name, 'zh-CN'));
}
export function usageMonthStart(at: number): number {return usageBucket(at,'month');}
export function usageZoom(start: number, end: number, anchor: number, factor: number): [number, number] {
    const width = Math.max(60, Math.min(3660*86400, (end-start)*factor));
    const ratio = end > start ? Math.max(0,Math.min(1,(anchor-start)/(end-start))) : .5;
    return [anchor-width*ratio,anchor+width*(1-ratio)];
}
export function usageDisplayScale(scale: UsageScale, start: number, end: number) {
    let index = usageScales.findIndex(item => item.value === scale);
    while (usageScales[index].seconds && (end-start)/usageScales[index].seconds > 4500) index++;
    return usageScales[index];
}
export function buildUsageChart(rows: UsagePoint[], options: UsageChartOptions): { times: number[]; curves: UsageCurve[]; samples: number; totalMb: number; scale: UsageScale } {
    const now = options.now ?? Date.now() / 1000;
    const requestedStart = options.start ?? now-options.range, requestedEnd = Math.min(now,options.end ?? now);
    const scale = usageDisplayScale(options.scale, requestedStart, requestedEnd);
    const resolution = scale.value === 'month' || scale.seconds >= 86400 ? 86400 : scale.seconds >= 3600 ? 3600 : 60;
    const start = usageBucket(requestedStart, scale.value), end = usageBucket(requestedEnd, scale.value);
    const times: number[] = [];
    for (let at = start; at <= end && times.length < 5000;) {
        times.push(at);
        if (scale.value === 'month') {
            const date = new Date((at + 28800) * 1000);
            at = Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 1) / 1000 - 28800;
        } else at += scale.seconds;
    }
    const index = new Map(times.map((at, i) => [at, i])), curves = new Map<string, UsageCurve>();
    const starts = new Map<string,number[]>(), ends = new Map<string,number[]>();
    const accepted = rows.filter(row=>Number.isFinite(row.at)&&row.at<=now&&Number.isFinite(row.usedMb)&&row.usedMb>=0&&row.sampledSeconds>0&&Number.isFinite(row.sampledSeconds)&&(!options.account||row.account===options.account)&&(options.presentation!=='interfaces'||options.interfaces===null||options.interfaces.includes(row.interfaceId)));
    let samples = 0, totalMb = 0;
    for (const row of accepted) {
        if (row.resolution !== resolution || (row.endedAt ?? row.at)>requestedEnd) continue;
        const i = index.get(usageBucket(row.at, scale.value));
        if (i === undefined) continue;
        const id = options.presentation === 'sum' ? 'sum' : row.interfaceId;
        const curve = curves.get(id) || { id, name: options.presentation === 'sum' ? '所有接口合计' : row.interfaceName, points: Array<number | null>(times.length).fill(null), totalMb: 0 };
        curve.points[i] = (curve.points[i] ?? 0) + row.usedMb;
        curve.totalMb += row.usedMb;
        const left=starts.get(id)||Array<number>(times.length).fill(Infinity),right=ends.get(id)||Array<number>(times.length).fill(-Infinity);
        if (Number.isFinite(row.startedAt)&&Number.isFinite(row.endedAt)&&row.endedAt!>=row.startedAt!) {left[i]=Math.min(left[i],row.startedAt!);right[i]=Math.max(right[i],row.endedAt!);}
        starts.set(id,left);ends.set(id,right);
        curves.set(id, curve); totalMb += row.usedMb; samples++;
    }
    const ordered = [...curves.values()].sort((a, b) => a.name.localeCompare(b.name, 'zh-CN'));
    for (const curve of ordered) {
        let previous=-1, sum=0, month=-1;
        curve.joinPrevious=curve.points.map((point,i)=>{
            if(point===null)return false;
            const join=previous>=0&&(i===previous+1||(resolution===60&&starts.get(curve.id)![i]<=ends.get(curve.id)![previous]+1));
            previous=i;return join;
        });
        if (options.mode === 'delta') continue;
        curve.points = curve.points.map((point,i) => {
            if(point===null)return null;
            const currentMonth=usageMonthStart(times[i]);
            if(options.mode==='monthly'&&month!==currentMonth){
                month=currentMonth;sum=0;
                const stop=times[i],day=usageBucket(stop,'day'),hour=usageBucket(stop,'hour');
                for(const row of accepted){
                    if((options.presentation==='interfaces'&&row.interfaceId!==curve.id)||row.at<month||row.at>=stop)continue;
                    if((row.resolution===86400&&row.at<day)||(row.resolution===3600&&row.at>=day&&row.at<hour)||(row.resolution===60&&row.at>=hour))sum+=row.usedMb;
                }
            }
            sum+=point;return sum;
        });
    }
    return { times, curves: ordered, samples, totalMb, scale:scale.value };
}
export function usageChartCsv(chart: ReturnType<typeof buildUsageChart>, mode: UsageChartOptions['mode']): string {
    const rows: (string | number)[][] = [['时间（北京时间）', '接口', mode === 'delta' ? '采样新增下载量（MB）' : mode==='monthly'?'当月累计下载量（MB）':'所选范围累计下载量（MB）']];
    chart.times.forEach((at, i) => chart.curves.forEach(curve => {
        if (curve.points[i] !== null) rows.push([new Date((at + 28800) * 1000).toISOString().slice(0, 19).replace('T', ' '), curve.name, curve.points[i]!.toFixed(3)]);
    }));
    return '\uFEFF' + rows.map(row => row.map(value => {const raw = String(value); return `"${(/^[=+\-@]/.test(raw.trimStart()) ? "'" + raw : raw).replaceAll('"', '""')}"`;}).join(',')).join('\r\n');
}
export interface UsageSample {
    at: number;
    used: number;
}
export function usageIdentity(source: string, account: string, device: DeviceUsage): string {
    return JSON.stringify([source, account, device.mac.replace(/[:-]/g, '').toLowerCase(), device.ip, device.loginAt]);
}
export function usageDelta(previous: UsageSample | undefined, at: number, value: string): number | null {
    if (!/^\d+(\.\d+)?$/.test(value))
        return null;
    const used = Number(value);
    return Number.isFinite(used) && previous && at > previous.at && at - previous.at <= 180 && used >= previous.used ? used - previous.used : null;
}
export function flowLabel(value: number): string { return value >= 1024 ? `${(value / 1024).toFixed(2)} GB` : `${value.toFixed(2)} MB`; }
export function flowMb(value: string): number | null {
    const match = value.trim().match(/^(\d+(?:\.\d+)?)\s*(MB|GB|TB)?$/i);
    if (!match)
        return null;
    return Number(match[1]) * ({ MB: 1, GB: 1024, TB: 1048576 }[match[2]?.toUpperCase() || 'MB'] || 1);
}
export function forecastUsage(rows: DailyUsage[], remaining: string, now = new Date()): {
    projectedMb: number;
    daysLeft: number;
    shortfallMb: number;
} | null {
    const available = flowMb(remaining);
    if (available === null)
        return null;
    const businessNow = new Date(now.getTime() + 8 * 3600000), today = businessNow.toISOString().slice(0, 10);
    const recent = rows.filter(row => row.day >= new Date(businessNow.getTime() - 7 * 86400000).toISOString().slice(0, 10) && row.day <= today && Number.isFinite(row.usedMb) && row.usedMb >= 0 && Number.isFinite(row.sampledSeconds) && row.sampledSeconds > 0);
    const seconds = recent.reduce((sum, row) => sum + row.sampledSeconds, 0), total = recent.reduce((sum, row) => sum + row.usedMb, 0);
    if (new Set(recent.map(row => row.day)).size < 2 || seconds < 3600 || seconds <= 0)
        return null;
    const end = Date.UTC(businessNow.getUTCFullYear(), businessNow.getUTCMonth() + 1, 1) - 8 * 3600000, daysLeft = Math.max(0, (end - now.getTime()) / 86400000);
    // Multiple devices run concurrently: average over the longest sampled
    // device duration each day, rather than dividing by summed device time.
    const days = new Map<string, Map<string, number>>();
    for (const row of recent) {
        const devices = days.get(row.day) || new Map<string, number>();
        devices.set(row.device, (devices.get(row.device) || 0) + row.sampledSeconds);
        days.set(row.day, devices);
    }
    const observed = Array.from(days.values()).reduce((sum, devices) => sum + Math.max(...devices.values()), 0);
    const projectedMb = total / observed * daysLeft * 86400;
    return { projectedMb, daysLeft, shortfallMb: Math.max(0, projectedMb - available) };
}
export function usageCsv(rows: DailyUsage[]): string {
    const cell = (value: unknown) => { const raw = String(value); return `"${(/^[=+\-@]/.test(raw.trimStart()) ? "'" + raw : raw).replaceAll('"', '""')}"`; };
    return '\uFEFF' + [['日期', '账号', '设备', '采样新增流量（MB）', '连续采样秒数'], ...rows.map(row => [row.day, row.account, row.label, row.usedMb.toFixed(3), row.sampledSeconds])].map(row => row.map(cell).join(',')).join('\r\n');
}
