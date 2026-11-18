import { presentationDialog } from './presentation-dialog';
import { animateResize } from './layout-motion';
const KEY = 'bjut_onboarding_v1';

interface OnboardingStep {
    title: string;
    subtitle: string;
    location: string;
    body: string;
    highlights: string[];
    action: string;
    page: string;
    button: string;
    targetSelector: string;
    fallbackSelector?: string;
    iconSvg: string;
}

const STEP_ICONS = {
    accounts: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>',
    network: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20h.01"/><path d="M2 8.82a15 15 0 0 1 20 0"/><path d="M5 12.859a10 10 0 0 1 14 0"/><path d="M8.5 16.429a5 5 0 0 1 7 0"/></svg>',
    background: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg>',
    router: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect width="20" height="8" x="2" y="14" rx="2"/><path d="M6.01 18H6"/><path d="M10.01 18H10"/><path d="M15 10v4"/><path d="M17.84 7.17a4 4 0 0 0-5.66 0"/><path d="M20.66 4.34a8 8 0 0 0-11.31 0"/></svg>',
};

const ARROW_SVG = '<svg class="onboarding-arrow-caret" viewBox="0 0 20 12" width="20" height="12" aria-hidden="true"><path d="M1 11L10 2L19 11Z" fill="var(--onboarding-surface)" stroke="var(--onboarding-arrow-border)" stroke-width="1.5" stroke-linejoin="round"/></svg><svg class="onboarding-arrow-indicator" viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 13V3"/><path d="M3.5 7.5L8 3l4.5 4.5"/></svg>';
const PIN_SVG = '<svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 14.5s4.5-4.1 4.5-8A4.5 4.5 0 0 0 3.5 6.5c0 3.9 4.5 8 4.5 8Z"/><circle cx="8" cy="6.5" r="1.6"/></svg>';

function clamp(value: number, min: number, max: number): number {
    if (max <= min) return min;
    return Math.min(max, Math.max(min, value));
}

function captureCurrentRoute(): string {
    const activePage = document.querySelector<HTMLElement>('.page.active')?.id || 'dashboard';
    if (activePage === 'settings') {
        const subpage = document.querySelector<HTMLElement>('.settings-subpage[data-settings-subpage]:not([hidden])')?.dataset.settingsSubpage;
        return subpage ? `settings:${subpage}` : 'settings:home';
    }
    return activePage;
}

function isElementRenderable(element: HTMLElement | null): element is HTMLElement {
    if (!element || !element.isConnected || element.hidden) return false;
    const style = getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden') return false;
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
}

function restingRect(element: HTMLElement): DOMRect {
    const raw = element.getBoundingClientRect();
    let dx = 0;
    let dy = 0;
    let current: HTMLElement | null = element;
    while (current && current !== document.body) {
        const transform = getComputedStyle(current).transform;
        if (transform && transform !== 'none') {
            try {
                const matrix = new DOMMatrixReadOnly(transform);
                dx += matrix.m41;
                dy += matrix.m42;
            } catch {
                // Ignore unparseable transform strings.
            }
        }
        current = current.parentElement;
    }
    return new DOMRect(raw.left - dx, raw.top - dy, raw.width, raw.height);
}

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
        const returnRoute = captureCurrentRoute();
        let keepTourDestination = false;
        let step = replay ? 0 : Math.max(0, Math.min(3, Number(localStorage.getItem(KEY) || 0) || 0));

        const { form, close: closeModal } = presentationDialog('onboarding-dialog', '开始使用 BJUT-AL');
        const overlay = form.parentElement!;
        overlay.classList.add('onboarding-overlay');
        form.classList.add('onboarding-dialog');

        const spotlight = document.createElement('div');
        spotlight.className = 'onboarding-spotlight';
        spotlight.setAttribute('aria-hidden', 'true');
        spotlight.hidden = true;
        overlay.prepend(spotlight);

        const heading = form.querySelector<HTMLElement>('h3');
        if (heading) {
            const headerBar = document.createElement('div');
            headerBar.className = 'onboarding-header';
            const titleGroup = document.createElement('div');
            titleGroup.className = 'onboarding-header-copy';
            const subtitle = document.createElement('p');
            subtitle.className = 'onboarding-subtitle';
            subtitle.textContent = '跟随指引前往对应页面完成关键设置';
            const stepBadge = document.createElement('span');
            stepBadge.className = 'onboarding-step-badge';
            heading.replaceWith(headerBar);
            titleGroup.append(heading, subtitle);
            headerBar.append(titleGroup, stepBadge);
        }

        const body = document.createElement('div');
        body.className = 'onboarding-body';
        const arrow = document.createElement('div');
        arrow.className = 'onboarding-arrow';
        arrow.setAttribute('aria-hidden', 'true');
        arrow.innerHTML = ARROW_SVG;
        form.append(body, arrow);

        const pages: OnboardingStep[] = [
            {
                title: '添加校园网账号',
                subtitle: '凭据与优先级',
                location: '账号管理 · 右上角「添加账号」',
                body: '没有网络也可以先添加账号。已有备份时，可到设置中导入。',
                highlights: ['支持多账号优先级排序与断线切换', '支持剪贴板或二维码一键导入配置'],
                action: '添加账号',
                page: 'accounts',
                button: 'btn-show-add',
                targetSelector: '#btn-show-add',
                fallbackSelector: '#accounts .accounts-header',
                iconSvg: STEP_ICONS.accounts,
            },
            {
                title: '选择连接方式',
                subtitle: '网卡与策略',
                location: '设置 → 自动化与网络 · 认证网卡',
                body: '默认自动选择认证网卡。需要固定使用某张网卡时，可在设置中选择；自动登录也由你决定。',
                highlights: ['自动识别校园无线与有线认证方式', '支持按 Wi-Fi SSID 绑定专属网络配置'],
                action: '留在此页调整',
                page: 'settings:automation',
                button: '',
                targetSelector: '.network-adapter-settings .diagnostic-panel-header',
                fallbackSelector: '[data-settings-subpage="automation"] .settings-group',
                iconSvg: STEP_ICONS.network,
            },
            {
                title: '保持后台运行',
                subtitle: options.android ? '系统权限保障' : '系统托盘驻留',
                location: options.android ? '设置 → 网络与安全 · 权限健康中心' : '设置 → 外观与界面 · 菜单栏与托盘',
                body: options.android
                    ? '在设置中检查后台与通知权限，避免系统休眠后停止自动登录。'
                    : '关闭窗口后可从托盘再次打开。左键显示主窗口，右键打开快捷菜单。',
                highlights: options.android
                    ? ['保持常驻通知，实时接收联网与余额提醒', '放行后台省电策略，网络切换时及时重连']
                    : ['定时静默检测网络健康状态与双栈连通性', '可自定义托盘图标样式与快捷菜单排序'],
                action: options.android ? '检查后台权限' : '自定义托盘样式',
                page: options.android ? 'settings:security' : 'settings:appearance',
                button: options.android ? '' : 'btn-tray-preferences',
                targetSelector: options.android ? '#permission-health-group .setting-group-actions' : '#btn-tray-preferences',
                fallbackSelector: options.android ? '#permission-health-group' : '[data-settings-subpage="appearance"] .settings-group:last-of-type',
                iconSvg: STEP_ICONS.background,
            },
            {
                title: '连接路由器（可选）',
                subtitle: '多设备上行协作',
                location: '控制台 · 路由器协作卡片',
                body: '发现已安装插件的路由器后，可在控制台连接，共同查看上游、账号与计费信息。也可以在设置的「路由协作」中手动连接。',
                highlights: ['同步监控多 LAN / WAN 上行口认证状态', '共享路由器侧流量统计与历史趋势图表'],
                action: '留在此页连接',
                page: 'dashboard',
                button: '',
                targetSelector: '#router-dashboard .router-panel-header',
                fallbackSelector: '#router-dashboard',
                iconSvg: STEP_ICONS.router,
            },
        ];

        const needsRouterSettings = (current: OnboardingStep) =>
            current.page === 'dashboard' && !isElementRenderable(document.getElementById('router-dashboard'));

        const resolveTarget = (current: OnboardingStep): HTMLElement | null => {
            const primary = document.querySelector<HTMLElement>(current.targetSelector);
            if (isElementRenderable(primary)) return primary;
            if (current.fallbackSelector) {
                const fallback = document.querySelector<HTMLElement>(current.fallbackSelector);
                if (isElementRenderable(fallback)) return fallback;
            }
            return null;
        };

        const safeViewport = () => {
            const vw = window.innerWidth;
            const vh = window.innerHeight;
            const isDesktop = document.body.classList.contains('is-desktop');
            const isMacos = document.body.classList.contains('is-macos');
            const titlebar = (isDesktop && !isMacos) ? document.querySelector<HTMLElement>('.titlebar') : null;
            const titlebarH = titlebar ? titlebar.getBoundingClientRect().height : 0;
            const nav = document.getElementById('nav');
            const navRect = nav?.getBoundingClientRect();
            const navVisible = Boolean(nav && navRect && navRect.width > 0 && navRect.height > 0 && getComputedStyle(nav).visibility !== 'hidden');
            const hasBottomNav = Boolean(!isDesktop && navVisible && navRect!.top > vh * 0.55);
            const hasSideNav = Boolean(isDesktop && navVisible && navRect!.left <= 4);
            return {
                vw,
                vh,
                safeTop: Math.max(12, Math.ceil(titlebarH + 10)),
                safeBottom: hasBottomNav ? Math.floor(navRect!.top - 10) : (vh - 12),
                safeLeft: hasSideNav ? Math.ceil(navRect!.right + 12) : 12,
                safeRight: vw - 12,
            };
        };

        const scrollTargetIntoUpperZone = (targetEl: HTMLElement) => {
            const mainEl = targetEl.closest<HTMLElement>('#main-content');
            if (!mainEl) return;
            const { safeTop, safeBottom } = safeViewport();
            const rect = restingRect(targetEl);
            const usableHeight = Math.max(120, safeBottom - safeTop);
            const desiredTop = safeTop + Math.min(84, Math.round(usableHeight * 0.14));
            if (rect.top < safeTop + 6 || rect.bottom > safeBottom - Math.min(260, usableHeight * 0.52)) {
                const maxScroll = Math.max(0, mainEl.scrollHeight - mainEl.clientHeight);
                const nextScroll = clamp(mainEl.scrollTop + (rect.top - desiredTop), 0, maxScroll);
                mainEl.scrollTo({ top: nextScroll, behavior: 'auto' });
                mainEl.classList.toggle('is-scrolled', mainEl.scrollTop > 8);
            }
        };

        const positionCoachmark = () => {
            if (!active || !overlay.isConnected) return;
            const current = pages[step];
            const target = resolveTarget(current);
            const bounds = safeViewport();
            const cardW = Math.max(240, form.offsetWidth || Math.min(392, bounds.vw - 24));
            const cardH = Math.max(180, form.offsetHeight || 310);

            if (!target) {
                spotlight.hidden = true;
                form.dataset.placement = 'center';
                const left = clamp((bounds.safeLeft + bounds.safeRight - cardW) / 2, 12, bounds.vw - cardW - 12);
                const top = clamp((bounds.safeTop + bounds.safeBottom - cardH) / 2, bounds.safeTop, bounds.safeBottom - cardH);
                form.style.left = `${Math.round(left)}px`;
                form.style.top = `${Math.round(top)}px`;
                overlay.dataset.positioned = 'true';
                return;
            }

            const rect = restingRect(target);
            const padX = 7;
            const padY = 6;
            const spotLeft = clamp(rect.left - padX, 4, bounds.vw - 28);
            const spotTop = clamp(rect.top - padY, 4, bounds.vh - 28);
            const spotRight = clamp(rect.right + padX, spotLeft + 24, bounds.vw - 4);
            const spotBottom = clamp(rect.bottom + padY, spotTop + 24, bounds.vh - 4);
            const spotW = spotRight - spotLeft;
            const spotH = spotBottom - spotTop;

            spotlight.hidden = false;
            spotlight.style.left = `${Math.round(spotLeft)}px`;
            spotlight.style.top = `${Math.round(spotTop)}px`;
            spotlight.style.width = `${Math.round(spotW)}px`;
            spotlight.style.height = `${Math.round(spotH)}px`;
            const targetRadius = parseFloat(getComputedStyle(target).borderRadius || '0');
            spotlight.style.borderRadius = `${Math.max(10, Math.min(20, Math.round((Number.isFinite(targetRadius) ? targetRadius : 8) + 4)))}px`;

            const arrowGap = 22;
            const edgePad = 14;
            const targetCenterX = (spotLeft + spotRight) / 2;
            const targetCenterY = (spotTop + spotBottom) / 2;

            const spaceBelow = bounds.safeBottom - (spotBottom + arrowGap);
            const spaceAbove = (spotTop - arrowGap) - bounds.safeTop;
            const spaceRight = bounds.safeRight - (spotRight + arrowGap);
            const spaceLeft = (spotLeft - arrowGap) - bounds.safeLeft;

            let placement: 'bottom' | 'top' | 'right' | 'left' = 'bottom';
            if (spaceBelow >= cardH) {
                placement = 'bottom';
            } else if (spaceAbove >= cardH) {
                placement = 'top';
            } else if (bounds.vw >= 720 && spaceRight >= cardW) {
                placement = 'right';
            } else if (bounds.vw >= 720 && spaceLeft >= cardW) {
                placement = 'left';
            } else {
                placement = spaceBelow >= spaceAbove ? 'bottom' : 'top';
            }

            let cardLeft = bounds.safeLeft;
            let cardTop = bounds.safeTop;
            const minHorizontal = bounds.safeRight - bounds.safeLeft >= cardW ? bounds.safeLeft : 12;
            const maxHorizontal = bounds.vw - cardW - 12;

            if (placement === 'bottom') {
                cardLeft = clamp(targetCenterX - cardW / 2, minHorizontal, maxHorizontal);
                cardTop = clamp(spotBottom + arrowGap, bounds.safeTop + edgePad, bounds.safeBottom - cardH);
            } else if (placement === 'top') {
                cardLeft = clamp(targetCenterX - cardW / 2, minHorizontal, maxHorizontal);
                cardTop = clamp(spotTop - arrowGap - cardH, bounds.safeTop, bounds.safeBottom - cardH - edgePad);
            } else if (placement === 'right') {
                cardLeft = clamp(spotRight + arrowGap, minHorizontal, maxHorizontal - edgePad);
                cardTop = clamp(targetCenterY - cardH / 2, bounds.safeTop, bounds.safeBottom - cardH);
            } else {
                cardLeft = clamp(spotLeft - arrowGap - cardW, minHorizontal + edgePad, maxHorizontal);
                cardTop = clamp(targetCenterY - cardH / 2, bounds.safeTop, bounds.safeBottom - cardH);
            }

            const arrowX = clamp(targetCenterX - cardLeft, 28, cardW - 28);
            const arrowY = clamp(targetCenterY - cardTop, 28, cardH - 28);
            form.dataset.placement = placement;
            form.style.setProperty('--onboarding-arrow-x', `${Math.round(arrowX)}px`);
            form.style.setProperty('--onboarding-arrow-y', `${Math.round(arrowY)}px`);
            form.style.left = `${Math.round(cardLeft)}px`;
            form.style.top = `${Math.round(cardTop)}px`;
            overlay.dataset.positioned = 'true';
        };

        let settleRaf = 0;
        let settleStop = 0;
        const stopTracking = () => {
            if (settleRaf) {
                cancelAnimationFrame(settleRaf);
                settleRaf = 0;
            }
            if (settleStop) {
                window.clearTimeout(settleStop);
                settleStop = 0;
            }
        };

        const trackStepTarget = () => {
            stopTracking();
            const current = pages[step];
            const firstTarget = resolveTarget(current);
            if (firstTarget) scrollTargetIntoUpperZone(firstTarget);
            positionCoachmark();
            let lastSig = '';
            let stableCount = 0;
            const tick = () => {
                if (!active || !overlay.isConnected) return;
                const target = resolveTarget(pages[step]);
                positionCoachmark();
                const sig = target ? (() => {
                    const r = target.getBoundingClientRect();
                    return `${Math.round(r.left)},${Math.round(r.top)},${Math.round(r.width)},${Math.round(r.height)},${form.offsetHeight}`;
                })() : 'none';
                stableCount = sig === lastSig ? stableCount + 1 : 0;
                lastSig = sig;
                if (stableCount < 4) {
                    settleRaf = requestAnimationFrame(tick);
                } else {
                    settleRaf = 0;
                }
            };
            settleRaf = requestAnimationFrame(tick);
            settleStop = window.setTimeout(stopTracking, 460);
        };

        const onViewportChange = () => positionCoachmark();
        const mainContent = document.getElementById('main-content');
        window.addEventListener('resize', onViewportChange);
        mainContent?.addEventListener('scroll', onViewportChange, { passive: true });
        const observer = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(onViewportChange) : null;
        observer?.observe(form);

        let cleanedUp = false;
        const cleanup = () => {
            if (cleanedUp) return;
            cleanedUp = true;
            active = false;
            stopTracking();
            window.removeEventListener('resize', onViewportChange);
            mainContent?.removeEventListener('scroll', onViewportChange);
            observer?.disconnect();
            if (!keepTourDestination && returnRoute) {
                options.navigate(returnRoute);
            }
        };

        const close = () => {
            active = false;
            closeModal();
        };

        form.addEventListener('presentation-close', cleanup);

        const triggerStepAction = (current: OnboardingStep) => {
            keepTourDestination = true;
            close();
            if (current.button) {
                document.getElementById(current.button)?.click();
            } else if (needsRouterSettings(current)) {
                options.navigate('settings:router');
            }
        };

        spotlight.addEventListener('click', () => {
            triggerStepAction(pages[step]);
        });

        const paint = () => {
            localStorage.setItem(KEY, String(step));
            const current = pages[step];
            options.navigate(current.page);

            const badge = form.querySelector<HTMLElement>('.onboarding-step-badge');
            if (badge) badge.textContent = `步骤 ${step + 1} / ${pages.length}`;
            body.replaceChildren();

            const stepsTrack = document.createElement('div');
            stepsTrack.className = 'onboarding-steps';
            stepsTrack.setAttribute('aria-label', `引导进度 ${step + 1} / ${pages.length}`);
            pages.forEach((item, index) => {
                const pill = document.createElement('button');
                pill.type = 'button';
                pill.className = 'onboarding-step-pill';
                if (index === step) pill.classList.add('is-active');
                if (index < step) pill.classList.add('is-completed');
                pill.title = `${index + 1}. ${item.title}`;
                pill.setAttribute('aria-label', `第 ${index + 1} 步：${item.title}`);
                pill.onclick = () => {
                    if (index !== step) {
                        step = index;
                        render(true);
                    }
                };
                stepsTrack.append(pill);
            });

            const card = document.createElement('div');
            card.className = 'onboarding-step-card';

            const locationChip = document.createElement('div');
            locationChip.className = 'onboarding-location';
            locationChip.innerHTML = `${PIN_SVG}<span>${current.location}</span>`;

            const head = document.createElement('div');
            head.className = 'onboarding-step-head';
            const iconWrap = document.createElement('div');
            iconWrap.className = 'onboarding-step-icon';
            iconWrap.innerHTML = current.iconSvg;
            const headCopy = document.createElement('div');
            headCopy.className = 'onboarding-step-copy';
            const eyebrow = document.createElement('span');
            eyebrow.className = 'onboarding-step-eyebrow';
            eyebrow.textContent = current.subtitle;
            const h = document.createElement('h4');
            h.textContent = current.title;
            headCopy.append(eyebrow, h);
            head.append(iconWrap, headCopy);

            const p = document.createElement('p');
            p.className = 'onboarding-step-desc';
            p.textContent = current.body;

            const highlightsList = document.createElement('ul');
            highlightsList.className = 'onboarding-highlights';
            for (const point of current.highlights) {
                const li = document.createElement('li');
                li.textContent = point;
                highlightsList.append(li);
            }

            card.append(locationChip, head, p, highlightsList);

            if (step === 0 && options.accounts()) {
                const saved = document.createElement('div');
                saved.className = 'onboarding-saved';
                saved.innerHTML = `<span class="onboarding-saved-dot" aria-hidden="true"></span><span>已保存 ${options.accounts()} 个账号</span>`;
                card.append(saved);
            }

            const actionRow = document.createElement('div');
            actionRow.className = 'onboarding-step-action';
            const open = document.createElement('button');
            open.type = 'button';
            open.className = 'btn btn-secondary btn-sm';
            open.textContent = needsRouterSettings(current) ? '打开路由协作设置' : current.action;
            open.onclick = () => triggerStepAction(current);
            actionRow.append(open);
            card.append(actionRow);

            const footer = document.createElement('div');
            footer.className = 'onboarding-footer';
            const finishGuide = (stayOnStepPage = false) => {
                keepTourDestination = stayOnStepPage;
                localStorage.setItem(KEY, 'done');
                document.getElementById('onboarding-resume')!.hidden = true;
                close();
            };
            const laterBtn = document.createElement('button');
            laterBtn.type = 'button';
            laterBtn.className = 'btn btn-secondary onboarding-later-btn';
            laterBtn.textContent = '稍后设置';
            laterBtn.onclick = () => finishGuide(false);

            const navBtns = document.createElement('div');
            navBtns.className = 'onboarding-footer-nav';
            if (step > 0) {
                const prevBtn = document.createElement('button');
                prevBtn.type = 'button';
                prevBtn.className = 'btn btn-secondary';
                prevBtn.textContent = '上一步';
                prevBtn.onclick = () => {
                    step--;
                    render(true);
                };
                navBtns.append(prevBtn);
            }
            const nextBtn = document.createElement('button');
            nextBtn.type = 'button';
            nextBtn.className = 'btn btn-primary';
            nextBtn.textContent = step === pages.length - 1 ? '完成' : '下一步';
            nextBtn.onclick = () => {
                if (step === pages.length - 1) {
                    finishGuide(true);
                } else {
                    step++;
                    render(true);
                }
            };
            navBtns.append(nextBtn);
            footer.append(laterBtn, navBtns);

            body.append(stepsTrack, card, footer);
            trackStepTarget();
        };

        const render = (animate = false) => {
            if (animate) {
                animateResize(body, paint);
            } else {
                paint();
            }
        };
        render(false);
        form.addEventListener('submit', event => event.preventDefault());
        form.addEventListener('keydown', event => { if (event.key === 'Escape') active = false; });
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
