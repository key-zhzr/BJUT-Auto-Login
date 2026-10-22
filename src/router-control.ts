import { invoke } from '@tauri-apps/api/core';
import { CustomSelect } from './custom-select';
import type { AccountView } from './models';
import type { RouterSnapshot } from './router-model';
import { RouterWorkbench } from './router-workbench';
import { setVisible, animateResize } from './layout-motion';

interface ConnectionInfo {address:string;username:string;certificate:string|null;allowHttp:boolean;autoReconnectReady?:boolean;networkName?:string;}
export function setupRouterControl(getAccounts:()=>AccountView[],log:(message:string)=>void,
  confirmCertificate:(text:string,title:string,confirmLabel:string)=>Promise<boolean>):RouterWorkbench {
  const byId=(id:string)=>document.getElementById(id)!;
  const form=byId('router-connect-form') as HTMLFormElement;
  const address=byId('router-address') as HTMLInputElement,username=byId('router-username') as HTMLInputElement,password=byId('router-password') as HTMLInputElement;
  const certificate=byId('router-certificate') as HTMLTextAreaElement,allowHttp=byId('router-allow-http') as HTMLInputElement,remember=byId('router-remember-password') as HTMLInputElement;
  const modal=byId('router-connect-modal'),message=byId('router-message'),connectedPanel=byId('router-connected');
  const connectButton=byId('btn-router-connect') as HTMLButtonElement,syncButton=byId('btn-router-sync') as HTMLButtonElement,localAccounts=byId('router-sync-accounts');
  const uplink=new CustomSelect('router-uplink-select'),account=new CustomSelect('router-account-select');
  let connected=false,busy=false,snapshot:RouterSnapshot|null=null,optionsStamp='',lastPoll=0,generation=0;
  let resumeInfo:ConnectionInfo|null=null,savedInfo:ConnectionInfo|null=null,allowRetry=false,failures=0,nextRetry=0,restoring=false;
  let paused=localStorage.getItem('bjut_router_paused')==='true',returnFocus:HTMLElement|null=null;
  address.value=localStorage.getItem('bjut_router_address') || 'https://192.168.1.1';
  username.value=localStorage.getItem('bjut_router_username') || 'root';certificate.value=localStorage.getItem('bjut_router_certificate') || '';allowHttp.checked=localStorage.getItem('bjut_router_allow_http')==='true';
  const workbench=new RouterWorkbench({connect:openConnection,refresh,login:loginUplink,log,confirm:confirmCertificate});
  function matchesResume() {return !!resumeInfo && resumeInfo.autoReconnectReady!==false && address.value.trim()===resumeInfo.address && username.value.trim()===resumeInfo.username && (certificate.value.trim()||null)===(resumeInfo.certificate||null) && allowHttp.checked===resumeInfo.allowHttp;}
  function updatePasswordField() {password.required=!matchesResume();password.placeholder=matchesResume()?'已保存，可直接连接':'管理密码';}
  for(const input of [address,username,certificate,allowHttp])input.addEventListener('input',updatePasswordField);
  function openConnection(endpoint?:string) {
    if(endpoint)address.value=endpoint;returnFocus=document.activeElement as HTMLElement|null;
    (byId('router-connection-settings') as HTMLDetailsElement).open=true;updatePasswordField();modal.classList.remove('hidden');
    window.setTimeout(()=>password.focus({preventScroll:true}),150);
  }
  function closeConnection() {modal.classList.add('hidden');returnFocus?.focus({preventScroll:true});}
  byId('btn-router-connect-cancel').addEventListener('click',closeConnection);
  modal.addEventListener('keydown',event=>{if(event.key==='Escape'){event.stopPropagation();closeConnection();}});
  function showMessage(text:string,failed=false) {
    message.textContent=text.replace(/ROUTER_(?:CERT_UNTRUSTED|NETWORK_MISMATCH|REPAIR_REQUIRED):/g,'');message.classList.toggle('error',failed);byId('router-connect-message').textContent=message.textContent;
    if(failed){byId('router-discovery-message').textContent=message.textContent;byId('btn-router-auto-connect').hidden=false;}
  }
  function render(data:RouterSnapshot) {
    snapshot=data;connected=true;byId('router-version').textContent=`路由端 v${data.version}`;
    const stamp=JSON.stringify([data.uplinks.map(item=>[item.id,item.network]),data.accounts]);
    if(stamp!==optionsStamp){
      const previous=uplink.value;uplink.setOptions(data.uplinks.length?data.uplinks.map(item=>({value:item.id,text:[item.id,item.network].filter(Boolean).join(' · ')})):[{value:'',text:'请先配置上游'}]);uplink.setDisabled(!data.uplinks.length);uplink.value=data.uplinks.some(item=>item.id===previous)?previous:data.uplinks[0]?.id || '';
      const previousAccount=account.value;account.setOptions([{value:'',text:'自动选择账号'},...data.accounts.filter(item=>item.hasPassword).map(item=>({value:item.id,text:item.user+(item.enabled?'':'（手动）')}))]);account.value=data.accounts.some(item=>item.id===previousAccount)?previousAccount:'';optionsStamp=stamp;
    }
    workbench.render(data);
    setVisible(connectedPanel,true);
  }
  function unavailable(error:unknown) {
    connected=false;snapshot=null;setVisible(connectedPanel,false);workbench.markDisconnected(String(error));setVisible(byId('router-dashboard'),true);
    if(/ROUTER_(NETWORK_MISMATCH|REPAIR_REQUIRED):/.test(String(error))) {nextRetry=Date.now()+30000;showMessage(String(error),true);}
    else if(String(error).includes('ROUTER_CERT_UNTRUSTED:') || String(error).includes('账号或密码')) {allowRetry=false;showMessage(`${String(error)}，请重新连接。`,true);}
    else {nextRetry=Date.now()+Math.min(60000,2000*2**Math.min(failures++,5));showMessage(`连接中断，${Math.round((nextRetry-Date.now())/1000)} 秒后重试。`,true);}
  }
  async function refresh() {
    if(!connected || busy)return;busy=true;const current=generation;lastPoll=Date.now();
    try{const data=await invoke<RouterSnapshot>('get_router_status');if(current===generation && connected)render(data);}
    catch(error){if(current===generation)unavailable(error);}finally{busy=false;}
  }
  async function retry() {
    if(!allowRetry || paused || busy || connected || Date.now()<nextRetry || !modal.classList.contains('hidden'))return;
    busy=true;const current=generation;showMessage('正在重新连接路由器…');
    try{const data=await invoke<RouterSnapshot>('reconnect_router');if(current!==generation)return;failures=0;nextRetry=0;render(data);showMessage('路由器已重新连接');}
    catch(error){if(current===generation)unavailable(error);}finally{busy=false;}
  }
  async function restoreSaved() {
    if(restoring || busy || resumeInfo)return;restoring=true;
    try{
      const profile=await invoke<ConnectionInfo|null>('saved_router_connection');
      if(!profile || !modal.classList.contains('hidden'))return;
      savedInfo=profile;resumeInfo=profile;address.value=profile.address;username.value=profile.username;certificate.value=profile.certificate || '';allowHttp.checked=profile.allowHttp;remember.checked=true;byId('btn-router-forget-password').hidden=false;updatePasswordField();allowRetry=!paused && profile.autoReconnectReady!==false;if(profile.autoReconnectReady===false)showMessage('请手动连接一次，以记住路由器和当前网络。');await retry();
    }catch{/* The existing manual connection stays available. */}finally{restoring=false;}
  }
  form.addEventListener('submit',async event=>{
    event.preventDefault();if(busy)return;
    const current=++generation;busy=true;connectButton.disabled=true;showMessage('正在连接路由器…');
    try{
      const connection={address:address.value.trim(),username:username.value.trim(),password:password.value,certificate:certificate.value.trim() || null,allowHttp:allowHttp.checked,rememberPassword:remember.checked};
      let data:RouterSnapshot;
      if(!connection.password && matchesResume()) {
        data=await invoke<RouterSnapshot>('reconnect_router',{rememberPassword:connection.rememberPassword});
      } else {
        try{data=await invoke<RouterSnapshot>('connect_router',connection);}
        catch(error){
          if(!String(error).includes('ROUTER_CERT_UNTRUSTED:') || connection.certificate)throw error;
          showMessage('正在读取路由器证书…');
          const cert=await invoke<{pem:string;fingerprint:string;address:string}>('probe_router_certificate',{address:connection.address});
          if(!await confirmCertificate(`地址：${cert.address}\nSHA-256 指纹：\n${cert.fingerprint}\n\n请核对这是你的路由器。确认后将记住此证书。`,'确认路由器证书','信任并连接'))throw new Error('未信任路由器证书');
          if(current!==generation)return;if(address.value.trim()!==connection.address)throw new Error('路由器地址已更改，请重新连接');
          certificate.value=cert.pem;connection.certificate=cert.pem;data=await invoke<RouterSnapshot>('connect_router',connection);
        }
      }
      if(current!==generation){await invoke('disconnect_router');return;}
      password.value='';resumeInfo={address:connection.address,username:connection.username,certificate:connection.certificate,allowHttp:connection.allowHttp};savedInfo=remember.checked?resumeInfo:null;
      localStorage.setItem('bjut_router_address',connection.address);localStorage.setItem('bjut_router_username',connection.username);localStorage.setItem('bjut_router_certificate',connection.certificate || '');localStorage.setItem('bjut_router_allow_http',String(connection.allowHttp));localStorage.removeItem('bjut_router_paused');
      paused=false;const saved=await invoke<ConnectionInfo|null>('saved_router_connection');if(saved && remember.checked){savedInfo=saved;resumeInfo=saved;}allowRetry=resumeInfo?.autoReconnectReady!==false;failures=0;nextRetry=0;byId('btn-router-forget-password').hidden=!savedInfo;render(data);closeConnection();showMessage('路由器已连接');log('路由器管理会话已连接');
    }catch(error){if(current===generation)showMessage(String(error),true);}finally{busy=false;connectButton.disabled=false;}
  });
  byId('btn-router-refresh').addEventListener('click',()=>{void refresh();});
  byId('btn-router-disconnect').addEventListener('click',async()=>{
    const current=++generation;paused=true;allowRetry=false;localStorage.setItem('bjut_router_paused','true');connected=false;snapshot=null;setVisible(connectedPanel,false);workbench.disconnect();resumeInfo=savedInfo;updatePasswordField();await invoke('disconnect_router');if(current===generation)showMessage('已停止路由协作，可随时重新连接。');
  });
  byId('btn-router-forget-password').addEventListener('click',async()=>{
    try{await invoke('forget_router_password');savedInfo=null;remember.checked=false;byId('btn-router-forget-password').hidden=true;showMessage('已移除保存的路由密码，当前连接不受影响。');}
    catch(error){showMessage(String(error),true);}
  });
  async function loginUplink(target:string,selectedAccount?:string) {
    if(!connected || busy || !target)return;const current=generation;busy=true;
    try{const result=await invoke<string>('router_login',{uplink:target,account:selectedAccount || account.value || null});if(current===generation && connected){showMessage(result);log(result);lastPoll=0;}}
    catch(error){if(current===generation)showMessage(String(error),true);}finally{busy=false;void refresh();}
  }
  byId('btn-router-login').addEventListener('click',()=>{void loginUplink(uplink.value);});
  byId('btn-router-billing').addEventListener('click',()=>{if(snapshot)byId('btn-open-billing').click();});
  byId('router-sync-details').addEventListener('toggle',event=>{
    if(!(event.target as HTMLDetailsElement).open)return;
    const checked=new Set([...localAccounts.querySelectorAll<HTMLInputElement>('input:checked')].map(i=>i.value));
    animateResize(localAccounts,()=>{localAccounts.replaceChildren(...getAccounts().filter(item=>item.hasPassword).map(item=>{
      const label=document.createElement('label');label.className='router-sync-account';const input=document.createElement('input');input.type='checkbox';input.value=item.user;input.checked=checked.has(item.user);const text=document.createElement('span');text.textContent=item.user+(item.isDisabled?'（自动登录已停用）':'');label.append(input,text);return label;
    }));if(!localAccounts.childElementCount)localAccounts.textContent='请先在账号管理中保存账号密码。';});
  });
  syncButton.addEventListener('click',async()=>{
    if(!connected || busy)return;const current=generation;const users=[...localAccounts.querySelectorAll<HTMLInputElement>('input:checked')].map(i=>i.value);
    if(!users.length){showMessage('请勾选要同步的账号');return;}busy=true;syncButton.disabled=true;
    try{const result=await invoke<string>('sync_router_accounts',{users});if(current===generation){showMessage(result);log(result);const data=await invoke<RouterSnapshot>('get_router_status');if(current===generation && connected){optionsStamp='';render(data);}}}
    catch(error){if(current===generation)showMessage(String(error),true);}finally{busy=false;syncButton.disabled=false;}
  });
  window.setInterval(()=>{
    if(document.hidden)return;
    if(connected && Date.now()-lastPoll>=(snapshot?.uplinks.some(item=>item.checking)?1000:5000))void refresh();
    else if(!connected && allowRetry)void retry();
  },1000);
  const wake=()=>{nextRetry=0;if(connected){lastPoll=0;void refresh();}else if(!resumeInfo)void restoreSaved();else void retry();};
  window.addEventListener('online',wake);window.addEventListener('focus',wake);
  document.addEventListener('visibilitychange',()=>{if(!document.hidden)wake();});
  void restoreSaved();return workbench;
}
