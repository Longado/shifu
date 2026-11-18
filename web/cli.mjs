import { homedir } from 'node:os';
import { join } from 'node:path';
import { readFile } from 'node:fs/promises';
import { call, createService } from './service.mjs';
import { MAX_REQUEST } from './broker.mjs';

const help = `Shifu web：多个 Agent 共用的无界面网页任务池（按本机内存限额，不接管本人浏览器）

  shifu web serve --endpoint ws://127.0.0.1:9339/pw
                              连接已存在的Playwright端点，不自行启动浏览器
  shifu web status            查看任务、占用、等待和未决结果
  shifu web request           从stdin读一条JSON请求，不把填写内容放在进程参数
  shifu web request --control
                              本人控制入口：暂停、核对后恢复、复核释放、停止

运行目录：SHIFU_WEB_HOME，默认~/.shifu/web。
关键词正文查找：request的JSON使用method:"find"、queries:["关键词"]。
任务协议与例子见docs/WEB-POOL.md。需要Node.js与web目录的固定依赖。`;

async function main() {
  const [command = 'help', ...args] = process.argv.slice(2);
  if (['help', '--help', '-h'].includes(command)) { console.log(help); return; }
  const home = process.env.SHIFU_WEB_HOME || join(homedir(), '.shifu/web');
  if (command === 'serve') {
    if (args.length !== 2 || args[0] !== '--endpoint') throw new Error('需要serve --endpoint <本机Playwright端点>。');
    const service = await createService({ home, endpoint: args[1] });
    console.log(JSON.stringify({ listening: true, home, pid: process.pid }));
    for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => {
      if (service.broker.tasks.size) { process.stderr.write('仍有任务页，未关闭；请先逐项复核释放，再停止服务。\n'); return; }
      void service.close().catch(error => { process.stderr.write(error.message + '\n'); process.exitCode = 1; });
    });
    await service.closed;
    return;
  }
  let request;
  if (command === 'status' && !args.length) request = { method: 'status' };
  else if (command === 'request' && (args.length === 0 || args.length === 1 && args[0] === '--control')) {
    let bytes = 0;
    const chunks = [];
    for await (const chunk of process.stdin) {
      bytes += chunk.length;
      if (bytes > MAX_REQUEST) throw new Error('请求超过64 KiB。');
      chunks.push(chunk);
    }
    request = JSON.parse(Buffer.concat(chunks).toString());
    if (!request || typeof request !== 'object' || Array.isArray(request)) throw new Error('需要JSON对象。');
    if ('controlToken' in request) throw new Error('stdin不能携带控制凭证；本人控制操作使用--control。');
    if (args[0] === '--control') request.controlToken = JSON.parse(await readFile(join(home, 'control.json'), 'utf8')).token;
  } else throw new Error('不支持的参数；运行shifu web help。');
  const response = await call(home, request);
  console.log(JSON.stringify(response));
  if (!response.ok) process.exitCode = 1;
}

try { await main(); }
catch (error) {
  console.log(JSON.stringify({ ok: false, error: { code: error.code || 'CLI_ERROR', message: error.message, phase: error.phase || 'not_dispatched' } }));
  process.exitCode = 1;
}
