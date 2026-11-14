import { RemoteChannel, parseCredential, parsePairLink, type CredentialStore } from "./channel";
export type { TerminalSocket } from "./channel";

export const cloudHost = location.pathname.match(/^\/h\/([a-f0-9]{32})\/?$/i)?.[1] || null;
let report = (_text: string, _reset?: boolean) => {};
let current: RemoteChannel | null = null;
const storageKey = `shifu-device:${cloudHost}`;
const store: CredentialStore = {
  async save(credential) { localStorage.setItem(storageKey, JSON.stringify(credential)); },
  async remove() { localStorage.removeItem(storageKey); },
};
export function cloudStatus(callback: (text: string, reset?: boolean) => void) { report = callback; }
export function pairFromLink(link: string) {
  const {target, credential} = parsePairLink(link);
  current?.fail("正在重新配对", true);
  current = new RemoteChannel(target, credential, true, store, (...args) => report(...args));
}
export function startCloud() {
  if (!cloudHost) return;
  const pair = location.hash;
  if (pair) { history.replaceState(null, "", location.pathname); pairFromLink(`${location.origin}${location.pathname}${pair}`); }
  else {
    const saved = localStorage.getItem(storageKey);
    if (saved) current = new RemoteChannel({relay: location.origin, host: cloudHost}, parseCredential(JSON.parse(saved)), false, store, (...args) => report(...args));
  }
}
export function cloudAgents() { if (!current) return Promise.reject(new Error("请从远端 Mac 获取配对地址，或扫描配对二维码。")); return current.list(); }
export function terminalSocket(name: string) {
  if (cloudHost) { if (!current) throw new Error("尚未配对"); return current.open(name); }
  return new WebSocket(`${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/api/terminal/${encodeURIComponent(name)}`);
}
export function forgetCloud() { current?.fail("已忘记此设备", true); current = null; localStorage.removeItem(storageKey); }
