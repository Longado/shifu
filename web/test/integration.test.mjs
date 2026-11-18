import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdtemp, rm, readFile, mkdir, writeFile, chmod } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import { createService, call } from '../service.mjs';
import { fixture } from './fixture.mjs';
import { machineCapacity, readCapacity } from '../capacity.mjs';

const connector = process.env.SHIFU_TEST_CONNECTOR;
const connect = connector ? createRequire(import.meta.url)(connector).connect : null;
const required = { skip: !connect && '需要SHIFU_TEST_CONNECTOR指向共用无界面浏览器连接器；未执行真实浏览器测试。', timeout: 30000 };
const control = (service, request) => ({ ...request, controlToken: service.controlToken });
const ok = response => { assert.equal(response.ok, true, JSON.stringify(response)); return response.result; };

async function setup(options = {}) {
  const home = await mkdtemp(join(tmpdir(), 'sf-b-'));
  const web = await fixture();
  const service = await createService({ home, connect, ...options });
  const request = value => call(home, value);
  const open = async task => ok(await request({ method: 'open', task, url: web.url }));
  const actor = async (task, label) => ok(await request({ method: 'join', task, label })).actorId;
  const claim = async (task, actorId) => ok(await request({ method: 'claim', task, actorId }));
  const cleanup = async () => { await service.close(); await web.close(); await rm(home, { recursive: true, force: true }); };
  return { home, web, service, request, open, actor, claim, cleanup };
}

test('真实页：排队、原页和模拟登录交接、旧实例拒绝、重复请求不重复保存、无正文落盘', required, async () => {
  const s = await setup();
  try {
    const opened = await s.open('shared');
    const a = await s.actor('shared', 'A');
    const b = await s.actor('shared', 'B');
    const leaseA = (await s.claim('shared', a)).lease;
    const page = s.service.broker.tasks.get('shared').resource.page;
    const context = page.context();
    const cookieA = await context.cookies();
    const observation = ok(await s.request({ method: 'observe', task: 'shared', actorId: a, lease: leaseA }));
    assert.match(observation.snapshot, /姓名/);
    assert.match(observation.snapshot, /备注/);
    const fillA = { method: 'act', task: 'shared', actorId: a, lease: leaseA, sequence: 1, requestId: 'fill-A', observationId: observation.observationId,
      operation: { type: 'fill_form', fields: [{ role: 'textbox', name: '姓名', value: 'ALPHA-PRIVATE-FIXTURE' }] } };
    assert.equal(ok(await s.request(fillA)).phase, 'completed');
    assert.equal((await s.claim('shared', b)).queued, true);
    assert.equal((await s.request({ ...fillA, actorId: b, requestId: 'B-wrong' })).error.code, 'STALE_LEASE');
    ok(await s.request({ method: 'yield', task: 'shared', actorId: a, lease: leaseA }));
    const leaseB = (await s.claim('shared', b)).lease;
    assert.equal(s.service.broker.tasks.get('shared').resource.pageId, opened.pageId);
    assert.deepEqual(await context.cookies(), cookieA);
    assert.equal((await s.request(fillA)).error.code, 'STALE_LEASE');
    let observed = ok(await s.request({ method: 'observe', task: 'shared', actorId: b, lease: leaseB }));
    ok(await s.request({ method: 'act', task: 'shared', actorId: b, lease: leaseB, sequence: 1, requestId: 'fill-B', observationId: observed.observationId,
      operation: { type: 'fill_form', fields: [{ role: 'textbox', name: '备注', value: 'BETA-PRIVATE-FIXTURE' }] } }));
    observed = ok(await s.request({ method: 'observe', task: 'shared', actorId: b, lease: leaseB }));
    const save = { method: 'act', task: 'shared', actorId: b, lease: leaseB, sequence: 2, requestId: 'save-B', observationId: observed.observationId,
      operation: { type: 'click', target: { role: 'button', name: '保存草稿' } } };
    ok(await s.request(save));
    await s.web.waitFor(1);
    assert.equal(ok(await s.request(save)).duplicate, true);
    assert.equal(s.web.saves.length, 1);
    assert.deepEqual(s.web.saves[0], { name: 'ALPHA-PRIVATE-FIXTURE', note: 'BETA-PRIVATE-FIXTURE' });
    const record = await readFile(join(s.home, 'state.json'), 'utf8');
    assert(!record.includes('ALPHA-PRIVATE-FIXTURE') && !record.includes('BETA-PRIVATE-FIXTURE'));
    assert(!record.includes('snapshot') && !record.includes('http:'));
    if (process.env.SHIFU_TEST_ARTIFACT_DIR) {
      await mkdir(process.env.SHIFU_TEST_ARTIFACT_DIR, { recursive: true });
      await page.getByText('已保存 1 次', { exact: true }).waitFor();
      await page.screenshot({ path: join(process.env.SHIFU_TEST_ARTIFACT_DIR, 'handoff.png') });
      await writeFile(join(process.env.SHIFU_TEST_ARTIFACT_DIR, 'handoff.json'), JSON.stringify({ samePage: true, sameSyntheticLogin: true, saveCount: s.web.saves.length, oldActorRejected: true, privateValuesAbsentFromJournal: true }));
    }
    assert.equal((await s.request({ method: 'close', task: 'shared', discard: true })).error.code, 'ACTOR_INVALID');
    assert.equal((await s.request(control(s.service, { method: 'stop' }))).error.code, 'TASKS_RETAINED');
    ok(await s.request(control(s.service, { method: 'close', task: 'shared', discard: true })));
    assert(page.isClosed());
  } finally { await s.cleanup(); }
});

test('原执行者退出后，本人能交给等待者；重复名称用区域范围精确填写', required, async () => {
  const s = await setup();
  try {
    ok(await s.request({ method: 'open', task: 'groups', url: s.web.url + 'groups' }));
    const a = await s.actor('groups', '原执行者'); const b = await s.actor('groups', '接续者');
    const leaseA = (await s.claim('groups', a)).lease;
    const all = ok(await s.request({ method: 'observe', task: 'groups', actorId: a, lease: leaseA }));
    assert.equal((await s.request({ method: 'act', task: 'groups', actorId: a, lease: leaseA, sequence: 1, requestId: 'ambiguous', observationId: all.observationId,
      operation: { type: 'fill_form', fields: [{ role: 'textbox', name: '姓名', value: '不能错填' }] } })).error.code, 'TARGET_NOT_OBSERVED');
    assert.equal((await s.claim('groups', b)).queued, true);
    const waiting = s.request({ method: 'wait', task: 'groups', actorId: b });
    assert.equal((await s.request({ method: 'handoff', task: 'groups' })).error.code, 'CONTROL_REQUIRED');
    ok(await s.request(control(s.service, { method: 'handoff', task: 'groups' })));
    const leaseB = ok(await waiting).lease;
    assert(leaseB);
    assert.equal((await s.request({ method: 'observe', task: 'groups', actorId: a, lease: leaseA })).error.code, 'STALE_LEASE');
    const observed = ok(await s.request({ method: 'observe', task: 'groups', actorId: b, lease: leaseB, scope: { role: 'group', name: '第二组' } }));
    ok(await s.request({ method: 'act', task: 'groups', actorId: b, lease: leaseB, sequence: 1, requestId: 'scoped', observationId: observed.observationId,
      operation: { type: 'fill_form', fields: [{ role: 'textbox', name: '姓名', value: '只填第二组' }] } }));
    const page = s.service.broker.tasks.get('groups').resource.page;
    assert.equal(await page.getByRole('group', { name: '第一组' }).getByRole('textbox').inputValue(), '');
    assert.equal(await page.getByRole('group', { name: '第二组' }).getByRole('textbox').inputValue(), '只填第二组');
    const scoped = ok(await s.request({ method: 'observe', task: 'groups', actorId: b, lease: leaseB, scope: { role: 'group', name: '第一组' } }));
    // The selected group itself is an observed target, not only its descendants.
    ok(await s.request({ method: 'act', task: 'groups', actorId: b, lease: leaseB, sequence: 2, requestId: 'scope-root', observationId: scoped.observationId,
      operation: { type: 'click', target: { role: 'group', name: '第一组' } } }));
  } finally { await s.cleanup(); }
});

test('真实资源：三任务占满，释放一个后第四任务继续，其余页与存储保留，最后环境可关闭', required, async () => {
  const s = await setup({ capacity: () => ({ limit: 3, basis: { test: true } }) });
  let observer;
  try {
    await s.open('one'); await s.open('two'); await s.open('three');
    assert.equal((await s.request({ method: 'open', task: 'four', url: s.web.url, wait: false })).error.code, 'CAPACITY_REACHED');
    const one = s.service.broker.tasks.get('one').resource;
    const two = s.service.broker.tasks.get('two').resource;
    const session = await one.context.newCDPSession(one.page);
    const ownId = (await session.send('Target.getTargetInfo')).targetInfo.browserContextId;
    await session.detach();
    const twoCookies = await two.context.cookies();
    observer = await connect();
    const inventory = await observer.newBrowserCDPSession();
    assert((await inventory.send('Target.getBrowserContexts')).browserContextIds.includes(ownId));
    ok(await s.request(control(s.service, { method: 'close', task: 'one', discard: true })));
    assert(!(await inventory.send('Target.getBrowserContexts')).browserContextIds.includes(ownId));
    await s.open('four');
    assert(!two.page.isClosed());
    assert.deepEqual(await two.context.cookies(), twoCookies);
    assert.notDeepEqual(await s.service.broker.tasks.get('four').resource.context.cookies(), twoCookies);
    for (const task of ['two', 'three', 'four']) ok(await s.request(control(s.service, { method: 'close', task, discard: true })));
    assert.equal(s.service.provider.owned.size, 0);
    assert.equal((await inventory.send('Browser.getVersion')).product.startsWith('HeadlessChrome'), true);
    await inventory.detach();
  } finally { if (observer) await observer.close(); await s.cleanup(); }
});

test('真实在途副作用：超时不交接、不重放，核对后同页能继续', required, async () => {
  const s = await setup({ providerOptions: { actionTimeout: 600, navigationTimeout: 600 } });
  try {
    await s.open('uncertain');
    const a = await s.actor('uncertain', 'A'); const b = await s.actor('uncertain', 'B');
    let lease = (await s.claim('uncertain', a)).lease;
    const observation = ok(await s.request({ method: 'observe', task: 'uncertain', actorId: a, lease }));
    const action = s.request({ method: 'act', task: 'uncertain', actorId: a, lease, requestId: 'slow-save', sequence: 1, observationId: observation.observationId,
      operation: { type: 'click', target: { role: 'button', name: '缓慢保存' } } });
    await s.web.waitFor(1);
    assert.equal((await s.claim('uncertain', b)).queued, true);
    assert.equal(ok(await s.request({ method: 'status' })).tasks[0].inFlight, true);
    ok(await s.request({ method: 'yield', task: 'uncertain', actorId: a, lease }));
    assert.equal((await s.request(control(s.service, { method: 'resume', task: 'uncertain', outcome: 'checked' }))).error.code, 'BUSY');
    assert.equal((await s.request(control(s.service, { method: 'close', task: 'uncertain', discard: true }))).error.code, 'BUSY');
    assert.equal(ok(await action).phase, 'outcome_unknown');
    assert.equal(s.web.saves.length, 1);
    assert.equal((await s.request({ method: 'claim', task: 'uncertain', actorId: b })).error.code, 'TASK_PAUSED');
    assert.equal(ok(await s.request({ method: 'receipt', task: 'uncertain', actorId: a, requestId: 'slow-save' })).phase, 'outcome_unknown');
    s.web.releaseSlow();
    const page = s.service.broker.tasks.get('uncertain').resource.page;
    await page.waitForURL('**/slow', { waitUntil: 'domcontentloaded' });
    assert.match(ok(await s.request(control(s.service, { method: 'inspect', task: 'uncertain' }))).snapshot, /已保存 1 次/);
    assert.equal((await s.request({ method: 'resume', task: 'uncertain', outcome: 'checked' })).error.code, 'CONTROL_REQUIRED');
    ok(await s.request(control(s.service, { method: 'resume', task: 'uncertain', outcome: 'checked' })));
    lease = (await s.claim('uncertain', a)).lease;
    ok(await s.request({ method: 'yield', task: 'uncertain', actorId: a, lease }));
    lease = (await s.claim('uncertain', b)).lease;
    const fresh = ok(await s.request({ method: 'observe', task: 'uncertain', actorId: b, lease }));
    ok(await s.request({ method: 'act', task: 'uncertain', actorId: b, lease, sequence: 1, requestId: 'after-review', observationId: fresh.observationId,
      operation: { type: 'fill_form', fields: [{ role: 'textbox', name: '备注', value: '核对后继续' }] } }));
    assert.equal(await page.getByRole('textbox', { name: '备注', exact: true }).inputValue(), '核对后继续');
    assert.equal(s.web.saves.length, 1);
    ok(await s.request(control(s.service, { method: 'close', task: 'uncertain', discard: true })));
  } finally { s.web.releaseSlow(); await s.cleanup(); }
});

test('UTF-8跨数据包保持；同名执行者不共用身份；服务不能重复启动', required, async () => {
  const s = await setup();
  try {
    await s.open('utf8');
    const first = await s.actor('utf8', '同名');
    const raw = Buffer.from(JSON.stringify({ method: 'join', task: 'utf8', label: '同名' }) + '\n');
    const position = raw.indexOf(Buffer.from('同')) + 1;
    const response = await new Promise((resolve, reject) => {
      const socket = net.connect(s.service.socketPath);
      let result = '';
      socket.on('error', reject);
      socket.on('data', chunk => { result += chunk.toString(); if (result.includes('\n')) { socket.end(); resolve(JSON.parse(result)); } });
      socket.on('connect', () => { socket.write(raw.subarray(0, position)); setImmediate(() => socket.write(raw.subarray(position))); });
    });
    assert.equal(ok(response).label, '同名');
    assert.notEqual(response.result.actorId, first);
    await assert.rejects(createService({ home: s.home, connect }), /已运行/);
  } finally { await s.cleanup(); }
});

test('未命名正文区域可以观察，但不能用空名称点击或填写', required, async () => {
  const s = await setup();
  try {
    await s.open('unnamed');
    const actorId = await s.actor('unnamed', '正文读取');
    const lease = (await s.claim('unnamed', actorId)).lease;
    const observed = ok(await s.request({ method: 'observe', task: 'unnamed', actorId, lease, scope: { role: 'main', name: '' } }));
    assert.match(observed.snapshot, /姓名/);
    const response = await s.request({ method: 'act', task: 'unnamed', actorId, lease, requestId: 'empty-target', sequence: 1,
      observationId: observed.observationId, operation: { type: 'click', target: { role: 'button', name: '' } } });
    assert.equal(response.error.code, 'INVALID_REQUEST');
    assert.equal(response.error.phase, 'not_dispatched');
    const fill = await s.request({ method: 'act', task: 'unnamed', actorId, lease, requestId: 'empty-field', sequence: 2,
      observationId: observed.observationId, operation: { type: 'fill_form', fields: [{ role: 'textbox', name: '', value: '不能写入' }] } });
    assert.equal(fill.error.code, 'INVALID_REQUEST');
    assert.equal(fill.error.phase, 'not_dispatched');
  } finally { await s.cleanup(); }
});

test('关键词查找：正文、来源、未命中和实时变化；不保存正文或提供动作观察', required, async () => {
  const s = await setup();
  try {
    ok(await s.request({ method: 'open', task: 'read', url: s.web.url + 'read' }));
    const actorId = await s.actor('read', '资料读取');
    const lease = (await s.claim('read', actorId)).lease;
    const request = { method: 'find', task: 'read', actorId, lease, scope: { role: 'main', name: '' }, queries: ['TAB_GONE', '[a+b]?', 'ARTICLE-PRIVATE-FIXTURE', 'SIDEBAR-ONLY', '没有这个词'] };
    const observed = ok(await s.request({ ...request, method: 'observe' }));
    const state = await readFile(join(s.home, 'state.json'), 'utf8');
    const found = ok(await s.request(request));
    assert.equal(found.url, s.web.url + 'read');
    assert.equal(found.results[0].found, true);
    assert.match(found.results[0].excerpts.join(''), /不会接管另一页/);
    assert.equal(found.results[1].found, true);
    assert.equal(found.results[2].found, true);
    assert.equal(found.results[3].found, false);
    assert.equal(found.results[4].found, false);
    assert.equal(found.observationId, undefined);
    assert.equal((await s.request({ method: 'act', task: 'read', actorId, lease, requestId: 'find-not-action-proof', sequence: 1,
      observationId: observed.observationId, operation: { type: 'click', target: { role: 'button', name: '保存草稿' } } })).error.code, 'STALE_OBSERVATION');
    assert.equal(await readFile(join(s.home, 'state.json'), 'utf8'), state);
    const page = s.service.broker.tasks.get('read').resource.page;
    await page.locator('#live').evaluate(element => { element.textContent = 'ONLY-LIVE-NEW 当前新版本'; });
    const fresh = ok(await s.request({ ...request, queries: ['ARTICLE-PRIVATE-FIXTURE', 'ONLY-LIVE-NEW'] }));
    assert.equal(fresh.results[0].found, false);
    assert.equal(fresh.results[1].found, true);
    for (const queries of [undefined, [], [''], [' '], [42], ['x'.repeat(129)], Array(9).fill('x')]) {
      assert.equal((await s.request({ ...request, queries })).error.code, 'INVALID_REQUEST');
    }
    assert.equal((await s.request({ ...request, cursor: 0 })).error.code, 'INVALID_REQUEST');
  } finally { await s.cleanup(); }
});

test('关键词查找：交接后旧执行者被拒绝，换页后不返回上一页资料', required, async () => {
  const s = await setup();
  try {
    const opened = ok(await s.request({ method: 'open', task: 'read', url: s.web.url + 'read' }));
    const a = await s.actor('read', 'A'), b = await s.actor('read', 'B');
    const leaseA = (await s.claim('read', a)).lease;
    const find = { method: 'find', task: 'read', actorId: a, lease: leaseA, queries: ['tab_gone'] };
    assert.equal(ok(await s.request(find)).results[0].found, true);
    assert.equal((await s.claim('read', b)).queued, true);
    ok(await s.request({ method: 'yield', task: 'read', actorId: a, lease: leaseA }));
    assert.equal((await s.request(find)).error.code, 'STALE_LEASE');
    const claimed = await s.claim('read', b);
    assert.equal(claimed.pageId, opened.pageId);
    assert.equal(ok(await s.request({ ...find, actorId: b, lease: claimed.lease })).results[0].found, true);
    ok(await s.request({ method: 'act', task: 'read', actorId: b, lease: claimed.lease, sequence: 1, requestId: 'next', operation: { type: 'navigate', url: s.web.url + 'read-next' } }));
    const next = ok(await s.request({ ...find, actorId: b, lease: claimed.lease, queries: ['tab_gone', 'ONLY-NEXT-PAGE'] }));
    assert.equal(next.url, s.web.url + 'read-next');
    assert.equal(next.results[0].found, false);
    assert.equal(next.results[1].found, true);
    ok(await s.request(control(s.service, { method: 'pause', task: 'read' })));
    assert.equal((await s.request({ ...find, actorId: b, lease: claimed.lease })).error.code, 'TASK_PAUSED');
  } finally { await s.cleanup(); }
});

test('关键词查找：常见词明确截断，末尾词可读，中文与emoji不损坏', required, async () => {
  const s = await setup();
  try {
    ok(await s.request({ method: 'open', task: 'large', url: s.web.url + 'large' }));
    const actorId = await s.actor('large', '长文');
    const lease = (await s.claim('large', actorId)).lease;
    const result = ok(await s.request({ method: 'find', task: 'large', actorId, lease, queries: ['中文测试🙂', '大页面末尾', '不在页面中'] }));
    assert.equal(result.results[0].found, true);
    assert.equal(result.results[0].truncated, true);
    assert.equal(result.results[1].found, true);
    assert.match(result.results[1].excerpts.join(''), /大页面末尾/);
    assert.equal(result.results[2].found, false);
    const text = result.results.flatMap(item => item.excerpts).join('');
    assert(Buffer.byteLength(text) <= 8192);
    assert(!text.includes('\uFFFD'));
    assert(Array.from(text).every(character => { const point = character.codePointAt(0); return point < 0xd800 || point > 0xdfff; }));
    ok(await s.request({ method: 'act', task: 'large', actorId, lease, sequence: 1, requestId: 'huge', operation: { type: 'navigate', url: s.web.url + 'read-huge' } }));
    const request = { method: 'find', task: 'large', actorId, lease, queries: ['超限正文'] };
    assert.equal((await s.request(request)).error.code, 'READ_TOO_LARGE');
    assert.equal(ok(await s.request({ ...request, scope: { role: 'heading', name: '超限正文' } })).results[0].found, true);
  } finally { await s.cleanup(); }
});

test('已有但无归属的socket路径不删除，也不连接浏览器', { timeout: 5000 }, async () => {
  const home = await mkdtemp(join(tmpdir(), 'sf-owner-'));
  try {
    const sentinel = join(home, 'browser.sock');
    await writeFile(sentinel, '需要保留的原文件');
    await assert.rejects(createService({ home, connect: async () => { throw new Error('不应连接'); } }), /未删除原文件/);
    assert.equal(await readFile(sentinel, 'utf8'), '需要保留的原文件');
  } finally { await rm(home, { recursive: true, force: true }); }
});

test('未连接与已发出后的异常返回区分执行阶段', { timeout: 5000 }, async () => {
  const home = await mkdtemp(join(tmpdir(), 'sf-wire-'));
  let server;
  try {
    await assert.rejects(call(home, { method: 'status' }), error => error.phase === 'not_dispatched');
    server = net.createServer(socket => { socket.on('data', () => socket.end('not-json\n')); });
    await new Promise(resolve => server.listen(join(home, 'browser.sock'), resolve));
    await assert.rejects(call(home, { method: 'act' }), error => error.code === 'INVALID_RESPONSE' && error.phase === 'outcome_unknown');
  } finally { if (server) await new Promise(resolve => server.close(resolve)); await rm(home, { recursive: true, force: true }); }
});

test('存储写入失败时不派发动作；修复存储并明确恢复后可继续', required, async () => {
  const s = await setup();
  try {
    await s.open('storage');
    const actorId = await s.actor('storage', '写入失败测试');
    let lease = (await s.claim('storage', actorId)).lease;
    const observation = ok(await s.request({ method: 'observe', task: 'storage', actorId, lease }));
    await chmod(s.home, 0o500);
    const reply = await s.request({ method: 'act', task: 'storage', actorId, lease, sequence: 1, requestId: 'not-dispatched', observationId: observation.observationId,
      operation: { type: 'fill_form', fields: [{ role: 'textbox', name: '姓名', value: '不应填入' }] } });
    assert.equal(reply.error.code, 'STORAGE_ERROR');
    assert.equal(reply.error.phase, 'not_dispatched');
    const page = s.service.broker.tasks.get('storage').resource.page;
    assert.equal(await page.getByRole('textbox', { name: '姓名' }).inputValue(), '');
    await chmod(s.home, 0o700);
    ok(await s.request(control(s.service, { method: 'resume', task: 'storage', outcome: 'checked' })));
    lease = (await s.claim('storage', actorId)).lease;
    const fresh = ok(await s.request({ method: 'observe', task: 'storage', actorId, lease }));
    ok(await s.request({ method: 'act', task: 'storage', actorId, lease, sequence: 2, requestId: 'after-storage-repair', observationId: fresh.observationId,
      operation: { type: 'fill_form', fields: [{ role: 'textbox', name: '姓名', value: '修复后继续' }] } }));
    assert.equal(await page.getByRole('textbox', { name: '姓名' }).inputValue(), '修复后继续');
  } finally { await chmod(s.home, 0o700); await s.cleanup(); }
});

test('大观察按需分页且UTF-8完整；回执轮转后旧顺序号仍不重新执行', required, async () => {
  const s = await setup({ maxReceipts: 1 });
  try {
    ok(await s.request({ method: 'open', task: 'large', url: s.web.url + 'large' }));
    const actorId = await s.actor('large', '分页测试');
    const lease = (await s.claim('large', actorId)).lease;
    let part = ok(await s.request({ method: 'observe', task: 'large', actorId, lease }));
    let joined = part.snapshot;
    assert(part.nextCursor !== null);
    while (part.nextCursor !== null) {
      const response = await s.request({ method: 'observe', task: 'large', actorId, lease, cursor: part.nextCursor, observationId: part.observationId });
      assert(Buffer.byteLength(JSON.stringify(response)) < 32768);
      part = ok(response); joined += part.snapshot;
    }
    assert(!joined.includes('\ufffd'));
    assert.match(joined, /大页面末尾/);
    const first = { method: 'act', task: 'large', actorId, lease, sequence: 1, requestId: 'nav-one', operation: { type: 'navigate', url: s.web.url } };
    ok(await s.request(first));
    assert.equal((await s.request({ method: 'observe', task: 'large', actorId, lease, cursor: 0, observationId: part.observationId })).error.code, 'STALE_OBSERVATION');
    ok(await s.request({ ...first, sequence: 2, requestId: 'nav-two' }));
    assert.equal((await s.request(first)).error.code, 'SEQUENCE_REUSED');
    const current = ok(await s.request({ method: 'observe', task: 'large', actorId, lease }));
    assert.equal((await s.request({ method: 'act', task: 'large', actorId, lease, sequence: 3, requestId: 'no-js', observationId: current.observationId, operation: { type: 'evaluate', script: '1+1' } })).error.code, 'UNSUPPORTED_OPERATION');
    assert.equal((await s.request({ method: 'act', task: 'large', actorId, lease, sequence: 3, requestId: 'no-target', observationId: current.observationId,
      operation: { type: 'click', target: { role: 'button', name: '没有这个按钮' } } })).error.code, 'TARGET_NOT_OBSERVED');
  } finally { await s.cleanup(); }
});

async function startWorker(home) {
  const child = spawn(process.execPath, [fileURLToPath(new URL('./worker.mjs', import.meta.url))], {
    env: { ...process.env, SHIFU_WEB_HOME: home }, stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stderr = '';
  child.stderr.on('data', chunk => { stderr += chunk.toString(); });
  await new Promise((resolve, reject) => {
    let text = '';
    const timer = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('worker未就绪')); }, 10000);
    child.on('exit', code => { clearTimeout(timer); if (code !== null) reject(new Error(`worker退出 ${code}: ${stderr.slice(0, 600)}`)); });
    child.stdout.on('data', chunk => { text += chunk.toString(); if (text.includes('\n')) { clearTimeout(timer); assert(JSON.parse(text.split('\n')[0]).ready); resolve(); } });
  });
  return child;
}

test('真实服务崩溃：未决记录保留、临时页丢失不伪造恢复、核对后新任务继续且原保存不重放', required, async () => {
  const home = await mkdtemp(join(tmpdir(), 'sf-crash-'));
  const web = await fixture();
  let worker;
  let observer;
  const request = value => call(home, value);
  const admin = async value => request({ ...value, controlToken: JSON.parse(await readFile(join(home, 'control.json'), 'utf8')).token });
  try {
    worker = await startWorker(home);
    ok(await request({ method: 'open', task: 'crash', url: web.url }));
    const actorId = ok(await request({ method: 'join', task: 'crash', label: '崩溃测试A' })).actorId;
    const lease = ok(await request({ method: 'claim', task: 'crash', actorId })).lease;
    const observationId = ok(await request({ method: 'observe', task: 'crash', actorId, lease })).observationId;
    const pending = request({ method: 'act', task: 'crash', actorId, lease, observationId, requestId: 'crash-save', sequence: 1,
      operation: { type: 'click', target: { role: 'button', name: '缓慢保存' } } }).catch(error => ({ connectionError: error.code, phase: error.phase }));
    await web.waitFor(1);
    const before = JSON.parse(await readFile(join(home, 'state.json'), 'utf8'));
    assert.equal(before.tasks[0].pending.requestId, 'crash-save');
    observer = await connect();
    const inventory = await observer.newBrowserCDPSession();
    const info = (await inventory.send('Target.getTargets')).targetInfos.find(target => target.url === web.url);
    assert(info?.browserContextId);
    await inventory.send('Target.setDiscoverTargets', { discover: true });
    const destroyed = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('崩溃客户端的原页面未关闭')), 5000);
      inventory.on('Target.targetDestroyed', event => { if (event.targetId === info.targetId) { clearTimeout(timer); resolve(); } });
    });
    worker.kill('SIGKILL'); await once(worker, 'exit'); assert.equal((await pending).phase, 'outcome_unknown'); await destroyed;
    assert(!(await inventory.send('Target.getBrowserContexts')).browserContextIds.includes(info.browserContextId));
    worker = await startWorker(home);
    const status = ok(await request({ method: 'status' }));
    assert.equal(status.tasks.length, 0);
    assert.equal(status.recovery[0].pending.phase, 'outcome_unknown');
    assert.equal(web.saves.length, 1);
    assert.equal((await request({ method: 'open', task: 'crash', url: web.url })).error.code, 'TASK_EXISTS');
    assert.equal((await request({ method: 'resolve_lost', task: 'crash', outcome: 'checked' })).error.code, 'CONTROL_REQUIRED');
    ok(await admin({ method: 'resolve_lost', task: 'crash', outcome: 'checked' }));
    ok(await request({ method: 'open', task: 'crash', url: web.url }));
    assert.equal((await request({ method: 'observe', task: 'crash', actorId, lease })).error.code, 'ACTOR_INVALID');
    const actorB = ok(await request({ method: 'join', task: 'crash', label: '新执行者' })).actorId;
    const leaseB = ok(await request({ method: 'claim', task: 'crash', actorId: actorB })).lease;
    const observed = ok(await request({ method: 'observe', task: 'crash', actorId: actorB, lease: leaseB }));
    ok(await request({ method: 'act', task: 'crash', actorId: actorB, lease: leaseB, sequence: 1, requestId: 'new-after-crash', observationId: observed.observationId,
      operation: { type: 'click', target: { role: 'button', name: '保存草稿' } } }));
    await web.waitFor(2);
    assert.equal(web.saves.length, 2);
    ok(await admin({ method: 'close', task: 'crash', discard: true }));
    const exit = once(worker, 'exit');
    ok(await admin({ method: 'stop' })); await exit; worker = null;
    assert((await inventory.send('Browser.getVersion')).product.startsWith('HeadlessChrome'));
    await inventory.detach();
  } finally {
    if (worker) { const exit = once(worker, 'exit'); worker.kill('SIGKILL'); await exit; }
    web.releaseSlow(); if (observer) await observer.close(); await web.close(); await rm(home, { recursive: true, force: true });
  }
});

test('机器容量：按可用内存和核数算上限，环境变量可覆盖', { timeout: 5000 }, async () => {
  const GiB = 2 ** 30;
  assert.equal(machineCapacity({ available: 22 * GiB, cpus: 10 }).limit, 10);
  assert.equal(machineCapacity({ available: 3 * GiB, cpus: 10 }).limit, 3);
  assert.equal(machineCapacity({ available: 0.1 * GiB, cpus: 10 }).limit, 1);
  assert.deepEqual(readCapacity({ SHIFU_WEB_MAX_TASKS: '5' }).limit, 5);
  assert.throws(() => readCapacity({ SHIFU_WEB_MAX_TASKS: 'abc' }));
});

test('并行查资料：满额时open排队，执行者查完自己释放，排队任务接着打开', required, async () => {
  const s = await setup({ capacity: () => ({ limit: 1, basis: { test: true } }) });
  try {
    await s.open('first');
    const waiting = s.request({ method: 'open', task: 'second', url: s.web.url });
    const a = await s.actor('first', 'A');
    const lease = (await s.claim('first', a)).lease;
    assert.equal((await s.request({ method: 'close', task: 'first', actorId: a, lease })).error.code, 'CONFIRM_DISCARD');
    const b = await s.actor('first', 'B');
    assert.equal((await s.request({ method: 'close', task: 'first', actorId: b, lease, discard: true })).error.code, 'STALE_LEASE');
    const page = s.service.broker.tasks.get('first').resource.page;
    ok(await s.request({ method: 'close', task: 'first', actorId: a, lease, discard: true }));
    assert(page.isClosed());
    const second = ok(await waiting);
    assert.equal(second.task, 'second');
    const status = ok(await s.request({ method: 'status' }));
    assert.equal(status.capacity.limit, 1);
    assert.equal(status.capacity.used, 1);
    ok(await s.request(control(s.service, { method: 'close', task: 'second', discard: true })));
  } finally { await s.cleanup(); }
});

test('慢页：正文先到但页面一直没加载完，打开和跳转仍成功并标明未加载完，可读已到正文', required, async () => {
  const s = await setup({ providerOptions: { actionTimeout: 1000, navigationTimeout: 3000, settleTimeout: 500 } });
  try {
    const opened = ok(await s.request({ method: 'open', task: 'stalled', url: s.web.url + 'stalled' }));
    assert.equal(opened.loaded, false);
    const a = await s.actor('stalled', 'A');
    const lease = (await s.claim('stalled', a)).lease;
    const found = ok(await s.request({ method: 'find', task: 'stalled', actorId: a, lease, queries: ['STALLED-BEFORE-SCRIPT'] }));
    assert.equal(found.results[0].found, true);
    const moved = ok(await s.request({ method: 'act', task: 'stalled', actorId: a, lease, requestId: 'nav-1', sequence: 1,
      operation: { type: 'navigate', url: s.web.url + 'stalled' } }));
    assert.equal(moved.phase, 'completed');
    assert.equal(moved.loaded, false);
    s.web.releaseSlow();
    ok(await s.request({ method: 'close', task: 'stalled', actorId: a, lease, discard: true }));
  } finally { s.web.releaseSlow(); await s.cleanup(); }
});

test('跳转无响应：只读跳转失败不锁任务，执行者可换页继续并自行释放', required, async () => {
  const s = await setup({ providerOptions: { actionTimeout: 1000, navigationTimeout: 800, settleTimeout: 300 } });
  try {
    ok(await s.request({ method: 'open', task: 'hang', url: s.web.url + 'read' }));
    const a = await s.actor('hang', 'A');
    const lease = (await s.claim('hang', a)).lease;
    const failed = ok(await s.request({ method: 'act', task: 'hang', actorId: a, lease, requestId: 'nav-1', sequence: 1,
      operation: { type: 'navigate', url: s.web.url + 'slow' } }));
    assert.equal(failed.phase, 'navigation_failed');
    assert.equal(ok(await s.request({ method: 'status' })).tasks[0].state, 'active');
    const moved = ok(await s.request({ method: 'act', task: 'hang', actorId: a, lease, requestId: 'nav-2', sequence: 2,
      operation: { type: 'navigate', url: s.web.url + 'read-next' } }));
    assert.equal(moved.phase, 'completed');
    assert.equal(ok(await s.request({ method: 'find', task: 'hang', actorId: a, lease, queries: ['ONLY-NEXT-PAGE'] })).results[0].found, true);
    ok(await s.request({ method: 'close', task: 'hang', actorId: a, lease, discard: true }));
  } finally { s.web.releaseSlow(); await s.cleanup(); }
});

test('头部资源卡住：页面主体未建成时，查找改读已到的HTML正文；观察返回明确的未就绪', required, async () => {
  const s = await setup({ providerOptions: { actionTimeout: 800, navigationTimeout: 3000, settleTimeout: 300 } });
  try {
    assert.equal(ok(await s.request({ method: 'open', task: 'head', url: s.web.url + 'blocked-head' })).loaded, false);
    const a = await s.actor('head', 'A');
    const lease = (await s.claim('head', a)).lease;
    const found = ok(await s.request({ method: 'find', task: 'head', actorId: a, lease, queries: ['HEAD-BLOCKED-TEXT', 'NAV-ONLY', 'HIDDEN_SCRIPT_TEXT'], scope: { role: 'main', name: '' } }));
    assert.equal(found.source, 'html');
    assert.deepEqual(found.results.map(result => result.found), [true, false, false]);
    const whole = ok(await s.request({ method: 'find', task: 'head', actorId: a, lease, queries: ['NAV-ONLY'] }));
    assert.equal(whole.results[0].found, true);
    const observed = await s.request({ method: 'observe', task: 'head', actorId: a, lease });
    assert.equal(observed.error.code, 'PAGE_NOT_READY');
    assert.notEqual(observed.error.phase, 'outcome_unknown');
    assert.equal(ok(await s.request({ method: 'status' })).tasks[0].state, 'active');
    ok(await s.request({ method: 'close', task: 'head', actorId: a, lease, discard: true }));
  } finally { s.web.releaseSlow(); await s.cleanup(); }
});

test('读取失败是普通失败：范围不存在时不报未知结果，任务继续可用', required, async () => {
  const s = await setup({ providerOptions: { actionTimeout: 500 } });
  try {
    ok(await s.request({ method: 'open', task: 'scope', url: s.web.url + 'read-next' }));
    const a = await s.actor('scope', 'A');
    const lease = (await s.claim('scope', a)).lease;
    const failed = await s.request({ method: 'find', task: 'scope', actorId: a, lease, queries: ['x'], scope: { role: 'region', name: '不存在' } });
    assert.equal(failed.error.code, 'READ_FAILED');
    assert.equal(failed.error.phase, 'not_dispatched');
    assert.equal(ok(await s.request({ method: 'find', task: 'scope', actorId: a, lease, queries: ['ONLY-NEXT-PAGE'] })).results[0].found, true);
    ok(await s.request({ method: 'close', task: 'scope', actorId: a, lease, discard: true }));
  } finally { await s.cleanup(); }
});
