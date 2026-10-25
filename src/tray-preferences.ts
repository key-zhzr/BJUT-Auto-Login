import { presentationDialog } from './presentation-dialog';
export interface TrayPreferences {
    icon: string;
    customPng: string;
    menuOrder: string[];
    title: string;
    showAccount: boolean;
    showDualStack: boolean;
}
const defaults = (): TrayPreferences => ({ icon: 'default', customPng: '', menuOrder: ['status', 'check', 'login', 'pause', 'accounts'], title: 'none', showAccount: false, showDualStack: true });
let value = defaults();
export function getTrayPreferences(): TrayPreferences { return { ...value, menuOrder: [...value.menuOrder] }; }
export function loadTrayPreferences(input?: Partial<TrayPreferences>) { value = { ...defaults(), ...input }; }
export function setupTrayPreferences(persist: () => Promise<void>) {
    document.getElementById('btn-tray-preferences')?.addEventListener('click', () => {
        const draft = getTrayPreferences();
        const { form, close } = presentationDialog('tray-preferences-dialog', '菜单栏与托盘');
        const content = document.createElement('div');
        content.innerHTML = `<label class="form-group">图标<select name="icon"><option value="default">默认</option><option value="signal">无线信号</option><option value="dot">简洁圆点</option><option value="custom">自定义 PNG</option></select></label><label class="form-group">自定义图标<input type="file" accept="image/png" name="file"><small>不超过 256 × 256，128 KB。</small></label><label class="form-group">macOS 菜单栏文字<select name="title"><option value="none">不显示</option><option value="status">联网状态</option><option value="account">首选账号</option></select></label><div class="transfer-choices"><label><input type="checkbox" name="showAccount">悬停显示首选账号</label><label><input type="checkbox" name="showDualStack">悬停显示双栈状态</label></div><fieldset><legend>菜单项目与顺序</legend><div class="tray-menu-order"></div></fieldset><p class="transfer-note">左键显示主窗口，右键打开菜单。显示主窗口和退出始终保留。</p><p class="transfer-error" role="alert"></p><div class="modal-actions"><button type="button" class="btn btn-secondary" data-cancel>取消</button><button class="btn btn-primary" type="submit">保存</button></div>`;
        form.append(content);
        const controls = form.elements;
        (controls.namedItem('icon') as HTMLSelectElement).value = draft.icon;
        (controls.namedItem('title') as HTMLSelectElement).value = draft.title;
        for (const key of ['showAccount', 'showDualStack'] as const)
            (controls.namedItem(key) as HTMLInputElement).checked = draft[key];
        const labels: Record<string, string> = { status: '联网状态', check: '立即检测网络', login: '立即登录', pause: '暂停自动登录', accounts: '账号列表' };
        const order = [...draft.menuOrder, ...Object.keys(labels).filter(key => !draft.menuOrder.includes(key))];
        const enabled = new Set(draft.menuOrder);
        const render = () => {
            const list = form.querySelector('.tray-menu-order')!;
            list.replaceChildren();
            order.forEach((key, i) => {
                const row = document.createElement('div');
                row.className = 'tray-menu-order-row';
                const label = document.createElement('label'), check = document.createElement('input');
                check.type = 'checkbox';
                check.checked = enabled.has(key);
                check.onchange = () => { if (check.checked)
                    enabled.add(key);
                else
                    enabled.delete(key); };
                label.append(check, labels[key]);
                row.append(label);
                for (const [name, delta] of [['上移', -1], ['下移', 1]] as const) {
                    const btn = document.createElement('button');
                    btn.type = 'button';
                    btn.className = 'btn btn-secondary btn-sm';
                    btn.textContent = name;
                    btn.disabled = i + delta < 0 || i + delta >= order.length;
                    btn.onclick = () => { [order[i], order[i + delta]] = [order[i + delta], order[i]]; render(); };
                    row.append(btn);
                }
                list.append(row);
            });
        };
        render();
        form.querySelector('[data-cancel]')!.addEventListener('click', close);
        form.addEventListener('submit', async (event) => {
            event.preventDefault();
            const error = form.querySelector('.transfer-error')!;
            const submit = form.querySelector<HTMLButtonElement>('[type=submit]')!;
            submit.disabled = true;
            try {
                const file = (controls.namedItem('file') as HTMLInputElement).files?.[0];
                if (file) {
                    if (file.type !== 'image/png' || file.size > 131072)
                        throw new Error('请选择不超过 128 KB 的 PNG。');
                    draft.customPng = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result).split(',')[1]); reader.onerror = () => reject(new Error('无法读取图标')); reader.readAsDataURL(file); });
                }
                draft.icon = (controls.namedItem('icon') as HTMLSelectElement).value;
                if (draft.icon === 'custom' && !draft.customPng)
                    throw new Error('请先选择 PNG 图标。');
                draft.title = (controls.namedItem('title') as HTMLSelectElement).value;
                draft.menuOrder = order.filter(key => enabled.has(key));
                for (const key of ['showAccount', 'showDualStack'] as const)
                    draft[key] = (controls.namedItem(key) as HTMLInputElement).checked;
                const before = value;
                value = draft;
                try {
                    await persist();
                }
                catch (e) {
                    value = before;
                    throw e;
                }
                close();
            }
            catch (e) {
                error.textContent = String(e);
            }
            finally {
                submit.disabled = false;
            }
        });
    });
}
