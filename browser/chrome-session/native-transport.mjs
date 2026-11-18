import { randomUUID } from 'node:crypto';
import { endianness } from 'node:os';
import { BrowserError } from './playwright.mjs';

const MAX_FRAME = 1024 * 1024;
const readLength = buffer => endianness() === 'LE' ? buffer.readUInt32LE(0) : buffer.readUInt32BE(0);

export function nativeFrame(value) {
  const body = Buffer.from(JSON.stringify(value));
  if (body.length > MAX_FRAME) throw new BrowserError('MESSAGE_TOO_LARGE', '浏览器消息超过1 MiB。');
  const header = Buffer.alloc(4);
  if (endianness() === 'LE') header.writeUInt32LE(body.length); else header.writeUInt32BE(body.length);
  return Buffer.concat([header, body]);
}

export class NativeTransport {
  constructor(input, output, { timeoutMs = 15000, onClose = () => {} } = {}) {
    this.input = input;
    this.output = output;
    this.timeoutMs = timeoutMs;
    this.bindings = new Map();
    this.autoEnabled = false;
    this.pending = new Map();
    let buffer = Buffer.alloc(0);
    this.closed = false;
    const close = () => {
      if (this.closed) return;
      this.closed = true;
      this.autoEnabled = false;
      this.bindings.clear();
      for (const entry of this.pending.values()) {
        clearTimeout(entry.timer);
        entry.reject(new BrowserError('CHROME_DISCONNECTED', 'Chrome连接中断，核对后再恢复。', 'outcome_unknown'));
      }
      this.pending.clear();
      onClose();
    };
    input.on('data', chunk => {
      if (this.closed) return;
      buffer = Buffer.concat([buffer, chunk]);
      while (buffer.length >= 4) {
        const length = readLength(buffer);
        if (!length || length > MAX_FRAME) { close(); input.destroy(); return; }
        if (buffer.length < length + 4) return;
        let message;
        try { message = JSON.parse(buffer.subarray(4, length + 4).toString('utf8')); }
        catch { close(); input.destroy(); return; }
        buffer = buffer.subarray(length + 4);
        if (!message || typeof message !== 'object' || Array.isArray(message)) { close(); input.destroy(); return; }
        if (message.kind === 'mode' && typeof message.autoEnabled === 'boolean') {
          this.autoEnabled = message.autoEnabled;
        } else if (message.kind === 'bindings') {
          const valid = Array.isArray(message.bindings) && message.bindings.length <= 3 &&
            message.bindings.every(b => b && Number.isSafeInteger(b.tabId) && b.tabId > 0 &&
            Number.isSafeInteger(b.windowId) && b.windowId > 0 && b.id === `chrome_${b.tabId}` &&
            typeof b.grant === 'string' && b.grant.length > 0 && b.grant.length <= 80 && typeof b.url === 'string' && /^https?:\/\//.test(b.url));
          if (!valid) { close(); input.destroy(); return; }
          this.bindings = new Map(message.bindings.map(b => [b.id, b]));
        } else if (message.kind === 'reply') {
          const entry = this.pending.get(message.id);
          if (!entry) continue;
          this.pending.delete(message.id);
          clearTimeout(entry.timer);
          if (message.error) {
            const known = ['AUTO_DISABLED', 'INVALID_TAB', 'EXPECTED_URL_REQUIRED', 'PAGE_NOT_SUPPORTED', 'PAGE_CHANGED', 'TAB_STOPPED', 'CAPACITY_REACHED', 'TAB_NOT_AUTHORIZED'];
            entry.reject(new BrowserError(known.includes(message.error.code) ? message.error.code : 'CHROME_COMMAND_FAILED',
              'Chrome调用被拒绝或失败，先核对目标与连接状态。', message.error.phase === 'not_dispatched' ? 'not_dispatched' : 'outcome_unknown'));
          }
          else entry.resolve(message.result);
        }
      }
    });
    input.once('end', close);
    input.once('close', close);
    input.once('error', close);
    output.once('error', close);
  }

  send(value) { this.output.write(nativeFrame(value)); }

  request(tabId, method, params, grant) {
    if (this.closed) return Promise.reject(new BrowserError('CHROME_DISCONNECTED', 'Chrome连接已停止。'));
    const id = randomUUID();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new BrowserError('CHROME_TIMEOUT', 'Chrome调用超时；不自动重发。', 'outcome_unknown'));
      }, this.timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      try { this.send({ kind: 'command', id, tabId, method, params, grant }); }
      catch (error) { clearTimeout(timer); this.pending.delete(id); reject(error); }
    });
  }
}
