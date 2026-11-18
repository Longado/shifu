import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { NativeTransport } from './native-transport.mjs';
import { ChromeProvider } from './chrome-provider.mjs';
import { extensionId } from './chrome-setup.mjs';
import { createService } from './service.mjs';
import { BrowserError } from './playwright.mjs';

// stdout is exclusively Chrome's length-prefixed native messaging protocol.
async function main() {
  if (process.argv[2] !== `chrome-extension://${await extensionId()}/`) throw new Error('调用来源与登记扩展不符。');
  const home = process.env.SHIFU_BROWSER_HOME || join(homedir(), '.shifu/browser-chrome');
  const loadFiles = async () => {
    try {
      const files = JSON.parse(await readFile(join(home, 'files.json'), 'utf8'));
      if (!files || typeof files !== 'object' || Array.isArray(files)) throw new Error('invalid registry');
      return files;
    } catch (error) {
      if (error.code === 'ENOENT') return {};
      throw new BrowserError('INVALID_FILE_REGISTRY', '批准文件登记无法读取，请核对后再选择。');
    }
  };
  let service;
  let ending = false;
  const end = async () => {
    if (ending) return;
    ending = true;
    // Borrowed Chrome tabs outlive this process. No close/discard is sent.
    if (service) await service.close();
    process.stdin.destroy();
  };
  const transport = new NativeTransport(process.stdin, process.stdout, {
    onClose: () => { void end().catch(() => { process.exitCode = 1; }); },
  });
  service = await createService({ home, provider: new ChromeProvider(transport, { loadFiles }) });
  if (ending) { await service.close(); return; }
  transport.send({ kind: 'ready', version: 1, maxTasks: 3 });
  for (const signal of ['SIGTERM', 'SIGINT']) process.once(signal, () => {
    void end().catch(() => { process.exitCode = 1; });
  });
  await service.closed;
  process.stdin.destroy();
}

try { await main(); }
catch { process.stdin.destroy(); process.stderr.write('Shifu Chrome本地连接未启动；检查登记路径、扩展身份和私有运行目录。\n'); process.exitCode = 1; }
