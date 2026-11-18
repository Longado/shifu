import net from 'node:net';
import { randomUUID, randomBytes } from 'node:crypto';
import { mkdir, open, readFile, writeFile, unlink, lstat, chmod } from 'node:fs/promises';
import { join } from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import { Broker, MAX_REQUEST, MAX_RESULT } from './broker.mjs';
import { BrowserError, PlaywrightProvider } from './playwright.mjs';

export async function createService({ home, endpoint, connect, providerOptions, maxReceipts, capacity }) {
  await mkdir(home, { recursive: true, mode: 0o700 });
  if (!(await lstat(home)).isDirectory()) throw new Error('运行目录必须是真实目录。');
  const socketPath = join(home, 'browser.sock');
  if (Buffer.byteLength(socketPath) > 100) throw new Error('运行目录路径过长，请用较短的SHIFU_WEB_HOME。');
  const lockPath = join(home, 'runtime.lock');
  const marker = { kind: 'shifu-browser-v1', pid: process.pid, nonce: randomUUID() };
  try {
    const previous = JSON.parse(await readFile(lockPath, 'utf8'));
    if (previous.kind !== marker.kind || !Number.isInteger(previous.pid)) throw new Error('运行锁格式不明，未触碰原文件。');
    let alive = true;
    try { process.kill(previous.pid, 0); } catch (error) { if (error.code === 'ESRCH') alive = false; else throw error; }
    if (alive) throw new Error('网页服务已运行；未重复启动或删除其socket。');
    try {
      if (!(await lstat(socketPath)).isSocket()) throw new Error('socket路径不是旧服务的socket，未删除。');
      await unlink(socketPath);
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    await unlink(lockPath);
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  const lock = await open(lockPath, 'wx', 0o600);
  await lock.writeFile(JSON.stringify(marker));
  await lock.close();
  const controlToken = randomBytes(32).toString('hex');
  let provider;
  let server;
  let bound = false;
  const cleanup = async () => {
    if (server?.listening) await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    if (provider) await provider.disconnect();
    const current = JSON.parse(await readFile(lockPath, 'utf8'));
    if (current.nonce === marker.nonce) {
      if (bound) {
        try { await unlink(socketPath); } catch (error) { if (error.code !== 'ENOENT') throw error; }
      }
      await unlink(lockPath);
    }
  };
  try {
    try {
      await lstat(socketPath);
      throw new Error('socket路径已存在且没有可确认的旧实例归属，未删除原文件。');
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    await writeFile(join(home, 'control.json'), JSON.stringify({ token: controlToken }), { mode: 0o600 });
    provider = new PlaywrightProvider(connect ?? (async () => {
      const url = new URL(endpoint);
      if (!['ws:', 'wss:'].includes(url.protocol) || !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname)) {
        throw new BrowserError('ENDPOINT_INVALID', '最小版只连接本机Playwright无界面端点。');
      }
      let chromium;
      try { ({ chromium } = await import('playwright-core')); }
      catch { throw new BrowserError('DEPENDENCY_MISSING', '未安装网页运行依赖。源码目录执行npm --prefix web ci --ignore-scripts，或使用完整候选包。'); }
      return chromium.connect(endpoint, { timeout: 8000 }).catch(() => {
        throw new BrowserError('CONNECTION_UNAVAILABLE', '未连上本机Playwright端点；检查服务、地址和客户端版本，不会自行启动浏览器。');
      });
    }), providerOptions);
    const broker = new Broker({ home, provider, controlToken, maxReceipts, capacity });
    await broker.init();
    let closedResolve;
    const closed = new Promise(resolve => { closedResolve = resolve; });
    let stopping = false;
    const stop = async () => {
      if (stopping) return;
      stopping = true;
      await cleanup();
      closedResolve();
    };
    server = net.createServer(socket => {
      let size = 0;
      let buffer = '';
      const decoder = new StringDecoder('utf8');
      let accepted = false;
      socket.setTimeout(45000, () => socket.destroy());
      socket.on('error', () => {}); // A disconnected client does not cancel/replay its dispatched action.
      socket.on('data', chunk => {
        if (accepted) return;
        size += chunk.length;
        if (size > MAX_REQUEST) { socket.end(JSON.stringify({ ok: false, error: { code: 'REQUEST_TOO_LARGE', phase: 'not_dispatched' } }) + '\n'); accepted = true; return; }
        buffer += decoder.write(chunk);
        const end = buffer.indexOf('\n');
        if (end < 0) return;
        accepted = true;
        void (async () => {
          let response;
          let shouldStop = false;
          try {
            const request = JSON.parse(buffer.slice(0, end));
            if (request.method === 'stop') {
              broker.control(request);
              if (broker.tasks.size > 0) throw new BrowserError('TASKS_RETAINED', '仍有任务页；先逐项复核释放，不自动关闭草稿。');
              response = { ok: true, result: { stopping: true } };
              shouldStop = true;
            } else response = await broker.request(request);
          } catch (error) {
            response = { ok: false, error: { code: error.code ?? 'INVALID_REQUEST', message: error instanceof BrowserError ? error.message : '无效请求，未派发。', phase: 'not_dispatched' } };
          }
          let line = JSON.stringify(response) + '\n';
          if (Buffer.byteLength(line) > MAX_RESULT) line = JSON.stringify({ ok: false, error: { code: 'RESULT_TOO_LARGE', message: '结果超过32 KiB，先查任务状态，不自动重发动作。', phase: 'outcome_unknown' } }) + '\n';
          if (shouldStop) socket.once('close', () => { void stop().catch(error => { process.stderr.write(`网页服务停止失败：${error.message}\n`); process.exitCode = 1; }); });
          socket.end(line);
        })();
      });
    });
    await new Promise((resolve, reject) => { server.once('error', reject); server.listen(socketPath, resolve); });
    bound = true;
    await chmod(socketPath, 0o600);
    return { broker, provider, socketPath, controlToken, closed, close: stop };
  } catch (error) {
    await cleanup();
    throw error;
  }
}

export function call(home, request, { timeout = 45000 } = {}) {
  const line = JSON.stringify(request) + '\n';
  if (Buffer.byteLength(line) > MAX_REQUEST) return Promise.reject(new BrowserError('REQUEST_TOO_LARGE', '请求超过64 KiB。'));
  return new Promise((resolve, reject) => {
    let buffer = '';
    let size = 0;
    let sent = false;
    const decoder = new StringDecoder('utf8');
    const socket = net.connect(join(home, 'browser.sock'));
    socket.setTimeout(timeout, () => socket.destroy(new BrowserError('CLIENT_TIMEOUT', '等待超时；服务端可能仍在执行，先查status，不自动重试。', 'outcome_unknown')));
    socket.on('connect', () => { sent = true; socket.write(line); });
    socket.on('error', error => reject(error instanceof BrowserError ? error : new BrowserError(
      error.code || 'CONNECTION_ERROR', sent ? '连接异常；请求可能已执行，先查status/receipt，不自动重试。' : '未连上网页服务，请先启动服务。',
      sent ? 'outcome_unknown' : 'not_dispatched')));
    socket.on('data', chunk => {
      size += chunk.length;
      if (size > MAX_RESULT) { socket.destroy(new BrowserError('RESULT_TOO_LARGE', '返回超过上限，先查status。', 'outcome_unknown')); return; }
      buffer += decoder.write(chunk);
      if (buffer.includes('\n')) {
        try { const value = JSON.parse(buffer.slice(0, buffer.indexOf('\n'))); socket.end(); resolve(value); }
        catch { socket.destroy(); reject(new BrowserError('INVALID_RESPONSE', '返回不是完整JSON；请求可能已执行，先核对结果。', 'outcome_unknown')); }
      }
    });
    socket.on('end', () => { if (!buffer.includes('\n')) reject(new BrowserError('CONNECTION_LOST', '连接中断，先查status核对执行结果。', 'outcome_unknown')); });
  });
}
