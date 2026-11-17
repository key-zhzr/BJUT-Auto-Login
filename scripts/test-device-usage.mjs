import test from 'node:test';
import assert from 'node:assert/strict';
import {usageDelta,usageIdentity,forecastUsage,usageCsv} from '../src/device-usage-model.ts';
import {qualitySummary,diagnosticAdvice} from '../src/network-quality-model.ts';
import {usageTooltipPosition} from '../src/device-usage-model.ts';
import {RouterUsageReader} from '../src/device-usage-model.ts';

test('router history splits oversized days and preserves every minute and its observation interval', async () => {
    const first = Date.parse('2026-10-25T00:00:00+08:00') / 1000;
    const end = first + 3 * 86400 - 1;
    const metadata = {enabled: true, scope: 'local', history: [], accounts: [], series: []};
    const channels = ['lan3', 'lan4', 'lan5'].map(device =>
        ({device, account:'test', interfaceId:device, interfaceName:device, local:true}));
    let requests = 0, splits = 0;
    const request = async query => {
        requests++;
        if (query.part === 'summary') return {...metadata, part:'summary'};
        if (query.end - query.start >= 43200) {
            splits++;
            return {state:'split'};
        }
        const buckets = [];
        for (let at = query.start; at <= query.end; at += 60) {
            channels.forEach((_, i) => buckets.push([i, 60, at, 1, 60, at - 59, at + 1]));
        }
        return {...metadata, part:'series', timeline:{channels, buckets}};
    };
    const reader = new RouterUsageReader(), query = {start:first, end, resolution:60, monthly:false};
    const data = await reader.read(request, query);
    assert.equal(splits, 3);
    assert.equal(data.series.length, 3 * 3 * 1440);
    assert.equal(new Set(data.series.map(row => `${row.at}/${row.interfaceId}`)).size, data.series.length);
    assert.ok(data.series.every(row => row.startedAt === row.at - 59 && row.endedAt === row.at + 1));
    requests = 0;
    assert.deepEqual(await reader.read(request, query), data);
    assert.equal(requests, 1);
});

test('router history rejects missing status, stops superseded reads and keeps legacy responses usable', async () => {
    const reader = new RouterUsageReader();
    const start = Date.parse('2026-10-25T03:15:00+08:00') / 1000;
    const query = {start, end:start + 3600, resolution:60, monthly:true};
    await assert.rejects(reader.read(async () => ({}), query), /未返回/);
    const report = {enabled:true, scope:'local', accounts:[], history:[], series:[]};
    assert.equal(await reader.read(async () => report, query, () => false), null);
    const windows = [];
    const data = await reader.read(async request => {
        if (request.part !== 'summary') windows.push(request);
        return report;
    }, query);
    assert.equal(data.enabled, true);
    assert.deepEqual(windows.map(window => window.resolution), [86400, 3600, 60, 60]);
    assert.equal(windows.at(-2).end + 1, start);
    assert.equal(windows.at(-1).end, query.end);
    assert.ok(windows.every(window => !window.monthly && !window.part));
});
test('chart tooltips leave space around the pointer and stay in the plot',()=>{
 for(const [width,height,bw,bh,x,y] of [[1000,270,220,110,500,200],[1000,270,220,110,980,40],[310,270,280,130,160,230]]){
  const p=usageTooltipPosition(width,height,bw,bh,x,y);
  assert.ok(p.left>=8&&p.left+bw<=width-8&&p.top>=8&&p.top+bh<=height-8);
  assert.ok(p.left>=x+32||p.left+bw<=x-32||p.top>=y+32||p.top+bh<=y-32);
 }
 const narrow=usageTooltipPosition(310,270,280,210,160,135);assert.ok(narrow.top+210<=135-32);
});
test('device samples distinguish accounts, sources, sessions and MAC spelling',()=>{
 const row={mac:'AA-BB-CC-DD-EE-FF',ip:'192.0.2.1',loginAt:'2026-10-25 12:00',usedFlowMb:'100'};
 assert.equal(usageIdentity('local','one',row),usageIdentity('local','one',{...row,mac:'aa:bb:cc:dd:ee:ff'}));
 assert.notEqual(usageIdentity('local','one',row),usageIdentity('router','one',row));
 assert.notEqual(usageIdentity('local','one',row),usageIdentity('local','two',row));
 assert.notEqual(usageIdentity('local','one',row),usageIdentity('local','one',{...row,loginAt:'2026-10-25 13:00'}));
});
test('predictions require enough observed days, ignore missing data and use China billing month',()=>{
 const row={account:'one',device:'a',label:'device',usedMb:100,sampledSeconds:3600};
 const rows=[{...row,day:'2026-10-23'},{...row,day:'2026-10-24'}];
 assert.equal(forecastUsage(rows.slice(0,1),'100 GB',new Date('2026-10-25T12:00:00+08:00')),null);
 const prediction=forecastUsage(rows,'1 GB',new Date('2026-10-25T12:00:00+08:00'));
 assert.equal(prediction.daysLeft,6.5);assert.equal(prediction.projectedMb,15600);assert.equal(prediction.shortfallMb,14576);
 assert.equal(forecastUsage(rows,'无限'),null);
 assert.match(usageCsv([{...rows[0],label:'=malicious()'}]),/"'=malicious\(\)"/);
});
test('quality summaries distinguish checks from packet loss and advice follows evidence',()=>{
 const rows=[{ok:true,ms:50},{ok:true,ms:150},{ok:false,ms:3000}];
 assert.deepEqual(qualitySummary(rows),{count:3,failure:1/3,p50:50,p95:150});
 const report={stale:true,steps:[],overall:'changed'};
 assert.deepEqual(diagnosticAdvice(report),['网络在诊断期间发生变化，请重新检测。']);
 assert.match(diagnosticAdvice({steps:[{id:'campus_route',status:'warning'}],overall:'partial'}).join(' '),/放行校园网段/);
});
test('counter resets, stale samples and missing values never manufacture usage',()=>{
 const previous={at:1000,used:100};
 assert.equal(usageDelta(previous,1060,'112.5'),12.5);
 for(const [at,value] of [[1060,'99'],[1000,'112.5'],[1300,'112.5'],[1060,'--'],[1060,'NaN'],[1060,'-1']])assert.equal(usageDelta(previous,at,value),null);
 assert.equal(usageDelta(undefined,1060,'112.5'),null);
});

test('minute curves sum interfaces once and cumulative mode retains missing intervals',async()=>{
 const {buildUsageChart}=await import('../src/device-usage-model.ts');
 const t=1792886400, row={at:t,resolution:60,account:'one',device:'a',interfaceId:'lan3',interfaceName:'lan3',local:true,usedMb:2,sampledSeconds:60};
 const options={scale:'minute',range:3600,mode:'delta',presentation:'sum',interfaces:null,account:'',now:t+180};
 const rows=[row,{...row,at:t+60,usedMb:3},{...row,device:'b',interfaceId:'lan4',interfaceName:'lan4',usedMb:7},{...row,at:t+180,usedMb:1},{...row,resolution:3600,usedMb:999}];
 const chart=buildUsageChart(rows,options), i=chart.times.indexOf(t);
 assert.deepEqual(chart.curves[0].points.slice(i),[9,3,null,1]);assert.equal(chart.totalMb,13);
 const cumulative=buildUsageChart(rows,{...options,mode:'cumulative'});
 assert.deepEqual(cumulative.curves[0].points.slice(i),[9,12,null,13]);
 const individual=buildUsageChart(rows,{...options,presentation:'interfaces',interfaces:['lan4']});
 assert.equal(individual.curves.length,1);assert.equal(individual.curves[0].id,'lan4');assert.equal(individual.totalMb,7);
 assert.equal(buildUsageChart(rows,{...options,presentation:'interfaces',interfaces:[]}).samples,0);
});
test('monthly rollups use Beijing boundaries and never mix minute/hour/day tiers',async()=>{
 const {buildUsageChart,usageBucket,usageChartCsv}=await import('../src/device-usage-model.ts');
 const october=Date.parse('2026-10-01T00:00:00+08:00')/1000;
 assert.equal(usageBucket(october-1,'month'),Date.parse('2026-09-01T00:00:00+08:00')/1000);
 const row={at:october,resolution:86400,account:'one',device:'a',interfaceId:'a',interfaceName:'=unsafe',local:true,usedMb:12,sampledSeconds:60};
 const chart=buildUsageChart([row,{...row,at:october+86400,usedMb:3},{...row,resolution:60,usedMb:999},{...row,account:'two',usedMb:999}],{scale:'month',range:31536000,mode:'delta',presentation:'interfaces',interfaces:null,account:'one',now:october+86400});
 assert.equal(chart.totalMb,15);assert.equal(chart.curves[0].points.at(-1),15);
 assert.match(usageChartCsv(chart,'delta'),/"'=unsafe"/);assert.match(usageChartCsv(chart,'delta'),/2026-10-01 00:00/);
});
test('packed router samples decode without exposing other devices in local scope',async()=>{
 const {normalizeUsageReport}=await import('../src/device-usage-model.ts');
 const channel={account:'one',device:'x',interfaceId:'lan3',interfaceName:'lan3',local:true};
 const report={enabled:true,scope:'local',accounts:[{account:'one',updatedAt:1,devices:[{local:true,ip:'local'},{local:false,ip:'other'}]}],history:[{local:false},{local:true}],series:[],timeline:{channels:[channel,{...channel,local:false}],buckets:[[0,60,1792886400,2,60],[1,60,1792886400,7,60],[0,1,1792886400,7,60],[55,60,1792886400,7,60]]}};
 const local=normalizeUsageReport(report);assert.equal(local.series.length,1);assert.equal(local.series[0].usedMb,2);assert.equal(local.accounts[0].devices.length,1);assert.equal(local.history.length,1);
 assert.equal(normalizeUsageReport({...report,scope:'all'}).series.length,2);
});
test('real 61-second intervals join across a minute boundary, genuine gaps stay broken',async()=>{
 const {buildUsageChart}=await import('../src/device-usage-model.ts');const t=1792886400;
 const row={at:t,resolution:60,account:'one',device:'a',interfaceId:'lan3',interfaceName:'lan3',local:true,usedMb:2,sampledSeconds:60,startedAt:t-1,endedAt:t+59};
 const chart=buildUsageChart([row,{...row,at:t+120,startedAt:t+59,endedAt:t+120,sampledSeconds:61},{...row,at:t+300,startedAt:t+240,endedAt:t+300}],{scale:'minute',range:600,mode:'delta',presentation:'sum',interfaces:null,account:'',now:t+360});
 const c=chart.curves[0],i=chart.times.indexOf(t);assert.equal(c.points[i+1],null);assert.equal(c.joinPrevious[i+2],true);assert.equal(c.joinPrevious[i+5],false);assert.equal(chart.totalMb,6);
});
test('month cumulative includes prefix before the selected window without mixing rollup tiers',async()=>{
 const {buildUsageChart}=await import('../src/device-usage-model.ts');const t=Date.parse('2026-10-26T13:00:00+08:00')/1000;
 const row={at:t,resolution:60,account:'one',device:'a',interfaceId:'lan3',interfaceName:'lan3',local:true,usedMb:2,sampledSeconds:60};
 const rows=[{...row,at:t-13*3600-86400,resolution:86400,usedMb:100},{...row,at:t-3600,resolution:3600,usedMb:20},{...row,at:t-3600,usedMb:999},{...row,at:t-13*3600,resolution:86400,usedMb:999},row,{...row,at:t+60,usedMb:3}];
 const opts={scale:'minute',range:3600,start:t,end:t+120,mode:'monthly',presentation:'sum',interfaces:null,account:'',now:t+120};
 const c=buildUsageChart(rows,opts);assert.deepEqual(c.curves[0].points.slice(0,2),[122,125]);assert.equal(c.totalMb,5);
 const ordinary=buildUsageChart(rows,{...opts,mode:'cumulative'});assert.deepEqual(ordinary.curves[0].points.slice(0,2),[2,5]);
});
test('arbitrary year-to-minute windows adapt scale and zoom stays anchored',async()=>{
 const {buildUsageChart,usageZoom,usageStorageEstimate,defaultUsageRetention}=await import('../src/device-usage-model.ts');
 const start=Date.parse('2025-01-01T00:00:00+08:00')/1000,end=Date.parse('2026-01-01T00:00:00+08:00')/1000;
 const c=buildUsageChart([],{scale:'minute',range:end-start,start,end,mode:'delta',presentation:'sum',interfaces:null,account:'',now:end});assert.equal(c.scale,'day');assert.ok(c.times.length<5000);
 assert.deepEqual(usageZoom(0,1000,250,.5),[125,625]);assert.equal(usageStorageEstimate(4,defaultUsageRetention),usageStorageEstimate(1,defaultUsageRetention)*4);
});
