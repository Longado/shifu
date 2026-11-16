import { homedir } from 'node:os';
import { join } from 'node:path';
import { readFile } from 'node:fs/promises';
import { call } from './service.mjs';
import { MAX_REQUEST } from './broker.mjs';

const help = `Shifu Chrome 一次性接入

  node <CLI> status        查看任务、占用、等待和未决结果
  node <CLI> chrome-prepare --output /absolute/new-directory
                              准备独立Chrome组件；不安装扩展或更改Chrome设置
  node <CLI> bindings     列出当前任务已接入的Chrome页面
  node <CLI> request       从stdin读一条JSON请求，不把填写内容放在进程参数
  node <CLI> request --control
                              本人控制入口：暂停、核对后恢复、复核释放、停止

运行目录：SHIFU_BROWSER_HOME，默认~/.shifu/browser-chrome。
关键词正文查找：request的JSON使用method:"find"、queries:["关键词"]。
Chrome一次接入：全局开启后，attach携带bindingId:"chrome_实际tabID"和expectedUrl；不用逐页点击。
任务协议与例子见docs/CHROME-BRIDGE.md。Chrome组件只需Node.js。`;

async function main() {
  const [command = 'help', ...args] = process.argv.slice(2);
  if (['help', '--help', '-h'].includes(command)) { console.log(help); return; }
  if (command === 'chrome-prepare' && args.length === 2 && args[0] === '--output') {
    const { prepareChrome } = await import('./chrome-setup.mjs');
    console.log(JSON.stringify(await prepareChrome({ output: args[1], home: process.env.SHIFU_BROWSER_HOME })));
    return;
  }
  const home = process.env.SHIFU_BROWSER_HOME || join(homedir(), '.shifu/browser-chrome');
  let request;
  if (command === 'status' && !args.length) request = { method: 'status' };
  else if (command === 'bindings' && !args.length) request = { method: 'bindings' };
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
  } else throw new Error('不支持的参数；运行node <CLI> help。');
  const response = await call(home, request);
  console.log(JSON.stringify(response));
  if (!response.ok) process.exitCode = 1;
}

try { await main(); }
catch (error) {
  console.log(JSON.stringify({ ok: false, error: { code: error.code || 'CLI_ERROR', message: error.message, phase: error.phase || 'not_dispatched' } }));
  process.exitCode = 1;
}
