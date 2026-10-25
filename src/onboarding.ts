import { presentationDialog } from './presentation-dialog';
import { animateResize } from './layout-motion';
const KEY = 'bjut_onboarding_v1';
export function setupOnboarding(options: {
    accounts: () => number;
    navigate: (page: string) => void;
    android: boolean;
    desktop: boolean;
}) {
    let active = false;
    const show = (replay = false) => {
        if (active)
            return;
        active = true;
        let step = replay ? 0 : Math.max(0, Math.min(3, Number(localStorage.getItem(KEY) || 0) || 0));
        const { form, close: closeModal } = presentationDialog('onboarding-dialog', '开始使用 BJUT-AL');
        const body = document.createElement('div');
        form.append(body);
        const close = () => { active = false; closeModal(); };
        form.addEventListener('presentation-close', () => { active = false; });
        const render = () => {
            animateResize(body, () => {
                localStorage.setItem(KEY, String(step));
                const pages = [
                    ['添加校园网账号', '没有网络也可以先添加账号。已有备份时，可到设置中导入。', '添加账号', 'accounts', 'btn-show-add'],
                    ['选择连接方式', '默认自动选择认证网卡。需要固定使用某张网卡时，可在设置中选择；自动登录也由你决定。', '查看网络设置', 'settings', ''],
                    ['保持后台运行', options.android ? '在设置中检查后台与通知权限，避免系统休眠后停止自动登录。' : '关闭窗口后可从托盘再次打开。左键显示主窗口，右键打开快捷菜单。', '查看后台设置', 'settings', ''],
                    ['连接路由器（可选）', '安装了路由端时，可在控制台连接，共同查看上游、账号与计费信息。两端也可以独立使用。', '查看路由协作', 'dashboard', ''],
                ];
                const [title, text, action, page, button] = pages[step];
                body.replaceChildren();
                const progress = document.createElement('p');
                progress.className = 'transfer-note';
                progress.textContent = `${step + 1} / ${pages.length}`;
                const h = document.createElement('h4');
                h.textContent = title;
                const p = document.createElement('p');
                p.textContent = text;
                body.append(progress, h, p);
                if (step === 0 && options.accounts()) {
                    const saved = document.createElement('p');
                    saved.textContent = `已保存 ${options.accounts()} 个账号`;
                    saved.className = 'onboarding-saved';
                    body.append(saved);
                }
                const open = document.createElement('button');
                open.type = 'button';
                open.className = 'btn btn-secondary';
                open.textContent = action;
                open.onclick = () => { close(); options.navigate(page); if (button)
                    document.getElementById(button)?.click(); };
                body.append(open);
                const actions = document.createElement('div');
                actions.className = 'modal-actions';
                for (const [label, run] of [['稍后设置', () => { localStorage.setItem(KEY, 'done'); document.getElementById('onboarding-resume')!.hidden = true; close(); }], ...(step ? [['上一步', () => { step--; render(); }]] : []), [step === 3 ? '完成' : '下一步', () => { if (step === 3) {
                            localStorage.setItem(KEY, 'done');
                            document.getElementById('onboarding-resume')!.hidden = true;
                            close();
                        }
                        else {
                            step++;
                            render();
                        } }]] as [
                    string,
                    () => void
                ][]) {
                    const btn = document.createElement('button');
                    btn.type = 'button';
                    btn.className = label === '下一步' || label === '完成' ? 'btn btn-primary' : 'btn btn-secondary';
                    btn.textContent = label;
                    btn.onclick = run;
                    actions.append(btn);
                }
                body.append(actions);
            });
        };
        render();
        form.addEventListener('submit', event => event.preventDefault());
        form.addEventListener('keydown', event => { if (event.key === 'Escape')
            active = false; });
    };
    document.getElementById('btn-onboarding')?.addEventListener('click', () => show(true));
    document.getElementById('btn-onboarding-continue')?.addEventListener('click', () => show());
    const start = () => {
        const saved = localStorage.getItem(KEY);
        if (saved === 'done')
            return;
        if (!saved && options.accounts()) {
            localStorage.setItem(KEY, 'done');
            return;
        }
        document.getElementById('onboarding-resume')!.hidden = false;
        show();
    };
    return { start };
}
