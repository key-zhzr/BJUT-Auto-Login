import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import { setBillingSource, setRouterBillingBridge, setRouterBillingDataListener } from '../src/router-billing-bridge.ts';

// Compile the real controller; Node's strip-only loader cannot lower constructor parameter properties.
const bridgeUrl = new URL('../src/router-billing-bridge.ts', import.meta.url).href;
const source = readFileSync(new URL('../src/router-billing-center.ts', import.meta.url), 'utf8')
  .replace("from './router-billing-bridge.ts'", `from '${bridgeUrl}'`);
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2021, module: ts.ModuleKind.ESNext } }).outputText;
const { RouterBillingCenter } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

function fixture(respond = () => undefined) {
  let snapshot = {
    managementUrl: 'http://192.0.2.1', version: '0.1.6',
    accounts: [{ id: 'one', user: '25000001', hasPassword: 1, enabled: 1 }, { id: 'two', user: '25000002', hasPassword: 1, enabled: 0 }],
    uplinks: [{ id: 'wan1', state: 'online', account: '25000001' }, { id: 'wan2', state: 'online', account: '25000002' }],
  };
  const calls = [];
  const jobs = new Map();
  const published = [];
  const center = new RouterBillingCenter(async (operation, args = {}) => {
    calls.push({ ...args, action: args.operation, operation });
    const supplied = await respond(operation, args, jobs.get(args.ticket));
    if (supplied !== undefined) return supplied;
    if (operation === 'billing_poll') {
      const job = jobs.get(args.ticket);
      const account = job.account === 'two' ? '25000002' : '25000001';
      return { result: { state: 'ready', account, balance: '42', updatedAt: 1792252800, accountStatus: '',
        ...(job.operation === 'billing_details' ? { module: job.module } : {}),
      }, sessions: [] };
    }
    const ticket = String(jobs.size + 1);
    jobs.set(ticket, { ...args, operation });
    return { ticket };
  }, () => snapshot);
  setBillingSource('local');
  setRouterBillingBridge(center);
  setBillingSource('router');
  setRouterBillingDataListener(data => published.push(data));
  return { center, calls, published, setSnapshot(value) { snapshot = value; } };
}

const query = { kind: 'usage', page: 1, pageSize: 100, startDate: '2026-01-01', endDate: '2026-10-01' };
function records(total, page, overrides = {}) {
  return { result: { state: 'ready', account: '25000001', kind: 'usage', total, page,
    columns: ['loginTime', 'flowMb'],
    rows: Array.from({ length: Math.max(0, Math.min(100, total - (page - 1) * 100)) }, (_, i) => [`row-${(page - 1) * 100 + i}`, '0']), ...overrides,
  } };
}

test('router records request the selected page and retain the overall total', async () => {
  const { center, calls } = fixture((operation, _args, job) => operation === 'billing_poll' && job.operation === 'billing_records' ? records(250, Number(job.page), {summary:{'使用流量(MB)':'12.5',PRIVATE:'hidden'}}) : undefined);
  const result = await center.queryRecords({ ...query, page: 2 });
  assert.equal(calls.find(call => call.operation === 'billing_records').page, '2');
  assert.equal(result.page, 2);
  assert.equal(result.table.total, 250);
  assert.equal(result.table.rows[0].loginTime, 'row-100');
  assert.equal(result.table.rows[0].flowMb, '0');
  assert.deepEqual(result.table.summary,{'使用流量(MB)':'12.5'});
});

test('verified overview is visible before optional modules finish and retains App display values', async()=>{
  const pending=deferred();let held=true;
  const {center,published}=fixture((operation,_args,job)=>{
    if(operation==='billing_poll' && job.operation==='billing_start')return {result:{state:'ready',account:'25000001',balance:'3.1218',balanceText:'3.1218 元',remainingFlow:'15250.454',remainingFlowText:'14.89 GB',accountStatus:'正常',usedFlowText:'1024 MB',billingCycle:'2026-10-01 至 2026-10-31'}};
    if(operation==='billing_poll' && job.operation==='billing_details' && held){held=false;return pending.promise;}
  });
  const refresh=center.refresh();await new Promise(resolve=>setImmediate(resolve));
  assert.equal(published.length,1);assert.equal(published[0].overview.balance,'3.1218 元');
  assert.equal(published[0].overview.remainingFlow,'14.89 GB');assert.equal(published[0].overview.usedFlow,'1024 MB');
  pending.resolve({result:{state:'ready',account:'25000001',module:'services'}});await refresh;
  assert.ok(published.length>1);
});

test('full exports join every page and reject incomplete or changing results', async () => {
  const good = fixture((operation, _args, job) => operation === 'billing_poll' && job.operation === 'billing_records' ? records(225, Number(job.page)) : undefined);
  const result = await good.center.queryRecords({ ...query, all: true });
  assert.equal(result.table.total, 225);
  assert.equal(result.table.rows.length, 225);
  assert.equal(result.table.rows.at(-1).loginTime, 'row-224');
  for (const broken of [
    { total: 226 },
    { rows: [] },
    { columns: ['changed', 'flowMb'] },
    { account: '25000002' },
    { kind: 'payments' },
  ]) {
    const { center } = fixture((operation, _args, job) => operation === 'billing_poll' && job.operation === 'billing_records' ? records(225, Number(job.page), job.page === '2' ? broken : {}) : undefined);
    await assert.rejects(center.queryRecords({ ...query, all: true }));
  }
});

test('exports over the limit stop before requesting further pages', async () => {
  const { center, calls } = fixture((operation, _args, job) => operation === 'billing_poll' ? records(50_001, Number(job.page)) : undefined);
  await assert.rejects(center.queryRecords({ ...query, all: true }), /50,000|50000/);
  assert.equal(calls.filter(call => call.operation === 'billing_records').length, 1);
});

test('unknown ticket states and a different billing account fail closed', async () => {
  for (const result of [{ state: 'unexpected', account: '25000001' }, { state: 'ready', account: '25000002' }]) {
    const { center, published } = fixture(operation => operation === 'billing_poll' ? { result } : undefined);
    await assert.rejects(center.refresh());
    assert.equal(published.length, 0);
  }
});

test('disconnect invalidates old requests and permits an immediate new refresh', async () => {
  const pending = deferred();
  let held = true;
  const { center, published } = fixture((operation, _args, job) => {
    if (operation === 'billing_poll' && job.operation === 'billing_start' && held) { held = false; return pending.promise; }
  });
  const old = center.refresh();
  const rejected = assert.rejects(old, /变化|取消|断开/);
  await new Promise(resolve => setImmediate(resolve));
  center.invalidate();
  await center.refresh();
  assert.ok(published.length >= 1);
  assert.deepEqual([...new Set(published.map(data=>data.account))], ['25000001']);
  const completeCount=published.length;
  pending.resolve({ result: { state: 'ready', account: '25000001', balance: '999' } });
  await rejected;
  assert.equal(published.length, completeCount);
  assert.equal(published[0].overview.balance, '42.00 元');
});

test('switching billing source cancels old responses even after switching back', async () => {
  const pending = deferred();
  const { center, published } = fixture(operation => operation === 'billing_poll' ? pending.promise : undefined);
  const old = center.refresh();
  const rejected = assert.rejects(old, /变化|取消|断开/);
  await new Promise(resolve => setImmediate(resolve));
  setBillingSource('local');
  setBillingSource('router');
  pending.resolve({ result: { state: 'ready', account: '25000001' } });
  await rejected;
  assert.equal(published.length, 0);
});

test('selecting another account cannot publish or reuse the previous account response', async () => {
  const pending = deferred();
  const { center, published } = fixture((operation, _args, job) => operation === 'billing_poll' && job.operation === 'billing_start' && job.account === 'one' ? pending.promise : undefined);
  const old = center.refresh();
  const rejected = assert.rejects(old, /变化|取消/);
  await new Promise(resolve => setImmediate(resolve));
  await center.selectAccount('two');
  pending.resolve({ result: { state: 'ready', account: '25000001', balance: '999' } });
  await rejected;
  assert.deepEqual([...new Set(published.map(data => data.account))], ['25000002']);
});

test('action completion without an account field matches the router API contract', async () => {
  const { center, calls } = fixture((operation, _args, job) => operation === 'billing_poll' && job.operation === 'billing_action'
    ? { result: { state: 'ready', message: '无感认证状态已更新' } } : undefined);
  await center.refresh();
  assert.equal(await center.setMauth(false), '无感认证状态已更新');
  assert.equal(calls.filter(call => call.operation === 'billing_action').length, 1);
});

test('session indices take priority over a shared MAC address', async () => {
  const { center, calls } = fixture((operation, _args, job) => operation === 'billing_poll' && job.operation === 'billing_start'
    ? { result: { state: 'ready', account: '25000001' }, sessions: [
      { sessionIndex: '0', ip: '192.0.2.10', mac: 'aabbccddeeff' },
      { sessionIndex: '1', ip: '192.0.2.11', mac: 'aabbccddeeff', loginTime: '1721091600000', useTime: '121', downFlow: '1024', upFlow: '512' },
    ] } : undefined);
  await center.refresh();
  await center.disconnectSession('1', '192.0.2.11', 'aabbccddeeff');
  const action = calls.find(call => call.operation === 'billing_action');
  assert.equal(action.ticket, '1', 'Session actions must retain the overview ticket that owns online.json');
  assert.equal(JSON.parse(action.payload).sessionIndex, '1');
});

test('online session timestamps, durations and traffic use the App units', async () => {
  const { center, published } = fixture((operation, _args, job) => operation === 'billing_poll' && job.operation === 'billing_start'
    ? { result: { state: 'ready', account: '25000001' }, sessions: [
      { sessionIndex: '0', loginTime: '1721091600000', useTime: '121', downFlow: '1024', upFlow: '512' },
    ] } : undefined);
  await center.refresh();
  const session = published[0].overview.onlineSessions[0];
  assert.notEqual(session.loginAt, '1721091600000');
  assert.equal(session.durationMinutes, '2');
  assert.equal(session.usedFlowMb, '1.500');
});

test('manual accounts and current session remain selectable; unknown status is not reported as normal', async () => {
  const { center, published } = fixture();
  assert.deepEqual(center.accounts().map(account => account.value), ['current', 'one', 'two']);
  await center.refresh();
  assert.equal(published[0].overview.status, '--');
  assert.notEqual(published[0].fetchedAt, '1792252800');
});
