import { invoke } from '@tauri-apps/api/core';
import { CustomSelect } from './custom-select';
import { animateResize } from './layout-motion';
import { type AccountUsage, type DailyUsage, type UsageReport, type UsageScope } from './device-usage-model';
import { UsageChart, type UsageSelect } from './usage-chart';
import { presentationDialog } from './presentation-dialog';
import './usage-chart.css';
function confirmUsage(enabled:boolean):Promise<boolean>{
    return new Promise(resolve=>{const {form,close}=presentationDialog('usage-confirm-dialog',enabled?'开启流量统计':'暂停流量统计');let accepted=false;
        const text=document.createElement('p');text.textContent=enabled?'按设置中的统计范围记录设备下载量。是否开启？':'暂停采集，已有历史会保留。是否继续？';
        const actions=document.createElement('div');actions.className='modal-actions';
        for(const [label,yes] of [['取消',false],[enabled?'开启统计':'暂停统计',true]] as const){const button=document.createElement('button');button.type='button';button.className=yes?'btn btn-primary':'btn btn-secondary';button.textContent=label;button.onclick=()=>{accepted=yes;close();};actions.append(button);}
        form.addEventListener('presentation-close',()=>resolve(accepted),{once:true});form.append(text,actions);
    });
}
let sequence = 0;
export class DeviceUsageView {
    private chart: UsageChart;
    constructor(root: HTMLElement, source: string, configure: (enabled: boolean, scope: UsageScope) => Promise<void> = async () => {}, windowChanged?:()=>void) {
        this.chart = new UsageChart(root, {
            title: source === 'router' ? '路由器流量趋势' : '流量趋势',
            localLabel: source === 'router' ? '仅路由器上游' : '仅本机设备',
            resize: animateResize,
            configure,
            confirm:confirmUsage,
            windowChanged,
            select: (label, choices, change): UsageSelect => {
                const wrapper = document.createElement('div'); wrapper.className = 'custom-select'; wrapper.id = `usage-select-${++sequence}`; wrapper.setAttribute('aria-label', label);
                wrapper.innerHTML = '<div class="custom-select-trigger"><span></span><span aria-hidden="true">⌄</span></div><div class="custom-select-options"></div>';
                // Attach before the existing menu constructor resolves its ID.
                root.append(wrapper);
                const select = new CustomSelect(wrapper.id); select.setOptions(choices); select.addEventListener('change', event => change(event.target.value));
                return { element: wrapper, get value() {return select.value;}, set value(value: string) {select.value = value;}, setOptions: options => select.setOptions(options) };
            },
            exportCsv: async csv => {
                if (!window.__TAURI__) {
                    const link = document.createElement('a'); link.href = URL.createObjectURL(new Blob([csv], {type: 'text/csv;charset=utf-8'})); link.download = 'device-usage.csv'; link.click();
                    setTimeout(() => URL.revokeObjectURL(link.href), 1000); return;
                }
                const destination = await invoke<string>('export_billing_csv', {kind: 'usage', csv});
                if (window.AndroidBridge?.shareExportFile) {window.AndroidBridge.shareExportFile(destination, '流量统计'); return;}
                const {form, close} = presentationDialog('usage-export-dialog', '已导出');
                const path = document.createElement('p'); path.textContent = destination;
                const done = document.createElement('button'); done.type = 'button'; done.className = 'btn btn-primary'; done.textContent = '完成'; done.onclick = close; form.append(path, done);
            },
        });
    }
    update(data: UsageReport) {this.chart.update(data);}
    query(){return this.chart.query();}
    render(accounts: AccountUsage[], message = '', history: DailyUsage[] = []) {this.chart.update({enabled: false, scope: 'local', accounts, history, series: []}, message);}
    showError(message: string) {this.chart.showError(message);}
}
export function setupDeviceUsage(save: () => Promise<void>) {
    const view = new DeviceUsageView(document.getElementById('device-usage-panel')!, 'local', async (enabled, scope) => {
        const previous = [localStorage.getItem('bjut_traffic_monitor_enabled'), localStorage.getItem('bjut_traffic_monitor_scope')];
        localStorage.setItem('bjut_traffic_monitor_enabled', String(enabled)); localStorage.setItem('bjut_traffic_monitor_scope', scope);
        try {await save(); window.dispatchEvent(new Event('usage-preferences')); await refresh();}
        catch (error) {localStorage.setItem('bjut_traffic_monitor_enabled', previous[0] || 'false'); localStorage.setItem('bjut_traffic_monitor_scope', previous[1] || 'local'); throw error;}
    },()=>{void refresh();});
    let busy = false;
    const refresh = async () => {
        if (busy || document.hidden) return;
        busy = true;
        try {view.update(await invoke<UsageReport>('get_usage_report',{query:view.query()}));}
        catch {view.showError('暂时无法读取流量统计。');}
        finally {busy = false;}
    };
    // The backend owns the minute schedule; UI refreshes read only local data.
    window.setInterval(() => {void refresh();}, 5000);
    window.addEventListener('focus', () => {void refresh();});
    window.addEventListener('usage-preferences', () => {void refresh();});
    void refresh();
}
