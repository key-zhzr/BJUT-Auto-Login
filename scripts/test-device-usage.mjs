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
