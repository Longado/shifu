import init, { Channel } from "../../remote/crypto/pkg/shifu_channel.js";

export interface TerminalSocket {
  readyState: number; binaryType: BinaryType;
  onmessage: ((event: MessageEvent) => void) | null;
  onclose: ((event: CloseEvent) => void) | null;
  send(data: string): void; close(): void;
}
export interface Credential { id: string; key: string; name?: string; expires?: number }
let wasm: ReturnType<typeof init> | undefined;
const hex = (bytes: Uint8Array) => Array.from(bytes, b => b.toString(16).padStart(2, "0")).join("");
const random = (size: number) => hex(crypto.getRandomValues(new Uint8Array(size)));
function decode(key: string) { return new Uint8Array(key.match(/../g)!.map(b => parseInt(b, 16))); }
export function parseCredential(value: unknown): Credential {
  const v = value as Credential;
  if (!v || !/^[a-f0-9]{32}$/i.test(v.id) || !/^[a-f0-9]{64}$/i.test(v.key)) throw new Error("配对地址无效");
  return v;
}
export interface RelayTarget { relay: string; host: string }
export interface SessionCatalog {
  projects: {path: string; name: string}[];
  agents: {kind: "claude" | "codex"; name: string; installed: boolean}[];
}
export function parseSessionCatalog(value: unknown): SessionCatalog {
  const v = value as SessionCatalog;
  if (!v || !Array.isArray(v.projects) || !Array.isArray(v.agents)
    || v.projects.some(p => !p || typeof p.path !== "string" || typeof p.name !== "string")
    || v.agents.some(a => !a || !["claude", "codex"].includes(a.kind) || typeof a.name !== "string" || typeof a.installed !== "boolean")) throw new Error("远端返回了无效的新建选项");
  return v;
}
export class CreationUnconfirmed extends Error {
  constructor() { super("创建结果未确认，请刷新远端会话列表后再决定是否新建。"); }
}
class RequestRejected extends Error {}
export interface CredentialStore { save(credential: Credential): Promise<void>; remove(): Promise<void> }
export function parsePairLink(link: string): {target: RelayTarget; credential: Credential} {
  let url: URL;
  try { url = new URL(link); } catch { throw new Error("请粘贴远端 Shifu 生成的完整配对地址"); }
  const host = url.pathname.match(/^\/h\/([a-f0-9]{32})\/?$/i)?.[1];
  const local = ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname);
  if (!host || url.username || url.password || url.search || !(url.protocol === "https:" || (local && url.protocol === "http:")) || !url.hash.startsWith("#pair=")) throw new Error("请粘贴远端 Shifu 生成的完整配对地址");
  try {
    const value = url.hash.slice(6).replace(/-/g, "+").replace(/_/g, "/");
    return {target: {relay: url.origin, host}, credential: parseCredential(JSON.parse(atob(value)))};
  } catch { throw new Error("配对地址无效，请从远端 Mac 重新生成"); }
}

export class RemoteChannel {
  socket: WebSocket | null = null;
  cipher: Channel | null = null;
  authorized = false; stopped = false; retry = 800; timer = 0;
  supportsSessionCreate = false;
  stoppedReason = "请重新配对设备";
  terminal: CloudTerminal | null = null;
  waiters: Array<{ resolve: () => void; reject: (e: Error) => void }> = [];
  requests = new Map<number, { resolve: (value: any) => void; reject: (e: Error) => void }>();
  sequence = 0;
  proposed: Credential = { id: random(16), key: random(32), name: "我的浏览器" };
  constructor(public target: RelayTarget, public credential: Credential, public temporary: boolean, private store: CredentialStore, private report: (text: string, reset?: boolean) => void, deviceName = "我的浏览器") { this.proposed.name = deviceName; void this.connect(); }
  async connect() {
    try { await (wasm ??= init()); } catch { wasm = undefined; this.fail("无法加载加密组件，请重新打开远程连接", true); return; }
    if (this.stopped) return;
    if (this.temporary && (this.credential.expires || 0) <= Date.now() / 1000) { this.fail("配对码已过期，请从远端 Mac 重新生成。", true); return; }
    this.report(this.temporary ? "正在申请配对" : "正在连接远端 Mac");
    const ws = new WebSocket(`${this.target.relay.replace(/^https:/, "wss:").replace(/^http:/, "ws:")}/relay/view/${this.target.host}`);
    ws.binaryType = "arraybuffer"; this.socket = ws;
    const cipher = new Channel(true, decode(this.credential.key), `shifu-relay-v1/${this.target.host}/${this.credential.id}`); this.cipher = cipher;
    ws.onopen = () => {
      ws.send(JSON.stringify({ credential: this.credential.id }));
      ws.send(cipher.write(new Uint8Array()));
    };
    ws.onmessage = async event => {
      if (this.socket !== ws) return;
      try {
        if (typeof event.data === "string") { this.report("暂时无法连接远端 Mac，请确认它已经联网并启动 Shifu。"); return; }
        const wasReady = cipher.ready();
        const clear = cipher.read(new Uint8Array(event.data));
        if (!wasReady) {
          if (this.temporary) this.send({ op: "pair", device: this.proposed.id, key: this.proposed.key, name: this.proposed.name });
          return;
        }
        const value = JSON.parse(new TextDecoder().decode(clear));
        if (value.type === "pair_pending") this.report("等待远端 Mac 批准此设备");
        else if (value.type === "paired") {
          this.credential = this.proposed; this.temporary = false;
          this.socket = null; ws.close();
          try { await this.store.save(this.credential); } catch { this.fail("无法保存配对，请重新连接", true); return; }
          if (!this.stopped) void this.connect();
        } else if (value.type === "authenticated") {
          this.supportsSessionCreate = Array.isArray(value.capabilities) && value.capabilities.includes("session_create");
          this.authorized = true; this.retry = 800; this.report(""); this.waiters.splice(0).forEach(w => w.resolve());
        } else if (value.type === "agents") {
          this.requests.get(value.id)?.resolve(value.agents); this.requests.delete(value.id);
        } else if (value.type === "catalog" || value.type === "created") {
          if (value.type === "created" && typeof value.name !== "string") throw new Error("无效的新会话");
          this.requests.get(value.id)?.resolve(value.type === "catalog" ? value.catalog : value.name); this.requests.delete(value.id);
        } else if (value.type === "request_error") {
          this.requests.get(value.id)?.reject(new RequestRejected(String(value.message || "远端请求失败"))); this.requests.delete(value.id);
        } else if (value.type === "terminal") {
          const data = value.binary ? Uint8Array.from(atob(value.binary), b => b.charCodeAt(0)).buffer : value.text;
          this.terminal?.onmessage?.(new MessageEvent("message", { data }));
        } else if (value.type === "terminal_close") this.terminal?.disconnected();
        else if (value.type === "revoked") {
          try { await this.store.remove(); } catch { this.fail("此设备已被撤销；请忘记此 Mac 后重新配对。", true); return; }
          this.fail("此设备已被撤销，请重新配对。", true);
        }
      } catch { this.fail("加密连接验证失败，请重新连接或配对。", true); }
    };
    ws.onclose = () => {
      cipher.free(); if (this.socket !== ws) return;
      this.socket = null; this.cipher = null; this.authorized = false; this.supportsSessionCreate = false; this.terminal?.disconnected();
      this.requests.forEach(r => r.reject(new Error("连接已中断"))); this.requests.clear();
      if (!this.stopped) { this.report("重新连接远端 Mac"); this.timer = window.setTimeout(() => void this.connect(), this.retry); this.retry = Math.min(10000, this.retry * 2); }
    };
  }
  fail(text: string, stop: boolean) {
    this.stopped = stop; this.stoppedReason = text; this.authorized = false; this.report(text, stop);
    this.waiters.splice(0).forEach(w => w.reject(new Error(text)));
    this.requests.forEach(r => r.reject(new Error(text))); this.requests.clear();
    this.socket?.close(); clearTimeout(this.timer);
  }
  async ready() {
    if (this.stopped) throw new Error(this.stoppedReason);
    if (this.authorized) return;
    return new Promise<void>((resolve, reject) => this.waiters.push({ resolve, reject }));
  }
  send(value: unknown) {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN || !this.cipher?.ready()) throw new Error("连接未就绪");
    this.socket.send(this.cipher.write(new TextEncoder().encode(JSON.stringify(value))));
  }
  open(name: string): TerminalSocket { return new CloudTerminal(this, name); }
  async list() {
    await this.ready(); const id = ++this.sequence;
    return new Promise<any[]>((resolve, reject) => { this.requests.set(id, { resolve, reject }); this.send({ op: "list", id }); });
  }
  private async sessionRequest(op: "catalog" | "create", fields: Record<string, string>) {
    await this.ready();
    if (!this.supportsSessionCreate) throw new Error("请更新远端Shifu以使用远程新建会话");
    const id = ++this.sequence;
    return new Promise<any>((resolve, reject) => {
      let sent = false;
      const timer = window.setTimeout(() => {this.requests.delete(id); fail(new Error("远端响应超时"));}, 15000);
      const fail = (error: Error) => {
        clearTimeout(timer);
        reject(op === "create" && sent && !(error instanceof RequestRejected) ? new CreationUnconfirmed() : error);
      };
      this.requests.set(id, {resolve: value => {clearTimeout(timer); resolve(value);}, reject: fail});
      try {this.send({op, id, ...fields}); sent = true;}
      catch (error) {this.requests.delete(id); fail(error as Error);}
    });
  }
  async catalog(): Promise<SessionCatalog> { return parseSessionCatalog(await this.sessionRequest("catalog", {})); }
  async create(project: string, kind: string): Promise<string> { return this.sessionRequest("create", {project, kind}); }
}

class CloudTerminal implements TerminalSocket {
  readyState: number = WebSocket.CONNECTING; binaryType: BinaryType = "arraybuffer";
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  constructor(public owner: RemoteChannel, name: string) {
    void owner.ready().then(() => { if (this.readyState === WebSocket.CLOSED) return; owner.terminal = this; this.readyState = WebSocket.OPEN; owner.send({ op: "open", name }); }).catch(() => this.disconnected());
  }
  send(data: string) {
    // A terminal can still be marked open until the transport's asynchronous close event.
    if (this.readyState === WebSocket.OPEN && this.owner.socket?.readyState === WebSocket.OPEN) this.owner.send({ op: "input", data });
  }
  close() { if (this.owner.terminal === this) { this.owner.terminal = null; if (this.owner.authorized) this.owner.send({ op: "close" }); } this.disconnected(); }
  disconnected() { if (this.readyState === WebSocket.CLOSED) return; this.readyState = WebSocket.CLOSED; this.onclose?.(new CloseEvent("close")); }
}
