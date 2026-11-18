import { createHash } from 'node:crypto';
import { readFile, mkdir, writeFile, copyFile, cp, readdir } from 'node:fs/promises';
import { join, isAbsolute, resolve } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath } from 'node:url';

export async function extensionId() {
  const manifest = JSON.parse(await readFile(new URL('./chrome/manifest.json', import.meta.url), 'utf8'));
  return createHash('sha256').update(Buffer.from(manifest.key, 'base64')).digest('hex').slice(0, 32)
    .replace(/[0-9a-f]/g, value => String.fromCharCode(97 + parseInt(value, 16)));
}

const shellQuote = value => "'" + value.replace(/'/g, "'\\''") + "'";

// Produces a reviewable local component. It never registers a Chrome host or
// enables an extension, and never overwrites an earlier prepared component.
export async function prepareChrome({ output, home = join(homedir(), '.shifu/browser-chrome') }) {
  if (!isAbsolute(output ?? '') || !isAbsolute(home) || Buffer.byteLength(join(home, 'browser.sock')) > 100) {
    throw new Error('需要绝对输出路径和较短的绝对运行目录。');
  }
  const source = fileURLToPath(new URL('.', import.meta.url));
  if (resolve(output) === resolve(source) || resolve(output).startsWith(resolve(source) + '/')) {
    throw new Error('输出目录不能位于browser源码内。');
  }
  await mkdir(output, { recursive: false, mode: 0o700 });
  const runtime = join(output, 'runtime');
  await mkdir(runtime, { mode: 0o700 });
  for (const name of await readdir(source)) {
    if (name.endsWith('.mjs')) await copyFile(join(source, name), join(runtime, name));
  }
  await cp(join(source, 'chrome'), join(runtime, 'chrome'), { recursive: true, errorOnExist: true });
  await cp(join(source, 'chrome'), join(output, 'extension'), { recursive: true, errorOnExist: true });
  const id = await extensionId();
  const launcher = join(output, 'launch-host');
  await writeFile(launcher, '#!/bin/sh\nexec env SHIFU_BROWSER_HOME=' + shellQuote(home) + ' ' + shellQuote(process.execPath) +
    ' ' + shellQuote(join(runtime, 'native-host.mjs')) + ' "$@"\n', { mode: 0o700 });
  const manifestName = 'dev.longado.shifu.browser.json';
  await writeFile(join(output, manifestName), JSON.stringify({ name: 'dev.longado.shifu.browser',
    description: 'Shifu local Chrome bridge', path: launcher, type: 'stdio',
    allowed_origins: [`chrome-extension://${id}/`] }, null, 2) + '\n', { mode: 0o600 });
  return { prepared: true, installed: false, extensionId: id, extensionPath: join(output, 'extension'),
    nativeManifest: join(output, manifestName),
    chromeManifestDestination: join(homedir(), 'Library/Application Support/Google/Chrome/NativeMessagingHosts', manifestName),
    runtime: join(runtime, 'cli.mjs'), home, requiresUserEnable: true };
}
