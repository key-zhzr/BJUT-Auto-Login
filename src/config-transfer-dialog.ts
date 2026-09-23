export interface ConfigTransferChoice {
  scope: {settings:boolean;accounts:boolean};
  qr: boolean;
  passphrase: string;
}

export function chooseConfigTransfer(mode:'import'|'export'): Promise<ConfigTransferChoice|null> {
  if(document.getElementById('config-transfer-modal'))return Promise.resolve(null);
  return new Promise(resolve => {
    const exporting=mode==='export';
    const previous=document.activeElement as HTMLElement|null;
    const overlay=document.createElement('div'); overlay.className='modal-overlay hidden'; overlay.id='config-transfer-modal';
    overlay.innerHTML=`<form class="modal-content glass-card config-transfer-dialog" role="dialog" aria-modal="true" aria-labelledby="config-transfer-title">
      <h3 id="config-transfer-title">${exporting?'导出':'导入'}配置</h3>
      <fieldset><legend>${exporting?'导出':'导入'}内容</legend><div class="transfer-choices">
        <label><input type="checkbox" name="settings" checked>设置</label>
        <label><input type="checkbox" name="accounts" checked>账号密码</label>
      </div></fieldset>
      <fieldset><legend>${exporting?'导出':'导入'}方式</legend><div class="transfer-choices">
        <label><input type="radio" name="method" value="clipboard" checked>剪贴板</label>
        <label><input type="radio" name="method" value="qr">${exporting?'二维码':'扫描二维码'}</label>
      </div></fieldset>
      <div class="form-group"><label for="transfer-password">备份密码</label><input id="transfer-password" type="password" autocomplete="off" data-sensitive-password required></div>
      ${exporting?'<div class="form-group"><label for="transfer-confirm">确认密码</label><input id="transfer-confirm" type="password" autocomplete="off" data-sensitive-password required></div><p class="transfer-note">至少 3 个字符，建议使用更长的密码。</p>':'<p class="transfer-note">填写导出时设置的备份密码。</p>'}
      <p class="transfer-error" role="alert"></p>
      <div class="modal-actions"><button class="btn btn-secondary" type="button" data-cancel>取消</button><button class="btn btn-primary" type="submit">${exporting?'导出':'导入'}</button></div>
    </form>`;
    const form=overlay.querySelector('form')!;
    const password=overlay.querySelector<HTMLInputElement>('#transfer-password')!;
    const confirm=overlay.querySelector<HTMLInputElement>('#transfer-confirm');
    let closing=false;
    let enterFrame=0;
    const finish=(result:ConfigTransferChoice|null)=> {
      if(closing)return;
      closing=true;
      cancelAnimationFrame(enterFrame);
      password.value=''; if(confirm)confirm.value='';
      overlay.classList.add('hidden');
      const cleanup=()=> { overlay.remove(); previous?.focus({preventScroll:true}); resolve(result); };
      if(window.matchMedia('(prefers-reduced-motion: reduce)').matches) { cleanup(); return; }
      // Keep the element alive until both the backdrop and dialog have left.
      // A timer also handles a WebView pausing animation events in the background.
      const animations=overlay.getAnimations?.({subtree:true}) ?? [];
      const timeout=new Promise<void>(done=>setTimeout(done,400));
      void Promise.race([
        animations.length ? Promise.allSettled(animations.map(animation=>animation.finished)) : timeout,
        timeout,
      ]).then(cleanup);
    };
    overlay.querySelector('[data-cancel]')!.addEventListener('click',()=>finish(null));
    overlay.addEventListener('keydown',event=>{
      if(event.key==='Escape'){event.preventDefault();event.stopPropagation();finish(null);}
      if(event.key==='Tab') {
        const controls=Array.from(form.querySelectorAll<HTMLElement>('input,button'));
        const first=controls[0], last=controls[controls.length-1];
        if(event.shiftKey&&document.activeElement===first){event.preventDefault();last.focus();}
        else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}
      }
    });
    form.addEventListener('submit',event=>{
      event.preventDefault();
      if(closing)return;
      const scope={settings:(form.elements.namedItem('settings') as HTMLInputElement).checked,accounts:(form.elements.namedItem('accounts') as HTMLInputElement).checked};
      const error=!scope.settings&&!scope.accounts ? '请至少选择一项内容。'
        : exporting&&Array.from(password.value.trim()).length<3 ? '备份密码至少需要 3 个字符。'
        : exporting&&confirm?.value!==password.value ? '两次输入的密码不一致。' : '';
      overlay.querySelector('.transfer-error')!.textContent=error; if(error)return;
      finish({scope,qr:(form.elements.namedItem('method') as RadioNodeList).value==='qr',passphrase:password.value});
    });
    document.body.append(overlay);
    // Commit the hidden state first, so dynamically-created modals use the
    // same theme transitions as the existing dialogs.
    enterFrame=requestAnimationFrame(()=>{
      enterFrame=requestAnimationFrame(()=>{
        if(closing)return;
        overlay.classList.remove('hidden'); password.focus({preventScroll:true});
      });
    });
  });
}
