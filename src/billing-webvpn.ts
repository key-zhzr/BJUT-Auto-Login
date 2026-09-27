import { invoke } from '@tauri-apps/api/core';
import { CustomSelect } from './custom-select';

interface LoginStatus {stage:'ready'|'sms';challengeId:string|null;message:string}

export function loginBillingWebVpn(accounts: {user:string;label:string}[], preferred: string): Promise<string|null> {
  if(document.getElementById('billing-webvpn-modal'))return Promise.resolve(null);
  return new Promise(resolve=>{
    const previous=document.activeElement as HTMLElement|null;
    const overlay=document.createElement('div'); overlay.className='modal-overlay hidden'; overlay.id='billing-webvpn-modal';
    overlay.innerHTML=`<form class="modal-content glass-card config-transfer-dialog" role="dialog" aria-modal="true" aria-labelledby="webvpn-title">
      <h3 id="webvpn-title">校外访问</h3><p class="transfer-note">登录学校 WebVPN 后继续使用计费功能。</p>
      <div class="form-group"><label>统一认证账号</label><div class="custom-select" id="webvpn-account" aria-label="统一认证账号"><div class="custom-select-trigger"><span>选择账号</span></div><div class="custom-select-options"></div></div></div>
      <div class="form-group webvpn-sms" hidden><label for="webvpn-code">短信验证码</label><input id="webvpn-code" type="password" data-sensitive-password inputmode="numeric" maxlength="6" autocomplete="one-time-code"><button type="button" class="btn btn-secondary btn-sm" data-send>发送验证码</button></div>
      <p class="transfer-note" data-status role="status">使用账号管理中保存的密码。</p><p class="transfer-error" role="alert"></p>
      <div class="modal-actions"><button type="button" class="btn btn-secondary" data-restart hidden>重新登录</button><button type="button" class="btn btn-secondary" data-cancel>取消</button><button type="submit" class="btn btn-primary">继续</button></div>
    </form>`;
    const options=overlay.querySelector('.custom-select-options')!;
    accounts.forEach(account=>{const option=document.createElement('div');option.className='custom-option';option.dataset.value=account.user;option.textContent=account.label;options.append(option);});
    document.body.append(overlay);
    const accountSelect=new CustomSelect('webvpn-account');accountSelect.setValue(accounts.some(a=>a.user===preferred)?preferred:accounts[0]?.user||'');
    const form=overlay.querySelector('form')!;
    const code=overlay.querySelector<HTMLInputElement>('#webvpn-code')!;
    const submit=overlay.querySelector<HTMLButtonElement>('[type="submit"]')!;
    const send=overlay.querySelector<HTMLButtonElement>('[data-send]')!;
    const restart=overlay.querySelector<HTMLButtonElement>('[data-restart]')!;
    const status=overlay.querySelector<HTMLElement>('[data-status]')!;
    const error=overlay.querySelector<HTMLElement>('.transfer-error')!;
    let challengeId='';let busy=false;let closed=false;let resendAt=0;
    const timer=window.setInterval(()=>{const remaining=Math.max(0,Math.ceil((resendAt-Date.now())/1000));send.textContent=remaining?`${remaining} 秒后重发`:'发送验证码';send.disabled=busy||remaining>0;},1000);
    const finish=(user:string|null)=>{
      if(closed)return;closed=true;code.value='';accountSelect.setDisabled(true);clearInterval(timer);
      if(challengeId)void invoke('cancel_billing_webvpn',{challengeId}).catch(()=>{});
      overlay.classList.add('hidden');
      const animations=overlay.getAnimations?.({subtree:true})||[];
      const timeout=new Promise<void>(done=>setTimeout(done,400));
      const done=()=>{overlay.remove();previous?.focus({preventScroll:true});resolve(user);};
      if(window.matchMedia('(prefers-reduced-motion: reduce)').matches)done();
      else void Promise.race([animations.length?Promise.allSettled(animations.map(a=>a.finished)):timeout,timeout]).then(done);
    };
    const accept=(result:LoginStatus)=>{
      if(closed){if(result.challengeId)void invoke('cancel_billing_webvpn',{challengeId:result.challengeId}).catch(()=>{});return;}
      challengeId=result.challengeId||'';status.textContent=result.message;
      if(result.stage==='ready'){finish(accountSelect.value);return;}
      overlay.querySelector<HTMLElement>('.webvpn-sms')!.hidden=false;
      code.required=true;restart.hidden=false;submit.textContent='验证并继续';code.focus();
    };
    const request=async(resend=false)=>{
      if(busy||closed)return;
      if(!accountSelect.value){error.textContent='请先在账号管理中保存账号密码。';return;}
      if(challengeId&&!resend&&!/^\d{6}$/.test(code.value)){error.textContent='请输入 6 位短信验证码。';return;}
      busy=true;submit.disabled=true;send.disabled=true;restart.disabled=true;accountSelect.setDisabled(true);error.textContent='';
      status.textContent=challengeId?(resend?'正在发送验证码…':'正在验证…'):'正在连接学校 WebVPN…';
      try {
        const result=challengeId
          ?await invoke<LoginStatus>('verify_billing_webvpn',{challengeId,token:code.value,resend})
          :await invoke<LoginStatus>('begin_billing_webvpn',{accountUser:accountSelect.value});
        if(resend)resendAt=Date.now()+60000;
        accept(result);
      } catch(reason){if(!closed){error.textContent=String(reason);status.textContent='请检查后重试。';}}
      finally{busy=false;submit.disabled=false;send.disabled=Date.now()<resendAt;restart.disabled=false;accountSelect.setDisabled(Boolean(challengeId));}
    };
    overlay.querySelector('[data-cancel]')!.addEventListener('click',()=>finish(null));
    overlay.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();event.stopPropagation();finish(null);}});
    form.addEventListener('submit',event=>{event.preventDefault();void request();});
    send.addEventListener('click',()=>void request(true));
    restart.addEventListener('click',()=>{
      if(busy)return;
      if(challengeId)void invoke('cancel_billing_webvpn',{challengeId}).catch(()=>{});
      challengeId='';code.value='';code.required=false;overlay.querySelector<HTMLElement>('.webvpn-sms')!.hidden=true;
      restart.hidden=true;accountSelect.setDisabled(false);submit.textContent='继续';error.textContent='';status.textContent='使用账号管理中保存的密码。';
    });
    requestAnimationFrame(()=>requestAnimationFrame(()=>{if(!closed){overlay.classList.remove('hidden');overlay.querySelector<HTMLElement>('.custom-select-trigger')?.focus();}}));
  });
}
