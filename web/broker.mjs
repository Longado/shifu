import { randomUUID, createHash, timingSafeEqual } from 'node:crypto';
import { readFile, writeFile, rename } from 'node:fs/promises';
import { join } from 'node:path';
import { BrowserError, targetKey } from './playwright.mjs';
import { readCapacity } from './capacity.mjs';

export const MAX_REQUEST = 64 * 1024;
export const MAX_RESULT = 32 * 1024;
const MAX_RECOVERY = 32;
const SLOT_WAIT = 30000;
const fail = (code, message, phase) => { throw new BrowserError(code, message, phase); };
const object = value => value && typeof value === 'object' && !Array.isArray(value);
const string = (value, max = 128) => typeof value === 'string' && value.length > 0 && value.length <= max;
const id = value => typeof value === 'string' && /^[a-zA-Z0-9_-]{1,80}$/.test(value);
const roles = new Set(['textbox', 'button', 'link', 'combobox', 'checkbox', 'radio', 'region', 'main', 'group', 'dialog', 'form', 'heading']);

function target(value, allowUnnamed = false) {
  if (!object(value) || !roles.has(value.role) || !(string(value.name, 512) || allowUnnamed && value.name === '')) fail('INVALID_REQUEST', '目标需要受支持的role和准确名称。');
  return { role: value.role, name: value.name };
}

function findQueries(value) {
  if (!Array.isArray(value) || value.length < 1 || value.length > 8 || value.some(query => !string(query, 128) || !query.trim())) {
    fail('INVALID_REQUEST', 'queries需要1至8个非空关键词，每个不超过128字；按字面量查找，不接受正则表达式。');
  }
  return value.map(query => query.trim());
}

function url(value) {
  try {
    const parsed = new URL(value);
    if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password) throw new Error();
    return parsed.href;
  } catch { fail('INVALID_REQUEST', '只支持不带嵌入账号密码的HTTP/HTTPS网址。'); }
}

function operation(value) {
  if (!object(value)) fail('INVALID_REQUEST', '缺少operation。');
  if (value.type === 'navigate') return { type: value.type, url: url(value.url) };
  if (value.type === 'click') return { type: value.type, target: target(value.target) };
  if (value.type === 'fill_form' && Array.isArray(value.fields) && value.fields.length > 0 && value.fields.length <= 32) {
    return { type: value.type, fields: value.fields.map(field => {
      const locator = target(field);
      if (locator.role !== 'textbox' || typeof field.value !== 'string' || Buffer.byteLength(field.value) > 8192) {
        fail('INVALID_REQUEST', '最小版批量填写只支持textbox，单字段不超过8 KiB。');
      }
      return { ...locator, value: field.value };
    }) };
  }
  fail('UNSUPPORTED_OPERATION', '最小版仅支持navigate、fill_form和click，不接受JS/CDP或原生输入。');
}

export function snapshotPart(observation, cursor = 0) {
  const bytes = Buffer.from(observation.text);
  if (!Number.isInteger(cursor) || cursor < 0 || cursor > bytes.length || (cursor < bytes.length && (bytes[cursor] & 0xc0) === 0x80)) {
    fail('INVALID_REQUEST', '无效的观察游标。');
  }
  let end = Math.min(bytes.length, cursor + 8192);
  while (end < bytes.length && (bytes[end] & 0xc0) === 0x80) end--;
  return { observationId: observation.id, snapshot: bytes.subarray(cursor, end).toString(), nextCursor: end < bytes.length ? end : null, totalBytes: bytes.length };
}

export class Broker {
  constructor({ home, provider, controlToken, capacity = readCapacity, maxReceipts = 1000 }) {
    this.home = home;
    this.provider = provider;
    this.controlToken = controlToken;
    this.capacity = capacity;
    this.slotWaiters = new Set();
    this.maxReceipts = maxReceipts;
    this.epoch = randomUUID();
    this.tasks = new Map();
    this.receipts = new Map();
    this.recovery = [];
    this.saves = Promise.resolve();
  }

  async init() {
    try {
      const state = JSON.parse(await readFile(join(this.home, 'state.json'), 'utf8'));
      if (state.version !== 1 || !Array.isArray(state.tasks) || !Array.isArray(state.recovery)) throw new Error('version');
      for (const receipt of (state.receipts ?? []).slice(-this.maxReceipts)) {
        this.receipts.set(`${receipt.result.task}/${receipt.requestId}`, receipt);
      }
      // A disconnected worker cannot restore its incognito pages or replay a request.
      this.recovery = [...state.recovery, ...state.tasks.map(task => ({ task: task.id,
        pending: task.pending ? { ...task.pending, phase: task.pending.phase === 'dispatched' ? 'outcome_unknown' : task.pending.phase } : null,
        previousEpoch: state.epoch, state: 'connection_lost' }))];
      if (this.recovery.length > MAX_RECOVERY) fail('STATE_INVALID', '恢复记录超出任务上限，请保留原记录检查。');
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
    await this.persist();
  }

  persist() {
    const data = JSON.stringify({ version: 1, epoch: this.epoch, tasks: [...this.tasks.values()].map(task => ({
      id: task.id, pageId: task.resource?.pageId ?? null, state: task.state, pending: task.pending,
    })), recovery: this.recovery, receipts: [...this.receipts.values()].map(({ digest, ...receipt }) => receipt) });
    const save = this.saves.then(async () => {
      const temporary = join(this.home, `state-${randomUUID()}.tmp`);
      await writeFile(temporary, data, { mode: 0o600 });
      await rename(temporary, join(this.home, 'state.json'));
    });
    this.saves = save.catch(() => {}); // The caller still receives the failure; later explicit saves may recover.
    return save;
  }

  control(request) {
    const supplied = Buffer.from(typeof request.controlToken === 'string' ? request.controlToken : '');
    const actual = Buffer.from(this.controlToken);
    if (supplied.length !== actual.length || !timingSafeEqual(supplied, actual)) fail('CONTROL_REQUIRED', '需要本人控制入口，Agent不能自行确认释放或未知结果。');
  }

  task(request) {
    const task = this.tasks.get(request.task);
    if (!task) fail('TASK_NOT_FOUND', '没有这个任务；重启后旧任务只能核对，不能自动续写。');
    return task;
  }

  actor(task, request) {
    const actor = task.actors.get(request.actorId);
    if (!actor) fail('ACTOR_INVALID', '执行实例不属于本任务，请重新登记。');
    return actor;
  }

  lease(task, request, active = true) {
    this.actor(task, request);
    if (request.lease?.epoch !== this.epoch || request.lease?.generation !== task.generation || task.owner !== request.actorId) {
      fail('STALE_LEASE', '操作权已交给其他执行者，旧请求未派发。');
    }
    if (active && task.state !== 'active') fail('TASK_PAUSED', '任务已暂停或正在交接，未派发新动作。');
  }

  summary(task) {
    return { task: task.id, state: task.state, pageId: task.resource?.pageId ?? null, loaded: task.resource?.loaded ?? null,
      owner: task.owner ? task.actors.get(task.owner)?.label : null, generation: task.generation,
      inFlight: task.inFlight, pendingRequestId: task.pending?.requestId ?? null,
      queue: task.queue.map(actor => ({ actorId: actor, label: task.actors.get(actor)?.label })) };
  }

  handoff(task) {
    task.owner = task.queue.shift() ?? null;
    task.generation++;
    task.state = 'active';
    if (task.resource) task.resource.observation = null;
    this.notify(task);
  }

  ownership(task, actorId) {
    return { acquired: true, task: task.id, pageId: task.resource.pageId, lease: { epoch: this.epoch, generation: task.generation } };
  }

  notify(task) {
    for (const [actorId, waiter] of task.waiters) {
      if (task.state === 'active' && task.owner === actorId) waiter.finish(this.ownership(task, actorId));
      else if (!['active', 'draining'].includes(task.state) || !task.queue.includes(actorId) && task.owner !== actorId) {
        waiter.finish({ acquired: false, queued: false, ...this.summary(task) });
      }
    }
  }

  used() { return this.tasks.size + this.recovery.length; }

  // Waits until this machine has room for one more task environment. The check and the
  // reservation by the caller happen without an await in between, so waiters cannot overbook.
  async reserveSlot(request) {
    const deadline = Date.now() + (request.wait === false ? 0 : SLOT_WAIT);
    for (;;) {
      if (this.tasks.has(request.task) || this.recovery.some(task => task.task === request.task)) fail('TASK_EXISTS', '任务已存在，不能覆盖页面或未核对记录。');
      const capacity = this.capacity();
      if (this.used() < capacity.limit) return;
      const left = deadline - Date.now();
      if (left <= 0) fail('CAPACITY_REACHED', `本机当前最多同时${capacity.limit}个任务环境，已满；等任务释放后再打开。`);
      // Memory may also free up without a release, so recheck periodically.
      await new Promise(resolve => {
        const waiter = () => { clearTimeout(timer); this.slotWaiters.delete(waiter); resolve(); };
        const timer = setTimeout(waiter, Math.min(left, 2000));
        this.slotWaiters.add(waiter);
      });
    }
  }

  releaseSlot() { for (const waiter of [...this.slotWaiters]) waiter(); }

  async request(request) {
    try {
      if (!object(request) || !string(request.method, 64)) fail('INVALID_REQUEST', '需要JSON对象和method。');
      return { ok: true, result: await this.dispatch(request) };
    } catch (error) {
      if (error instanceof BrowserError) return { ok: false, error: { code: error.code, message: error.message, phase: error.phase } };
      // Provider errors can contain field contents/URLs. Never forward or persist raw errors.
      return { ok: false, error: { code: 'RUNTIME_ERROR', message: '运行失败；先查看任务状态，修改动作不要自动重试。', phase: 'outcome_unknown' } };
    }
  }

  async dispatch(request) {
    if (request.method === 'status') return { version: 1, epoch: this.epoch, mode: 'headless-owned-contexts',
      capacity: { ...this.capacity(), used: this.used(), waiting: this.slotWaiters.size },
      tasks: [...this.tasks.values()].map(task => this.summary(task)), recovery: this.recovery,
      coverage: '仅管理本入口创建的独立任务页，不管理本人浏览器或外部工具。' };
    if (request.method === 'open') {
      if (!id(request.task)) fail('INVALID_REQUEST', 'task使用1到80位字母、数字、下划线或短横线。');
      const destination = url(request.url);
      await this.reserveSlot(request);
      const task = { id: request.task, actors: new Map(), owner: null, generation: 0, queue: [], waiters: new Map(), state: 'opening', pending: null, inFlight: true, resource: null };
      this.tasks.set(task.id, task);
      try {
        await this.persist();
        task.resource = await this.provider.open(destination);
        task.state = 'active';
        task.inFlight = false;
        await this.persist();
        return this.summary(task);
      } catch (error) {
        if (task.resource) {
          task.state = 'paused';
          task.inFlight = false;
          fail('STORAGE_ERROR', '任务页已创建，但状态保存失败；保留该页和额度，修复存储后核对或释放。', 'outcome_unknown');
        }
        this.tasks.delete(task.id);
        this.releaseSlot();
        await this.persist();
        throw error;
      }
    }
    if (request.method === 'resolve_lost') {
      this.control(request);
      if (!['checked', 'abandon_unknown'].includes(request.outcome)) fail('INVALID_REQUEST', '须明确选择checked或abandon_unknown。');
      const index = this.recovery.findIndex(task => task.task === request.task);
      if (index < 0) fail('TASK_NOT_FOUND', '没有这个待核对的旧任务。');
      this.recovery.splice(index, 1);
      this.releaseSlot();
      await this.persist();
      return { task: request.task, resolved: true, outcome: request.outcome, replayed: false, pagesRestored: false };
    }
    if (request.method === 'receipt') {
      const receipt = this.receipts.get(`${request.task}/${request.requestId}`);
      if (request.controlToken) this.control(request);
      else {
        this.actor(this.task(request), request);
        if (receipt && receipt.actorId !== request.actorId) fail('ACTOR_INVALID', '不能读取另一执行者的动作回执。');
      }
      return receipt ? { known: true, ...receipt.result } : { known: false, message: '记录不存在或已轮转，不能据此判断原动作未执行。' };
    }

    const task = this.task(request);
    if (request.method === 'join') {
      if (!string(request.label, 64)) fail('INVALID_REQUEST', '需要64字以内的执行者名称。');
      if (task.actors.size >= 32) fail('CAPACITY_REACHED', '本任务登记实例已满。');
      const actorId = randomUUID();
      task.actors.set(actorId, { label: request.label, sequence: 0 });
      return { task: task.id, actorId, label: request.label };
    }
    if (request.method === 'claim') {
      this.actor(task, request);
      if (!['active', 'draining'].includes(task.state)) fail('TASK_PAUSED', '任务未准备好，先查看状态。');
      if (!task.owner && task.state === 'active') { task.owner = request.actorId; task.generation++; }
      if (task.owner !== request.actorId) {
        if (!task.queue.includes(request.actorId)) task.queue.push(request.actorId);
        return { acquired: false, queued: true, position: task.queue.indexOf(request.actorId) + 1, ...this.summary(task) };
      }
      if (task.state === 'draining') fail('TASK_PAUSED', '原执行者正在交回操作权，不能重新开始动作。');
      return this.ownership(task, request.actorId);
    }
    if (request.method === 'wait') {
      const claimed = await this.dispatch({ ...request, method: 'claim' });
      if (claimed.acquired) return claimed;
      if (task.waiters.has(request.actorId)) fail('BUSY', '本执行者已有一个等待请求。');
      return new Promise(resolve => {
        const finish = result => { clearTimeout(timer); task.waiters.delete(request.actorId); resolve(result); };
        const timer = setTimeout(() => finish({ acquired: false, queued: true, ...this.summary(task), reason: 'wait_timeout' }), 30000);
        task.waiters.set(request.actorId, { finish });
        this.notify(task);
      });
    }
    if (request.method === 'cancel_wait') {
      this.actor(task, request);
      task.queue = task.queue.filter(actor => actor !== request.actorId);
      this.notify(task);
      return this.summary(task);
    }
    if (request.method === 'yield' || request.method === 'handoff') {
      if (request.method === 'handoff') {
        this.control(request);
        if (!['active', 'draining'].includes(task.state)) fail('TASK_PAUSED', '先核对暂停任务，再转交操作权。');
      } else this.lease(task, request);
      task.state = 'draining';
      if (!task.inFlight) this.handoff(task);
      await this.persist();
      return this.summary(task);
    }
    if (request.method === 'pause') {
      this.control(request);
      task.state = 'paused';
      this.notify(task);
      await this.persist();
      return this.summary(task);
    }
    if (request.method === 'resume') {
      this.control(request);
      if (task.inFlight) fail('BUSY', '原调用仍在途，不能恢复写入。');
      if (task.state !== 'paused' || !['checked', 'abandon_unknown'].includes(request.outcome)) fail('INVALID_REQUEST', '只可在暂停且明确核对/处置后恢复。');
      task.pending = null;
      task.state = 'active';
      task.generation++;
      if (task.resource) task.resource.observation = null;
      await this.persist();
      return { ...this.summary(task), outcome: request.outcome, replayed: false };
    }
    if (request.method === 'close') {
      // The current lease holder may release its own finished task; only the person can settle unknown results.
      const byControl = request.controlToken !== undefined;
      if (byControl) this.control(request);
      else this.lease(task, request, false);
      if (request.discard !== true) fail('CONFIRM_DISCARD', '释放会关闭本任务全部页面；请明确discard:true。');
      if (task.inFlight) fail('BUSY', '原调用仍在途，先等待结果。');
      if (task.pending && !byControl) fail('REVIEW_REQUIRED', '有未知结果，须由本人核对后释放。');
      if (task.pending && !['checked', 'abandon_unknown'].includes(request.outcome)) fail('REVIEW_REQUIRED', '未知结果须先明确核对或处置。');
      task.state = 'disposing';
      task.inFlight = true;
      try {
        await this.persist();
        await this.provider.close(task.resource);
        task.state = 'disposed';
        this.notify(task);
        this.tasks.delete(task.id);
        this.releaseSlot();
        await this.persist();
        return { task: task.id, disposed: true, remainingTasks: this.tasks.size };
      } finally { task.inFlight = false; }
    }
    if (request.method === 'observe' || request.method === 'inspect' || request.method === 'find') {
      if (request.method === 'inspect') this.control(request);
      else this.lease(task, request);
      if (task.inFlight) fail('BUSY', '原页面调用未结束，暂不可观察。');
      if (!task.resource) fail('TASK_NOT_READY', '页面尚未准备好。');
      if (request.cursor !== undefined) {
        if (request.method === 'find') fail('INVALID_REQUEST', '关键词查找不使用观察游标，请缩小scope或细化关键词。');
        if (task.resource.observation?.id !== request.observationId) fail('STALE_OBSERVATION', '观察版本已失效。');
        return { task: task.id, ...snapshotPart(task.resource.observation, request.cursor) };
      }
      // Landmarks such as <main> commonly have no accessible name. This only
      // relaxes read scope selection; click/fill targets still require names.
      const scope = request.scope === undefined ? null : target(request.scope, true);
      const queries = request.method === 'find' ? findQueries(request.queries) : null;
      task.inFlight = true;
      try {
        if (queries) {
          // Reading excerpts never provides or preserves permission to act on a snapshot.
          task.resource.observation = null;
          return { task: task.id, pageId: task.resource.pageId, ...await this.provider.find(task.resource, queries, scope) };
        }
        const observation = await this.provider.observe(task.resource, scope);
        return { task: task.id, ...snapshotPart(observation) };
      } catch (error) {
        if (error instanceof BrowserError) throw error;
        // Reading changes nothing on the site; a timeout here is an ordinary failure, not an unknown result.
        fail('READ_FAILED', '读取失败：范围不存在或页面仍在加载。可去掉scope、稍后再读或换页。', 'not_dispatched');
      } finally {
        task.inFlight = false;
        if (task.state === 'draining') this.handoff(task);
      }
    }
    if (request.method === 'act') {
      this.lease(task, request);
      const actor = this.actor(task, request);
      const op = operation(request.operation);
      if (!string(request.requestId, 128) || !Number.isSafeInteger(request.sequence) || request.sequence < 1) fail('INVALID_REQUEST', '动作需要requestId与递增的sequence。');
      const digest = createHash('sha256').update(JSON.stringify(op)).digest('hex');
      const key = `${task.id}/${request.requestId}`;
      const previous = this.receipts.get(key);
      if (previous) {
        if (previous.actorId !== request.actorId || previous.digest !== digest || previous.sequence !== request.sequence) fail('REQUEST_ID_REUSED', '请求编号已被另一动作使用。');
        return { ...previous.result, duplicate: true };
      }
      if (request.sequence !== actor.sequence + 1) fail('SEQUENCE_REUSED', '顺序号不连续或已执行，旧动作不会重新派发。');
      if (task.inFlight) fail('BUSY', '本任务正在执行另一调用。');
      if (op.type !== 'navigate') {
        const observation = task.resource.observation;
        if (!observation || request.observationId !== observation.id) fail('STALE_OBSERVATION', '先观察当前页面，再用本次观察填写或点击。');
        for (const locator of op.type === 'fill_form' ? op.fields : [op.target]) {
          if (observation.targets.get(targetKey(locator)) !== 1) fail('TARGET_NOT_OBSERVED', '目标须在当前观察范围中出现一次；请缩小观察范围或使用准确名称。');
        }
      }
      actor.sequence = request.sequence;
      task.inFlight = true;
      task.pending = { requestId: request.requestId, operation: op.type, actorId: request.actorId, sequence: request.sequence, phase: 'dispatched' };
      try { await this.persist(); }
      catch {
        task.inFlight = false;
        task.state = 'paused';
        this.notify(task);
        task.pending.phase = 'not_dispatched';
        fail('STORAGE_ERROR', '未能保存动作摘要，动作未派发。修复存储后核对再恢复。');
      }
      let result;
      try {
        await this.provider.act(task.resource, op);
        result = { task: task.id, requestId: request.requestId, phase: 'completed', operation: op.type,
          ...(op.type === 'navigate' ? { loaded: task.resource.loaded } : {}) };
        task.pending = null;
      } catch {
        task.resource.observation = null;
        if (op.type === 'navigate') {
          // A navigation only loads a page; it does not submit anything on the site, so a timeout or
          // refused page is an ordinary failure. The task stays active and the actor may go elsewhere.
          result = { task: task.id, requestId: request.requestId, phase: 'navigation_failed', operation: op.type,
            message: '页面没有响应或无法打开；任务仍可用，可跳到其他页面。' };
          task.pending = null;
        } else {
          task.state = 'paused';
          this.notify(task);
          task.pending.phase = 'outcome_unknown';
          result = { task: task.id, requestId: request.requestId, phase: 'outcome_unknown', operation: op.type,
            message: '调用未正常结束，操作可能已生效或部分完成。核对后由本人恢复，不自动重试。' };
        }
      } finally { task.inFlight = false; }
      this.receipts.set(key, { requestId: request.requestId, actorId: request.actorId, sequence: request.sequence, digest, result });
      while (this.receipts.size > this.maxReceipts) {
        const removable = [...this.receipts.keys()].find(key => ![...this.tasks.values()].some(task => `${task.id}/${task.pending?.requestId}` === key));
        if (!removable) break;
        this.receipts.delete(removable);
      }
      if (task.state === 'draining') this.handoff(task);
      await this.persist();
      return result;
    }
    fail('UNSUPPORTED_METHOD', '不支持这个方法。');
  }
}
