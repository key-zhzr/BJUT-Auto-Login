import { presentationDialog } from './presentation-dialog';

export interface EndpointOverrides {
  dorm_gateway: string;
  wifi_gateway: string;
  lgn_gateway: string;
  lgn6_gateway: string;
}

const EMPTY_OVERRIDES = (): EndpointOverrides => ({
  dorm_gateway: '',
  wifi_gateway: '',
  lgn_gateway: '',
  lgn6_gateway: '',
});

let overrides = EMPTY_OVERRIDES();
let certPin = '';

export function getEndpointOverrides(): EndpointOverrides {
  return { ...overrides };
}

export function loadEndpointOverrides(input?: Partial<EndpointOverrides>) {
  overrides = { ...EMPTY_OVERRIDES(), ...input };
}

export function getDormCertSha256(): string {
  return certPin;
}

export function loadDormCertSha256(value?: string) {
  certPin = value ?? '';
}

/**
 * Register the advanced-settings row. `persist` is main.ts's syncConfigToRust;
 * validation stays in Rust so the dialog cannot drift from the save path.
 */
export function setupGatewayOverrides(persist: () => Promise<void>) {
  document.getElementById('btn-gateway-overrides')?.addEventListener('click', () => {
    const draft = getEndpointOverrides();
    const { form, close } = presentationDialog('gateway-overrides-dialog', '校园网网关地址');
    const content = document.createElement('div');
    content.innerHTML = `
      <label class="form-group">宿舍网网关（bjut-sushe）<input name="dorm_gateway" placeholder="10.21.221.98" autocomplete="off" spellcheck="false"></label>
      <label class="form-group">Wi-Fi 网关（bjut_wifi）<input name="wifi_gateway" placeholder="10.21.251.3" autocomplete="off" spellcheck="false"></label>
      <label class="form-group">有线网关（lgn）<input name="lgn_gateway" placeholder="172.30.201.2" autocomplete="off" spellcheck="false"></label>
      <label class="form-group">有线 IPv6 网关（lgn6）<input name="lgn6_gateway" placeholder="2001:da8:216:30c9::2" autocomplete="off" spellcheck="false"></label>
      <p class="transfer-note">只填写 IP 地址：协议、端口与路径由认证协议固定；域名与证书绑定，不接受修改。留空表示使用内置地址。VPN 低兼容模式按域名解析，自定义网关地址不生效。</p>
      <p class="transfer-error" role="alert"></p>
      <div class="modal-actions">
        <button type="button" class="btn btn-secondary" data-reset>恢复内置地址</button>
        <button type="button" class="btn btn-secondary" data-cancel>取消</button>
        <button class="btn btn-primary" type="submit">保存</button>
      </div>`;
    form.append(content);

    const fields: (keyof EndpointOverrides)[] = [
      'dorm_gateway',
      'wifi_gateway',
      'lgn_gateway',
      'lgn6_gateway',
    ];
    const input = (key: keyof EndpointOverrides) =>
      content.querySelector<HTMLInputElement>(`input[name="${key}"]`)!;
    for (const key of fields) input(key).value = draft[key];
    const error = content.querySelector<HTMLElement>('.transfer-error')!;
    content.querySelector('[data-cancel]')?.addEventListener('click', close);
    content.querySelector('[data-reset]')?.addEventListener('click', () => {
      for (const key of fields) input(key).value = '';
      error.textContent = '';
    });

    form.addEventListener('submit', async event => {
      event.preventDefault();
      const previous = { ...overrides };
      overrides = {
        dorm_gateway: input('dorm_gateway').value.trim(),
        wifi_gateway: input('wifi_gateway').value.trim(),
        lgn_gateway: input('lgn_gateway').value.trim(),
        lgn6_gateway: input('lgn6_gateway').value.trim(),
      };
      error.textContent = '';
      try {
        await persist();
        close();
      } catch (failure) {
        overrides = previous;
        error.textContent = failure instanceof Error ? failure.message : String(failure);
      }
    });
  });
}

/**
 * Register the dorm-certificate fingerprint row. The pin is a replacement, not
 * an off switch: the dialog only edits the SHA-256, and Rust still enforces the
 * expiry-only exception plus the SNI allowlist.
 */
export function setupDormCertPin(persist: () => Promise<void>) {
  document.getElementById('btn-dorm-cert-pin')?.addEventListener('click', () => {
    const { form, close } = presentationDialog('dorm-cert-pin-dialog', '宿舍网关证书指纹');
    const content = document.createElement('div');
    content.innerHTML = `
      <label class="form-group">SHA-256 指纹<input name="dorm_cert_sha256" placeholder="64 位十六进制，可带冒号或短横线" autocomplete="off" spellcheck="false"></label>
      <p class="transfer-note">只替换指纹，不能关闭证书校验。当前有效的证书无需替换；若学校换发的证书已过期，它必须仍覆盖 2025-08-20 才能通过内置校验时间点。留空表示使用内置指纹。</p>
      <p class="transfer-error" role="alert"></p>
      <div class="modal-actions">
        <button type="button" class="btn btn-secondary" data-reset>恢复内置指纹</button>
        <button type="button" class="btn btn-secondary" data-cancel>取消</button>
        <button class="btn btn-primary" type="submit">保存</button>
      </div>`;
    form.append(content);
    const input = content.querySelector<HTMLInputElement>('input[name="dorm_cert_sha256"]')!;
    const error = content.querySelector<HTMLElement>('.transfer-error')!;
    input.value = certPin;
    content.querySelector('[data-cancel]')?.addEventListener('click', close);
    content.querySelector('[data-reset]')?.addEventListener('click', () => {
      input.value = '';
      error.textContent = '';
    });
    form.addEventListener('submit', async event => {
      event.preventDefault();
      const previous = certPin;
      certPin = input.value.trim();
      error.textContent = '';
      try {
        await persist();
        close();
      } catch (failure) {
        certPin = previous;
        error.textContent = failure instanceof Error ? failure.message : String(failure);
      }
    });
  });
}
