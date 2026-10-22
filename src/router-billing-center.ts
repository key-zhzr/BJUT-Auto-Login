import type {
  BillingActionRequest,
  BillingActionResult,
  BillingCenterData,
  BillingLoginRecord,
  BillingOnlineSession,
  BillingPackageOption,
  BillingRecordQuery,
  BillingRecordResult,
  BillingServiceState,
  BillingTable,
} from './models';
import type { RouterSnapshot } from './router-model';
import {
  getBillingSource,
  publishRouterBillingData,
  type RouterBillingBridge,
} from './router-billing-bridge.ts';

type Data = Record<string, any>;
interface RequestScope {
  generation: number;
  router: string;
  account: string;
  uplink: string;
  expectedAccount: string;
}

const EMPTY_TABLE = (): BillingTable => ({ total: 0, rows: [], summary: {} });
const BILLING_PAGE_SIZE = 100;
const MAX_EXPORT_ROWS = 50_000;
const SUMMARY_LABELS = new Set(['计费金额(元)','记录数','使用流量(MB)','使用时长(分钟)','基本月租(元)','时长/流量计费(元)','金额(元)']);

// Promise.withResolvers is newer than the supported Android WebView baseline.
function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function tableRows(columns: string[] | undefined, rows: unknown[][] | undefined, total?: number): BillingTable {
  const keys = Array.isArray(columns) ? columns.map(String) : [];
  const values = Array.isArray(rows) ? rows : [];
  return {
    total: total ?? values.length,
    rows: values.map(row => Object.fromEntries(keys.map((key, index) => [key,
      ['loginTime', 'logoutTime', 'lastLoginAt'].includes(key) ? timestamp(row[index]) : String(row[index] ?? ''),
    ]))),
    summary: {},
  };
}

function rowValue(row: Record<string, string>, keys: string[]): string {
  for (const key of keys) {
    const value = row[key];
    if (value != null && String(value).trim() !== '') return String(value);
  }
  return '--';
}

function money(value: unknown): string {
  const text = String(value ?? '').trim();
  if (!text) return '--';
  if (/[^\d.\-]/.test(text)) return text;
  const number = Number(text);
  return Number.isFinite(number) ? `${number.toFixed(2)} 元` : text;
}

function flow(value: unknown): string {
  const text = String(value ?? '').trim();
  if (!text) return '--';
  if (/[a-z]/i.test(text)) return text;
  const number = Number(text);
  return Number.isFinite(number) ? `${(number / 1024).toFixed(2)} GB` : text;
}

function duration(value: unknown): string {
  const text = String(value ?? '').trim();
  if (!text) return '--';
  const number = Number(text);
  return Number.isFinite(number) ? String(Math.round(number)) : text;
}

function dateValue(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function timestamp(value: unknown): string {
  const text = String(value ?? '').trim();
  if (!/^\d{10}(\d{3})?$/.test(text)) return text || '--';
  const date = new Date(Number(text) * (text.length === 10 ? 1000 : 1));
  return Number.isFinite(date.getTime()) ? date.toLocaleString('zh-CN', { hour12: false }) : text;
}

export class RouterBillingCenter implements RouterBillingBridge {
  private account = '';
  private uplink = '';
  private generation = 0;
  private refreshRequest: Promise<void> | null = null;
  private cancelCaptcha: (() => void) | null = null;
  private actionTicket = '';
  private overview: Data | null = null;
  private sessions: Record<string, string>[] = [];
  private services: Data | null = null;
  private devices: Data | null = null;
  private security: Data | null = null;
  private history: Data | null = null;
  private moduleWarnings: string[] = [];

  constructor(
    private request: <T>(operation: string, args?: Record<string, unknown>) => Promise<T>,
    private snapshot: () => RouterSnapshot | null,
  ) {}

  accounts(): { value: string; text: string }[] {
    const accounts = (this.snapshot()?.accounts || []).filter(account => account.hasPassword);
    return [{ value: 'current', text: '当前认证账号' },
      ...accounts.map(account => ({ value: account.id, text: `${account.user}${account.enabled ? '' : '（手动）'}` }))];
  }

  currentAccount(): string {
    this.ensureAccount();
    return this.account;
  }

  async selectAccount(value: string): Promise<void> {
    if (!this.accounts().some(account => account.value === value)) throw new Error('账号已变化，请刷新后重试。');
    this.invalidate();
    this.account = value;
    this.chooseUplink();
    this.resetData();
    await this.refresh();
  }

  async refresh(): Promise<void> {
    if (this.refreshRequest) return this.refreshRequest;
    const scope = this.captureScope(false);
    this.resetData();
    const running = this.refreshScoped(scope).finally(() => {
      if (this.generation === scope.generation) this.refreshRequest = null;
    });
    this.refreshRequest = running;
    return running;
  }

  invalidate(): void {
    this.generation++;
    this.refreshRequest = null;
    this.cancelCaptcha?.();
    this.resetData();
  }

  private async refreshScoped(scope: RequestScope): Promise<void> {
    if(this.snapshot()?.capabilities?.includes('billing_background')) {
      try {
        const cached=await this.scopedRequest<Data>(scope,'billing_cached',{});
        const expected=scope.expectedAccount || this.snapshot()?.uplinks.find(item=>item.id===scope.uplink)?.account;
        const item=cached.items?.find((item:Data)=>item.account===expected && item.state==='ready');
        if(item){this.overview={...item,_cached:true};if(getBillingSource()==='router')publishRouterBillingData(this.compose());}
      } catch { /* The foreground query remains available without a cache. */ }
    }
    const start = await this.scopedRequest<Data>(scope, 'billing_start', {
      uplink: scope.uplink,
      account: scope.account,
    });
    if (!start.ticket) throw new Error(String(start.message || '无法开始计费查询'));
    const overview = await this.waitForTicket(String(start.ticket), scope);
    this.overview = overview.result || overview;
    if (!this.overview?.account) throw new Error('未取得计费账号，请刷新后重试。');
    scope.expectedAccount = String(this.overview.account);
    this.sessions = Array.isArray((overview as Data).sessions) ? (overview as Data).sessions : [];
    this.actionTicket = String(start.ticket);
    if(getBillingSource()==='router')publishRouterBillingData(this.compose());
    for (const module of ['services', 'devices', 'security', 'history']) {
      try {
        const result = await this.loadModule(module, scope);
        if (module === 'services') {
          this.services = result;
        } else if (module === 'devices') this.devices = result;
        else if (module === 'security') this.security = result;
        else this.history = result;
      } catch (error) {
        this.assertScope(scope);
        this.moduleWarnings.push(`${module === 'services' ? '账户服务' : module === 'devices' ? '设备' : module === 'security' ? '账号安全' : '近期记录'}：${String(error)}`);
      }
      if(getBillingSource()==='router')publishRouterBillingData(this.compose());
    }
    this.assertScope(scope);
    if (getBillingSource() === 'router') publishRouterBillingData(this.compose());
  }

  async queryRecords(query: BillingRecordQuery): Promise<BillingRecordResult> {
    const scope = this.captureScope();
    if (!Number.isSafeInteger(query.page) || query.page < 1 || query.pageSize !== BILLING_PAGE_SIZE) {
      throw new Error('请选择有效的账单页码，路由器每页显示 100 条。');
    }
    const fetchPage = async (page: number): Promise<{ rows: unknown[][]; columns: string[]; total: number; page: number; summary: Record<string,string> }> => {
      const start = await this.scopedRequest<Data>(scope, 'billing_records', {
        uplink: scope.uplink,
        account: scope.account,
        kind: query.kind,
        start_date: query.startDate || '',
        end_date: query.endDate || '',
        year: query.year || '',
        page: String(page),
      });
      if (!start.ticket) throw new Error(String(start.message || '查询未开始'));
      const response = await this.waitForTicket(String(start.ticket), scope);
      const result = response.result || response;
      const rows: unknown[][] = result.rows;
      const columns: string[] = result.columns;
      const total = Number(result.total);
      if (result.kind !== query.kind || Number(result.page) !== page
        || !Number.isSafeInteger(total) || total < 0
        || !Array.isArray(columns) || !columns.every(column => typeof column === 'string')
        || !Array.isArray(rows) || rows.some(row => !Array.isArray(row) || row.length !== columns.length)
        || rows.length !== Math.max(0, Math.min(BILLING_PAGE_SIZE, total - (page - 1) * BILLING_PAGE_SIZE))) {
        throw new Error('计费记录不完整或已变化，请重新查询。');
      }
      return {
        rows,
        columns,
        total,
        page,
        summary: result.summary && typeof result.summary==='object' && !Array.isArray(result.summary)
          ? Object.fromEntries(Object.entries(result.summary).filter(([key,value])=>SUMMARY_LABELS.has(key) && (typeof value==='string' || typeof value==='number')).map(([key,value])=>[key,String(value).slice(0,128)])) : {},
      };
    };

    const first = await fetchPage(query.all ? 1 : query.page);
    let rows = first.rows;
    const columns = first.columns;
    if (query.all && first.total > MAX_EXPORT_ROWS) throw new Error('完整导出最多支持 50,000 条，请缩小查询范围。');
    if (query.all && first.total > BILLING_PAGE_SIZE) {
      const pages = Math.ceil(first.total / BILLING_PAGE_SIZE);
      for (let page = 2; page <= pages; page++) {
        const next = await fetchPage(page);
        if (next.total !== first.total || JSON.stringify(next.columns) !== JSON.stringify(columns)) {
          throw new Error('查询期间计费记录已变化，请重新导出。');
        }
        rows = rows.concat(next.rows);
      }
    }
    return {
      kind: query.kind,
      page: query.all ? 1 : first.page,
      pageSize: BILLING_PAGE_SIZE,
      startDate: query.startDate,
      endDate: query.endDate,
      year: query.year,
      all: Boolean(query.all),
      table: { ...tableRows(columns, rows, first.total), summary: first.summary },
    };
  }

  async performAction(request: BillingActionRequest): Promise<BillingActionResult> {
    let payload: Record<string, string> = {};
    if (request.action === 'schedulePackage') payload = { packageId: request.packageId || '' };
    else if (request.action === 'setConsumeLimit') payload = { consumeLimit: request.consumeLimit || '' };
    else if (request.action === 'bindMac' || request.action === 'unbindMac') payload = { mac: request.mac || '' };
    else if (request.action === 'changePassword') payload = { oldPassword: request.oldPassword || '', newPassword: request.newPassword || '' };
    else if (request.action === 'updateQuestions') {
      const questions = request.questions || [];
      payload = {
        oldPassword: request.oldPassword || '',
        question1: questions[0]?.questionId || '',
        question2: questions[1]?.questionId || '',
        question3: questions[2]?.questionId || '',
        answer1: questions[0]?.answer || '',
        answer2: questions[1]?.answer || '',
        answer3: questions[2]?.answer || '',
      };
    } else if (!['stopNow', 'reopenNow', 'cancelPackage'].includes(request.action)) {
      throw new Error('路由器不支持该计费操作');
    }
    const result = await this.runAction(request.action, payload);
    return {
      message: String(result.message || '操作已完成'),
      passwordChanged: request.action === 'changePassword',
    };
  }

  async disconnectSession(sessionId: string, ip: string, mac: string): Promise<string> {
    const exact = this.sessions.find(item => String(item.sessionIndex ?? item.sessionId ?? '') === sessionId);
    const matches = this.sessions.filter(item => ip ? item.ip === ip : mac && item.mac === mac);
    const session = exact || (matches.length === 1 ? matches[0] : undefined);
    const index = session?.sessionIndex ?? session?.sessionId;
    if (index == null || String(index) === '') throw new Error('在线会话信息不完整，请刷新后重试。');
    const result = await this.runAction('disconnectSession', { sessionIndex: String(index) });
    return String(result.message || '会话已注销');
  }

  async setMauth(enabled: boolean): Promise<string> {
    const result = await this.runAction('setMauth', { enabled: enabled ? '1' : '0' });
    return String(result.message || '无感认证状态已更新');
  }

  private ensureAccount(): void {
    const accounts = this.accounts();
    if (!accounts.some(account => account.value === this.account)) {
      const snapshot = this.snapshot();
      const configured = snapshot?.accounts.find(account => account.user === snapshot.uplinks.find(uplink => uplink.account)?.account);
      this.account = accounts.some(account => account.value === configured?.id) ? configured!.id : accounts[0]?.value || 'current';
    }
  }

  private chooseUplink(): void {
    const snapshot = this.snapshot();
    if (!snapshot?.uplinks.length) {
      this.uplink = '';
      return;
    }
    const selected = snapshot.accounts.find(account => account.id === this.account);
    const matching = selected
      ? snapshot.uplinks.find(uplink => uplink.account === selected.user)
      : snapshot.uplinks.find(uplink => uplink.account);
    this.uplink = matching?.id || snapshot.uplinks[0].id;
  }

  private resetData(): void {
    this.overview = null;
    this.sessions = [];
    this.services = null;
    this.devices = null;
    this.security = null;
    this.history = null;
    this.moduleWarnings = [];
    this.actionTicket = '';
  }

  private captureScope(includeOverview = true): RequestScope {
    this.ensureAccount();
    this.chooseUplink();
    const snapshot = this.snapshot();
    if (!snapshot) throw new Error('路由器已断开，请重新连接。');
    if (!this.uplink) throw new Error('请先在路由器设置中添加上游。');
    return {
      generation: this.generation, router: snapshot.managementUrl, account: this.account, uplink: this.uplink,
      expectedAccount: String(snapshot.accounts.find(account => account.id === this.account)?.user
        || (includeOverview ? this.overview?.account : '') || ''),
    };
  }

  private assertScope(scope: RequestScope): void {
    if (scope.generation !== this.generation || !this.snapshot()
      || this.snapshot()?.managementUrl !== scope.router || this.account !== scope.account) {
      throw new Error('路由器连接或计费账号已变化，已取消旧查询。');
    }
  }

  private async scopedRequest<T>(scope: RequestScope, operation: string, args: Record<string, unknown>): Promise<T> {
    this.assertScope(scope);
    const response = await this.request<T>(operation, args);
    this.assertScope(scope);
    return response;
  }

  private async loadModule(module: string, scope: RequestScope): Promise<Data> {
    const start = await this.scopedRequest<Data>(scope, 'billing_details', {
      uplink: scope.uplink,
      account: scope.account,
      module,
    });
    if (!start.ticket) throw new Error(String(start.message || '无法开始查询'));
    const response = await this.waitForTicket(String(start.ticket), scope);
    const result = response.result || response;
    return { ...result, ticket: String(start.ticket) };
  }

  private async runAction(operation: string, payload: Data): Promise<Data> {
    const scope = this.captureScope();
    if (!this.actionTicket) throw new Error('计费会话已过期，请刷新后重试。');
    const start = await this.scopedRequest<Data>(scope, 'billing_action', {
      ticket: this.actionTicket,
      operation,
      payload: JSON.stringify(payload),
    });
    if (!start.ticket) throw new Error(String(start.message || '操作未开始'));
    const response = await this.waitForTicket(String(start.ticket), scope, false);
    return response.result || response;
  }

  private async waitForTicket(ticket: string, scope: RequestScope, requireAccount = true): Promise<Data> {
    let captchaRounds = 0;
    for (let attempt = 0; attempt < 120; attempt++) {
      const response = await this.scopedRequest<Data>(scope, 'billing_poll', { ticket });
      const result = response.result || response;
      if (result.account && scope.expectedAccount && String(result.account) !== scope.expectedAccount) {
        throw new Error('计费账号与所选账号不一致，请刷新后重试。');
      }
      if (result.state === 'loading') {
        const wait = deferred<void>();
        window.setTimeout(wait.resolve, 250);
        await wait.promise;
        continue;
      }
      if (result.state === 'captcha') {
        if (++captchaRounds > 6) throw new Error('计费验证码尝试次数过多');
        const answer = await this.askCaptcha(result);
        this.assertScope(scope);
        if (!answer) throw new Error('已取消计费验证码');
        const captcha = await this.scopedRequest<Data>(scope, 'billing_captcha', {
          ticket,
          code: answer.code || '',
          refresh: Boolean(answer.refresh),
        });
        if (captcha.state === 'error') throw new Error(String(captcha.message || '计费验证码未通过'));
        continue;
      }
      if (result.state === 'error') throw new Error(String(result.message || '计费查询失败'));
      if (result.state !== 'ready') throw new Error('路由器返回了无法识别的计费状态，请刷新后重试。');
      if (requireAccount && !result.account) {
        throw new Error('未能核对计费账号，请刷新后重试。');
      }
      return response;
    }
    throw new Error('计费查询超时，请稍后重试');
  }

  private askCaptcha(result: Data): Promise<{ code?: string; refresh?: boolean } | null> {
    const pending = deferred<{ code?: string; refresh?: boolean } | null>();
    const overlay = document.createElement('div');
    overlay.className = 'modal-overlay hidden image-captcha-overlay';
    overlay.innerHTML = `<form class="modal-content glass-card config-transfer-dialog" role="dialog" aria-modal="true">
      <h3>路由器计费验证码</h3><p class="transfer-note">请填写图片中的验证码。</p>
      <div class="image-captcha-picture"><img alt="计费验证码"><button type="button" class="btn btn-secondary btn-sm" data-refresh>换一张</button></div>
      <div class="form-group"><label>验证码<input type="text" autocomplete="off" autocapitalize="off" spellcheck="false" maxlength="12" required></label></div>
      <p class="transfer-error" role="alert"></p>
      <div class="modal-actions"><button type="button" class="btn btn-secondary" data-cancel>取消</button><button type="submit" class="btn btn-primary">继续</button></div>
    </form>`;
    const image = overlay.querySelector('img')!;
    if (/^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/=]+$/.test(String(result.image || ''))) image.src = result.image;
    const input = overlay.querySelector('input')!;
    const finish = (value: { code?: string; refresh?: boolean } | null) => {
      overlay.remove();
      if (this.cancelCaptcha === cancel) this.cancelCaptcha = null;
      pending.resolve(value);
    };
    const cancel = () => finish(null);
    this.cancelCaptcha?.();
    this.cancelCaptcha = cancel;
    overlay.querySelector('form')!.addEventListener('submit', event => {
      event.preventDefault();
      if (input.value.trim()) finish({ code: input.value.trim() });
    });
    overlay.querySelector('[data-refresh]')!.addEventListener('click', () => finish({ refresh: true }));
    overlay.querySelector('[data-cancel]')!.addEventListener('click', () => finish(null));
    overlay.addEventListener('keydown', event => {
      if (event.key === 'Escape') { event.preventDefault(); finish(null); }
    });
    document.body.append(overlay);
    requestAnimationFrame(() => {
      if (!overlay.isConnected) return;
      overlay.classList.remove('hidden');
      input.focus({ preventScroll: true });
    });
    return pending.promise;
  }

  private compose(): BillingCenterData {
    const overview = this.overview || {};
    const now = new Date();
    const start = new Date(now.getFullYear(), now.getMonth(), 1);
    const account = String(overview.account || this.snapshot()?.accounts.find(item => item.id === this.account)?.user || '');
    const loginHistory = this.historyRows();
    const onlineSessions = this.sessionRows();
    const macs = this.devices?.macs;
    const service = this.serviceData(overview);
    const policy = this.security || {};
    const updatedAt = Number(overview.updatedAt);
    const fetchedAt = Number.isFinite(updatedAt) && updatedAt > 0
      ? new Date(updatedAt * 1000).toLocaleString('zh-CN', { hour12: false })
      : String(overview.updatedAt || new Date().toLocaleString('zh-CN', { hour12: false }));
    return {
      account,
      overview: {
        account,
        balance: String(overview.balanceText || money(overview.balance)),
        remainingFlow: String(overview.remainingFlowText || flow(overview.remainingFlow)),
        usedFlow: String(overview.usedFlowText || flow(overview.usedFlow)),
        status: String(overview.accountStatus || '--'),
        statusReason: String(overview.statusReason || ''),
        package: String(overview.package || this.services?.currentPackage || ''),
        packageDetail: String(overview.packageDetail || ''),
        billingCycle: String(overview.billingCycle || ''),
        updatedAt: fetchedAt,
        loginHistory,
        onlineSessions,
        offlineTip: '注销会话会中断对应设备的校园网连接；无感认证设备可能还需要解绑 MAC。',
        mauthEnabled: this.devices?.mauth === '1' ? true : this.devices?.mauth === '0' ? false : undefined,
        warnings: this.moduleWarnings.concat(Array.isArray(overview.warnings)?overview.warnings.map(String):[],overview._cached?['当前显示路由器后台保存的结果，请注意数据更新时间。']:[],overview.backgroundMessage?[String(overview.backgroundMessage)]:[]),
      },
      fetchedAt,
      queryStartDate: dateValue(start),
      queryEndDate: dateValue(now),
      queryYear: String(now.getFullYear()),
      usageRecords: EMPTY_TABLE(),
      monthlyBills: EMPTY_TABLE(),
      payments: EMPTY_TABLE(),
      operations: EMPTY_TABLE(),
      stopLogs: EMPTY_TABLE(),
      reopenLogs: EMPTY_TABLE(),
      packageLogs: EMPTY_TABLE(),
      devices: tableRows(macs?.columns, macs?.rows),
      tariffGroups: EMPTY_TABLE(),
      service,
      passwordPolicy: {
        minLength: Number(policy.FLDMINLENGTH) || 8,
        maxLength: 16,
        requireUppercase: Boolean(Number(policy.FLDREQUIREUPPER)),
        requireLowercase: Boolean(Number(policy.FLDREQUIRELOWER)),
        requireDigit: Boolean(Number(policy.FLDREQUIREDIGIT)),
        requireSpecial: Boolean(Number(policy.FLDREQUIRESPECIAL)),
      },
      securityQuestions: (Array.isArray(policy.questions) ? policy.questions : []).map((question: Data) => ({
        id: String(question.id ?? ''),
        text: String(question.name ?? question.text ?? ''),
      })),
      rechargeAvailable: false,
      warnings: this.moduleWarnings.slice(),
    };
  }

  private historyRows(): BillingLoginRecord[] {
    const history: Data = this.history?.history || {};
    const columns: string[] = Array.isArray(history.columns) ? history.columns.map(String) : [];
    const rows: unknown[][] = Array.isArray(history.rows) ? history.rows : [];
    return rows.map((row: unknown[]) => {
      const values = Object.fromEntries(columns.map((key, index) => [key, String(row[index] ?? '')]));
      return {
        loginAt: timestamp(rowValue(values, ['loginTime', 'loginAt'])),
        logoutAt: timestamp(rowValue(values, ['logoutTime', 'logoutAt'])),
        ip: rowValue(values, ['userIp', 'ip']),
        ipv6: rowValue(values, ['userIp1', 'ipv6']),
        mac: rowValue(values, ['macAddress', 'mac']),
        durationMinutes: duration(rowValue(values, ['durationMinutes', 'time'])),
        usedFlowMb: rowValue(values, ['flowMb', 'flow']),
        billingMode: ({ '1': '时长', '2': '流量', '3': '包月' } as Record<string, string>)[rowValue(values, ['billingMode'])] || rowValue(values, ['billingMode']),
        amount: money(rowValue(values, ['costMoney', 'amount'])),
      };
    });
  }

  private sessionRows(): BillingOnlineSession[] {
    return this.sessions.map((session, index) => ({
      loginAt: timestamp(session.loginTime || session.loginAt),
      ip: String(session.ip || '--'),
      ipv6: String(session.ipv6 || '--'),
      mac: String(session.mac || '--'),
      durationMinutes: session.useTime ? String(Math.round(Number(session.useTime) / 60)) : String(session.durationMinutes || '--'),
      usedFlowMb: session.downFlow?.trim() && session.upFlow?.trim()
        && Number.isFinite(Number(session.downFlow)) && Number.isFinite(Number(session.upFlow))
        ? ((Number(session.downFlow) + Number(session.upFlow)) / 1024).toFixed(3)
        : String(session.flowMb || session.flow || '--'),
      sessionId: String(session.sessionIndex ?? session.sessionId ?? session.ip ?? index),
    }));
  }

  private serviceData(overview: Data): BillingServiceState {
    const service = this.services || {};
    const packages: BillingPackageOption[] = (Array.isArray(service.packages) ? service.packages : []).map((item: Data) => ({
      id: String(item.id ?? ''),
      name: String(item.name ?? ''),
      description: '',
    }));
    const currentPackage = String(service.currentPackage || overview.package || '');
    const scheduledPackage = String(service.scheduledPackage || '');
    const normalize = (value: string) => value.replace(/\s+/g, '').toLowerCase();
    const currentPackageId = packages.find(item => normalize(item.name) === normalize(currentPackage))?.id || '';
    const scheduledPackageId = packages.find(item => normalize(item.name) === normalize(scheduledPackage))?.id || '';
    const limit = String(service.installmentFlag ?? '');
    return {
      accountStatus: String(overview.accountStatus || ''),
      statusReason: String(service.message || '报停、复通、套餐预约与消费保护。'),
      currentPackageId,
      currentPackage,
      packageDetail: '',
      nextSettlementDate: '',
      canStopNow: Boolean(Number(service.canStop)),
      canReopenNow: Boolean(Number(service.canReopen)),
      packageScheduled: Boolean(scheduledPackage),
      scheduledPackageId,
      scheduledPackage,
      consumeLimit: limit === '999999' ? '不限制' : limit ? `${limit} 元` : '',
      currentCycleSpend: service.useMoney ? `${service.useMoney} 元` : '',
      balance: money(overview.balance),
      packageOptions: packages,
    };
  }
}
