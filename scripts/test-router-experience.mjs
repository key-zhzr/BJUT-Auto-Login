import test from 'node:test';
import assert from 'node:assert/strict';
import {routerStatus,routerAdvice,routerSettingVisible} from '../src/router-model.ts';
test('router setting dependencies match LuCI without clearing hidden values',()=>{
 const values={log_target:'system',pushplus_enabled:'0',billing_background:'0',log_directory:'/mnt/log'};
 const visible=key=>routerSettingVisible(key,k=>values[k]||'');
 for(const key of ['log_directory','log_web','log_max_kb','pushplus_token','_pushplus_test','billing_interval','flow_threshold'])assert.equal(visible(key),false,key);
 values.log_target='file';values.pushplus_enabled='1';values.billing_background='1';
 for(const key of ['log_directory','log_web','log_max_kb','pushplus_token','_pushplus_test','billing_interval','flow_threshold'])assert.equal(visible(key),true,key);
 assert.equal(visible('log_level'),true);assert.equal(values.log_directory,'/mnt/log');
});

test('old or disconnected router data cannot keep a healthy status',()=>{
  const item={id:'wan',state:'online',session:'online',age_seconds:10,valid_for:90};
  assert.equal(routerStatus(item),'互联网已连接');
  assert.equal(routerStatus(item,true),'数据已过期');
  assert.equal(routerStatus({...item,stale:1}),'数据已过期');
  assert.equal(routerStatus(item,false,81),'数据已过期');
});
test('advice separates an authenticated session from internet and IPv6 failures',()=>{
  assert.match(routerAdvice({id:'wan',state:'authenticated_limited',session:'online',internet4:'unreachable',internet6:'unreachable'}),/认证会话有效/);
  assert.match(routerAdvice({id:'wan',state:'online',internet4:'reachable',internet6:'unreachable'}),/IPv6/);
  assert.match(routerAdvice({id:'wan',state:'account_busy'}),/并发上限/);
  assert.match(routerAdvice({id:'wan',state:'online',stale:1}),/过期/);
});
