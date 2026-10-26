import test from 'node:test';
import assert from 'node:assert/strict';
import {usageDelta,usageIdentity,forecastUsage,usageCsv} from '../src/device-usage-model.ts';
import {qualitySummary,diagnosticAdvice} from '../src/network-quality-model.ts';
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
