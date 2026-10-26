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
export interface UsageSwitch { at: number; interfaceId: string; interfaceName: string; from: string; to: string; }
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
        buckets: number[][];
    };
}
export function normalizeUsageReport(report: UsageReport): UsageReport {
    const series = [...(report.series || [])];
    for (const row of report.timeline?.buckets || []) {
        const channel = report.timeline?.channels[row[0]];
        if (!channel || ![5,7].includes(row.length) || !row.every(Number.isFinite) || ![60, 3600, 86400].includes(row[1]) || row[3] < 0 || row[4] <= 0) continue;
        series.push({...channel, resolution: row[1], at: row[2], usedMb: row[3], sampledSeconds: row[4], ...(row.length === 7 ? {startedAt:row[5],endedAt:row[6]} : {})});
    }
    // Scope is enforced by the sampler/API and again here for cached data from
    // an earlier all-device session. Unclassified legacy rows are not local.
    const all = report.scope === 'all';
    return {...report, timeline: undefined, accounts: (report.accounts || []).map(account => ({...account, devices: (account.devices || []).filter(device => all || device.local === true)})), history: (report.history || []).filter(row => all || row.local === true), series: series.filter(row => all || row.local === true)};
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
export function buildUsageChart(rows: UsagePoint[], options: UsageChartOptions): { times: number[]; curves: UsageCurve[]; samples: number; totalMb: number; scale: UsageScale } {
    const now = options.now ?? Date.now() / 1000;
    const requestedStart = options.start ?? now-options.range, requestedEnd = Math.min(now,options.end ?? now);
    let scale = usageScales.find(item => item.value === options.scale)!;
    while (scale.seconds && (requestedEnd-requestedStart)/scale.seconds>4500) scale=usageScales[Math.min(usageScales.indexOf(scale)+1,usageScales.length-1)];
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
