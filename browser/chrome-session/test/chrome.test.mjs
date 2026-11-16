import test from 'node:test';
import assert from 'node:assert/strict';
import { PassThrough } from 'node:stream';
import { mkdtemp, rm, readFile, writeFile, mkdir, stat } from 'node:fs/promises';
import { tmpdir, endianness } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { setTimeout as delay } from 'node:timers/promises';
import http from 'node:http';
import { NativeTransport, nativeFrame } from '../native-transport.mjs';
import { prepareChrome, extensionId } from '../chrome-setup.mjs';
import { call } from '../service.mjs';

const ok = response => { assert.equal(response.ok, true, JSON.stringify(response)); return response.result; };
const event = () => ({ listeners: [], addListener(fn) { this.listeners.push(fn); }, emit(...args) { this.listeners.forEach(fn => fn(...args)); } });
async function until(check, timeoutMs = 5000) {
  const deadline = Date.now() + timeoutMs;
  do { if (await check()) return; await delay(25); } while (Date.now() < deadline);
  throw new Error('等待测试条件超时');
}

test('native messaging按字节分帧，UTF8不丢失；断线拒绝在途请求且不重发', async () => {
  const input = new PassThrough(); const output = new PassThrough();
  const frames = []; output.on('data', chunk => frames.push(chunk));
  const transport = new NativeTransport(input, output, { timeoutMs: 1000 });
  const binding = nativeFrame({ kind: 'bindings', bindings: [{ id: 'chrome_7', tabId: 7, windowId: 2, url: 'https://example.test/', grant: '中文授权' }] });
  for (const byte of binding) input.write(Buffer.from([byte]));
  assert.equal(transport.bindings.get('chrome_7').grant, '中文授权');
  const request = transport.request(7, 'tab_info', {}, '中文授权');
  const sent = JSON.parse(frames[0].subarray(4));
  input.write(nativeFrame({ kind: 'reply', id: sent.id, result: { url: 'https://example.test/中文' } }));
  assert.equal((await request).url, 'https://example.test/中文');
  const pending = transport.request(7, 'Runtime.callFunctionOn', {}, '中文授权');
  input.end();
  await assert.rejects(pending, error => error.code === 'CHROME_DISCONNECTED' && error.phase === 'outcome_unknown');
  assert.equal(frames.length, 2);
  assert.equal(transport.bindings.size, 0);
  output.destroy();
});

test('native messaging拒绝超长帧，不保留连接', () => {
  const input = new PassThrough(); const output = new PassThrough();
  const transport = new NativeTransport(input, output);
  const header = Buffer.alloc(4);
  if (endianness() === 'LE') header.writeUInt32LE(1024 * 1024 + 1); else header.writeUInt32BE(1024 * 1024 + 1);
  input.write(header);
  assert.equal(transport.closed, true);
  output.destroy();
  for (const value of [null, { kind: 'bindings', bindings: [null] }]) {
    const malformed = new PassThrough(); const sink = new PassThrough();
    const peer = new NativeTransport(malformed, sink);
    malformed.write(nativeFrame(value));
    assert.equal(peer.closed, true);
    sink.destroy();
  }
});

test('准备的是自含本地组件，无Chrome设置写入、无依赖安装、无覆盖旧候选', async t => {
  const root = await mkdtemp(join(tmpdir(), 'sf-c-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const prepared = await prepareChrome({ output: join(root, "component space's"), home: join(root, 'home') });
  assert.equal(prepared.installed, false);
  const manifest = JSON.parse(await readFile(prepared.nativeManifest));
  assert.deepEqual(manifest.allowed_origins, [`chrome-extension://${await extensionId()}/`]);
  assert.equal((await stat(manifest.path)).mode & 0o777, 0o700);
  await assert.rejects(prepareChrome({ output: join(root, "component space's") }), { code: 'EEXIST' });
  const child = spawn(process.execPath, [prepared.runtime, 'help']);
  let text = ''; child.stdout.on('data', chunk => { text += chunk; });
  const [code] = await once(child, 'exit');
  assert.equal(code, 0); assert.match(text, /chrome-prepare/);
});

test('一次接入暂停状态跨扩展重新加载保持，不启动本地进程', async () => {
  const original = globalThis.chrome;
  let starts = 0; let title = '';
  globalThis.chrome = {
    storage: { local: { get: async () => ({ autoConnectEnabled: false }), set: async () => {} } },
    action: { onClicked: event(), setBadgeText: async () => {}, setTitle: async value => { title = value.title; } },
    runtime: { onStartup: event(), onInstalled: event(), connectNative: () => { starts++; throw new Error('must not start'); } },
    debugger: { onDetach: event() }, tabs: { onRemoved: event() },
  };
  try {
    await import(new URL('../chrome/background.mjs?paused-state', import.meta.url));
    await until(() => title.includes('已暂停'));
    assert.equal(starts, 0);
  } finally { globalThis.chrome = original; }
});

const connector = process.env.SHIFU_TEST_CONNECTOR;
test('一次开启+独立进程+真实CDP：新任务免点击接入、准确URL、三页和释放复用、保存与停止', {
  skip: !connector && '需要共享浏览器连接器；未执行浏览器链路', timeout: 45000,
}, async t => {
  const root = await mkdtemp(join(tmpdir(), 'sf-c-'));
  const home = join(root, 'home');
  await mkdir(home);
  const file = join(root, 'cv.txt');
  const fileText = 'APPROVED-CV-FIXTURE';
  await writeFile(file, fileText);
  const prepared = await prepareChrome({ output: join(root, "component space's"), home });
  const saves = [];
  const server = http.createServer(async (request, response) => {
    if (request.method === 'POST') {
      let body = ''; for await (const chunk of request) body += chunk;
      saves.push(JSON.parse(body)); response.end('{}');
    } else {
      response.setHeader('content-type', 'text/html; charset=utf-8');
      response.end(`<!doctype html><main><label>Name<input id="name"></label>
        <label>Study<select id="study"><option>Economics</option><option>Data Science</option></select></label>
        <label>CV<input id="cv" type="file"></label><label>Password<input type="password"></label>
        <button id="save">Save</button><p role="status" aria-label="Save status">Ready</p></main>
        <script>document.cookie='synthetic-session=kept;path=/';document.querySelector('#save').onclick=async()=>{
        const body={name:document.querySelector('#name').value,study:document.querySelector('#study').value,
        file:await document.querySelector('#cv').files[0]?.text()};
        await fetch('/save',{method:'POST',body:JSON.stringify(body)});document.querySelector('[role=status]').textContent='Saved';};</script>`);
    }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const url = `http://127.0.0.1:${server.address().port}/`;
  const browser = await createRequire(import.meta.url)(connector).connect();
  const context = await browser.newContext();
  const pages = new Map(); const sessions = new Map();
  for (const id of [11, 12, 13, 14]) { const page = await context.newPage(); await page.goto(url); pages.set(id, page); }
  let native; let nativeExit; let lastPort;
  const storageValues = {};
  const oldChrome = globalThis.chrome;
  const action = { onClicked: event(), setBadgeText: async () => {}, setTitle: async () => {} };
  const debuggerApi = { onDetach: event(),
    attach: async ({ tabId }) => { sessions.set(tabId, await context.newCDPSession(pages.get(tabId))); },
    detach: async ({ tabId }) => { if (sessions.has(tabId)) { await sessions.get(tabId).detach(); sessions.delete(tabId); } debuggerApi.onDetach.emit({ tabId }); },
    sendCommand: ({ tabId }, method, params) => sessions.get(tabId).send(method, params),
  };
  // Only the Chrome extension API/launcher is doubled. The extension worker,
  // framed stdio, packaged native process, socket, broker and DOM/CDP are real.
  globalThis.chrome = { action, debugger: debuggerApi,
    storage: { local: { get: async key => ({ [key]: storageValues[key] }), set: async value => Object.assign(storageValues, value) } }, tabs: {
    // No url: the production worker must work without activeTab/tabs permission.
    get: async id => ({ id, windowId: 1 }), onRemoved: event(),
  }, runtime: { onStartup: event(), onInstalled: event(), connectNative() {
    native = spawn(join(root, "component space's", 'launch-host'), [`chrome-extension://${prepared.extensionId}/`]);
    nativeExit = once(native, 'exit');
    const port = { onMessage: event(), onDisconnect: event(), postMessage: value => native.stdin.write(nativeFrame(value)),
      disconnect: () => native.stdin.end() };
    lastPort = port;
    let buffer = Buffer.alloc(0);
    native.stdout.on('data', chunk => {
      buffer = Buffer.concat([buffer, chunk]);
      while (buffer.length >= 4) {
        const n = endianness() === 'LE' ? buffer.readUInt32LE() : buffer.readUInt32BE();
        if (buffer.length < n + 4) break;
        const message = JSON.parse(buffer.subarray(4, n + 4)); buffer = buffer.subarray(n + 4);
        port.onMessage.emit(message);
      }
    });
    native.once('exit', () => port.onDisconnect.emit());
    return port;
  } } };
  t.after(async () => {
    if (lastPort && native?.exitCode === null) {
      lastPort.onMessage.emit({ kind: 'command', id: 'cleanup-mode', method: 'bridge_shutdown' });
      await until(() => storageValues.autoConnectEnabled === false).catch(() => {});
    }
    if (native && native.exitCode === null) { native.stdin.end(); await Promise.race([nativeExit, delay(3000)]); if (native.exitCode === null) native.kill('SIGTERM'); }
    globalThis.chrome = oldChrome;
    await context.close(); await browser.close(); server.closeAllConnections();
    await new Promise(resolve => server.close(resolve)); await rm(root, { recursive: true, force: true });
  });
  await import(pathToFileURL(join(prepared.extensionPath, 'background.mjs')).href);
  await until(async () => { try { return ok(await call(home, { method: 'status' })).connection.autoConnectEnabled; } catch { return false; } });
  // The persistent connection is already running when a later task approves a file.
  await writeFile(join(home, 'files.json'), JSON.stringify({ cv: { path: file, sha256: createHash('sha256').update(fileText).digest('hex') } }), { mode: 0o600 });
  const request = value => call(home, value);
  assert.equal((await request({ method: 'attach', task: 'missing-url', bindingId: 'chrome_11' })).error.code, 'EXPECTED_URL_REQUIRED');
  assert.equal((await request({ method: 'attach', task: 'wrong-url', bindingId: 'chrome_11', expectedUrl: url + 'different' })).error.code, 'PAGE_CHANGED');
  assert.equal(sessions.size, 0);
  assert.equal(ok(await request({ method: 'bindings' })).bindings.length, 0);
  const attach = (task, id) => request({ method: 'attach', task, bindingId: `chrome_${id}`, expectedUrl: pages.get(id).url() });
  ok(await attach('demo', 11));
  const actorId = ok(await request({ method: 'join', task: 'demo', label: 'owner' })).actorId;
  let lease = ok(await request({ method: 'claim', task: 'demo', actorId })).lease;
  const control = JSON.parse(await readFile(join(home, 'control.json'))).token;
  const resume = async () => {
    ok(await request({ method: 'resume', task: 'demo', outcome: 'checked', controlToken: control }));
    lease = ok(await request({ method: 'claim', task: 'demo', actorId })).lease;
  };
  let sequence = 0;
  const observe = async () => ok(await request({ method: 'observe', task: 'demo', actorId, lease }));
  const act = async operation => {
    const observed = await observe();
    const value = { method: 'act', task: 'demo', actorId, lease, observationId: observed.observationId,
      requestId: `action-${++sequence}`, sequence, operation };
    return { request: value, result: ok(await request(value)) };
  };
  const name = { role: 'textbox', name: 'Name' };
  const save = { role: 'button', name: 'Save' };
  await act({ type: 'fill_form', fields: [{ ...name, value: 'PRIVATE-FIXTURE-NAME' }] });
  await act({ type: 'select_option', target: { role: 'combobox', name: 'Study' }, label: 'Data Science' });
  const upload = await act({ type: 'upload_file', target: { role: 'button', name: 'CV' }, fileRef: 'cv' });
  assert.equal(upload.result.verification.draftSaved, 'not_checked');
  const saved = await act({ type: 'click', target: save, expect: { target: { role: 'status', name: 'Save status' }, text: 'Saved' } });
  assert.equal(saved.result.verification.scope, 'declared_condition');
  assert.deepEqual(saves, [{ name: 'PRIVATE-FIXTURE-NAME', study: 'Data Science', file: fileText }]);
  assert.equal(ok(await request(saved.request)).duplicate, true);
  assert.equal(saves.length, 1);
  const excerpt = ok(await request({ method: 'find', task: 'demo', actorId, lease, queries: ['Saved'] }));
  assert.equal(excerpt.results[0].found, true);
  const credentials = await act({ type: 'fill_form', fields: [{ ...name, value: 'MUST-NOT-CHANGE' },
    { role: 'textbox', name: 'Password', value: 'MUST-NOT-FILL' }] });
  assert.equal(credentials.result.phase, 'not_dispatched');
  assert.equal(credentials.result.code, 'HUMAN_REQUIRED');
  assert.equal(await pages.get(11).getByRole('textbox', { name: 'Name', exact: true }).inputValue(), 'PRIVATE-FIXTURE-NAME');
  await resume();
  const near = await act({ type: 'select_option', target: { role: 'combobox', name: 'Study' }, label: 'Data Processing' });
  assert.equal(near.result.phase, 'not_dispatched'); assert.equal(near.result.code, 'EXACT_OPTION_REQUIRED');
  await resume();
  await writeFile(file, 'CHANGED');
  const changed = await act({ type: 'upload_file', target: { role: 'button', name: 'CV' }, fileRef: 'cv' });
  assert.equal(changed.result.phase, 'not_dispatched'); assert.equal(changed.result.code, 'FILE_CHANGED');
  await resume();
  await writeFile(file, fileText);
  await act({ type: 'upload_file', target: { role: 'button', name: 'CV' }, fileRef: 'cv' });
  const unknown = await act({ type: 'click', target: save,
    expect: { target: { role: 'status', name: 'Save status' }, text: 'Never appears' } });
  assert.equal(unknown.result.phase, 'outcome_unknown'); assert.equal(saves.length, 2);
  await resume();
  assert.equal(ok(await request({ ...unknown.request, lease })).duplicate, true);
  assert.equal(saves.length, 2);
  ok(await request({ method: 'pause', task: 'demo', controlToken: control }));
  assert.equal((await request({ method: 'observe', task: 'demo', actorId, lease })).error.code, 'TASK_PAUSED');
  ok(await request({ method: 'resume', task: 'demo', outcome: 'checked', controlToken: control }));
  lease = ok(await request({ method: 'claim', task: 'demo', actorId })).lease;
  const observed = await observe();
  await pages.get(11).reload();
  const stale = ok(await request({ method: 'act', task: 'demo', actorId, lease, observationId: observed.observationId,
    requestId: `action-${++sequence}`, sequence, operation: { type: 'fill_form', fields: [{ ...name, value: 'MUST-NOT-BE-FILLED' }] } }));
  assert.equal(stale.phase, 'not_dispatched'); assert.equal(stale.code, 'PAGE_CHANGED');
  assert.equal(await pages.get(11).getByRole('textbox', { name: 'Name', exact: true }).inputValue(), '');
  ok(await attach('second', 12)); ok(await attach('third', 13));
  assert.equal((await attach('fourth', 14)).error.code, 'CAPACITY_REACHED');
  assert.equal(ok(await request({ method: 'bindings' })).bindings.length, 3);
  assert.equal(sessions.has(14), false);
  ok(await request({ method: 'close', task: 'second', controlToken: control }));
  assert.equal(pages.get(12).isClosed(), false);
  ok(await attach('fourth', 14));
  await resume();
  await sessions.get(11).detach(); sessions.delete(11);
  debuggerApi.onDetach.emit({ tabId: 11 });
  await until(async () => !ok(await request({ method: 'bindings' })).bindings.some(b => b.tabId === 11));
  assert.equal((await request({ method: 'observe', task: 'demo', actorId, lease })).error.code, 'TAB_AUTHORIZATION_CHANGED');
  ok(await request({ method: 'pause', task: 'demo', controlToken: control }));
  assert(!(await readFile(join(home, 'state.json'), 'utf8')).includes('PRIVATE-FIXTURE-NAME'));
  ok(await request({ method: 'close', task: 'demo', outcome: 'checked', controlToken: control }));
  assert.equal((await attach('stopped', 11)).error.code, 'TAB_STOPPED');
  ok(await request({ method: 'close', task: 'third', controlToken: control }));
  ok(await request({ method: 'close', task: 'fourth', controlToken: control }));
  assert.equal(ok(await request({ method: 'bindings' })).bindings.length, 0);
  assert.equal(ok(await request({ method: 'status' })).connection.autoConnectEnabled, true);
  action.onClicked.emit();
  await nativeExit;
  assert.equal(storageValues.autoConnectEnabled, false);
  action.onClicked.emit();
  await until(async () => { try { return ok(await request({ method: 'status' })).connection.autoConnectEnabled; } catch { return false; } });
  ok(await attach('after-global-enable', 11));
  const nextControl = JSON.parse(await readFile(join(home, 'control.json'))).token;
  ok(await request({ method: 'close', task: 'after-global-enable', controlToken: nextControl }));
  ok(await request({ method: 'stop', controlToken: nextControl }));
  await nativeExit;
  assert.equal(storageValues.autoConnectEnabled, false);
  assert.equal(pages.get(11).isClosed(), false);
  assert((await context.cookies()).some(cookie => cookie.name === 'synthetic-session' && cookie.value === 'kept'));
  const journal = JSON.parse(await readFile(join(home, 'state.json')));
  assert.equal(journal.tasks.length, 0);
  if (process.env.SHIFU_TEST_ARTIFACT_DIR) {
    await mkdir(process.env.SHIFU_TEST_ARTIFACT_DIR, { recursive: true });
    await writeFile(join(process.env.SHIFU_TEST_ARTIFACT_DIR, 'chrome-bridge.json'), JSON.stringify({
      realBrowserEngine: true, actualChromeExtensionInstall: false, nativeMessagingLaunchDoubled: true,
      productionWorker: true, independentPackagedNodeHost: true, fileContentMatchedAtServer: true,
      saveCount: saves.length, unknownSaveNotReplayed: true, staleReloadRejected: true,
      passwordAndChangedFileRejectedBeforeInput: true, revokedGrantRejected: true,
      onceEnabledAutoConnect: true, perPageClicks: 0, exactUrlRequired: true,
      globalPauseResume: true, releasedSlotReused: true, maxBindings: 3, pageAndSyntheticCookiePreservedOnDisconnect: true,
    }, null, 2) + '\n');
  }
});
