import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { RemoteChannel, parsePairLink, type Credential, type RelayTarget, type TerminalSocket } from "../remote/channel";
import { remoteMacsLoad, remoteMacsSave, remoteMacsForget } from "./api";
import { present, closeNewPopover } from "./newpopover";

interface Agent { name: string; title: string; kind: string; task: string | null; status: string; cwd: string }
const button = (text: string) => Object.assign(document.createElement("button"), {type: "button", className: "btn", textContent: text});
let active: RemoteView | null = null;

function agentList(value: unknown): Agent[] {
  if (!Array.isArray(value) || value.some(v => !v || ["name", "title", "kind", "status", "cwd"].some(k => typeof v[k] !== "string") || !(v.task == null || typeof v.task === "string"))) throw new Error("远端返回了无效的会话列表");
  return value;
}

class RemoteView {
  private root = document.createElement("section");
  private connection = document.createElement("span");
  private rows = document.createElement("div");
  private title = document.createElement("strong");
  private screen = document.createElement("div");
  private notice = document.createElement("p");
  private reply = document.createElement("textarea");
  private send = button("发送");
  private keys: HTMLButtonElement[] = [];
  private term = new Terminal({cols: 100, rows: 24, fontSize: 14, screenReaderMode: true, scrollback: 5000, theme: {background: "#171d20", foreground: "#e5e8e5"}});
  private fit = new FitAddon();
  private channel: RemoteChannel;
  private socket: TerminalSocket | null = null;
  private selected: Agent | null = null;
  private observer: ResizeObserver;
  private ready = false;
  private disposed = false;
  private stopped = false;
  private ended = false;
  private listBusy = false;
  private next = 0;
  private pending: {id: number; text: string} | null = null;
  private reconnectTimer = 0;
  private ackTimer = 0;
  private listTimer = 0;
  private newSession = button("新建会话");
  private createForm = document.createElement("form");
  private project = document.createElement("select");
  private kind = document.createElement("select");
  private createButton = button("创建并进入");
  private createNotice = document.createElement("p");
  private versionNotice = document.createElement("p");
  private creating = false;
  private catalogBusy = false;
  private local = Array.from(document.querySelectorAll<HTMLElement>(".app-body, .statusbar, .side-toggle"), element => ({element, inert: element.inert}));

  constructor(private name: string, target: RelayTarget, credential: Credential, temporary: boolean, private online: (value: boolean) => void) {
    this.root.className = "remote-control-workspace";
    this.local.forEach(({element}) => {element.inert = true;});
    this.root.setAttribute("role", "region"); this.root.setAttribute("aria-label", "远程 Mac");
    const head = document.createElement("header"); head.className = "remote-control-head";
    const back = button("断开并返回本机"); back.onclick = () => this.close();
    head.append(Object.assign(document.createElement("strong"), {textContent: `远程 Mac：${name}`}), this.connection, back);
    this.connection.className = "remote-control-connection"; this.connection.setAttribute("aria-live", "polite");
    const body = document.createElement("div"); body.className = "remote-control-body";
    const side = document.createElement("nav"); side.className = "remote-control-sessions"; side.setAttribute("aria-label", "远端会话");
    const refresh = button("刷新会话"); refresh.onclick = () => void this.refresh();
    this.newSession.onclick = () => void this.showCreate();
    this.createForm.className = "remote-control-create remote-form"; this.createForm.hidden = true;
    this.project.setAttribute("aria-label", "远端项目"); this.kind.setAttribute("aria-label", "远端Agent");
    const field = (label: string, select: HTMLSelectElement) => {
      const row = document.createElement("label"); row.append(Object.assign(document.createElement("span"), {textContent: label}));
      const wrap = document.createElement("span"); wrap.className = "select-wrap"; select.className = "pick-select"; wrap.append(select); row.append(wrap); return row;
    };
    this.createButton.type = "submit";
    const cancel = button("取消"); cancel.onclick = () => {this.createForm.hidden = true;};
    this.createNotice.className = "remote-error"; this.createNotice.hidden = true; this.createNotice.setAttribute("role", "alert");
    this.versionNotice.className = "remote-note"; this.versionNotice.hidden = true; this.versionNotice.textContent = "远端版本较旧，更新后可新建会话。";
    this.createForm.append(field("项目", this.project), field("Agent", this.kind), this.createButton, cancel, this.createNotice);
    this.createForm.onsubmit = event => {event.preventDefault(); void this.createSession();};
    this.project.onchange = () => {this.project.title = this.project.value; this.controls();};
    this.kind.onchange = () => this.controls();
    side.append(this.newSession, this.createForm, this.versionNotice, refresh, this.rows);
    const pane = document.createElement("div"); pane.className = "remote-control-pane";
    this.title.className = "remote-control-title"; this.title.textContent = "选择远端会话";
    this.screen.className = "remote-control-terminal";
    this.notice.className = "remote-error"; this.notice.hidden = true; this.notice.setAttribute("role", "alert");
    const keys = document.createElement("div"); keys.className = "remote-control-keys";
    for (const [label, data] of [["Esc", "\x1b"], ["Tab", "\t"], ["↑", "\x1b[A"], ["↓", "\x1b[B"], ["Enter", "\r"], ["Ctrl+C", "\x03"]]) {
      const key = button(label); key.onclick = () => this.write(data); keys.append(key); this.keys.push(key);
    }
    const form = document.createElement("form"); form.className = "remote-control-reply";
    this.reply.setAttribute("aria-label", "回复远端会话"); this.reply.placeholder = "回复远端会话"; this.reply.oninput = () => this.controls(); this.send.type = "submit";
    form.append(this.reply, this.send); form.onsubmit = e => {e.preventDefault(); this.submit();};
    this.reply.onkeydown = e => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.isComposing) {e.preventDefault(); this.submit();} };
    pane.append(this.title, this.screen, this.notice, keys, form); body.append(side, pane); this.root.append(head, body);
    document.getElementById("app")!.append(this.root);
    this.term.loadAddon(this.fit); this.term.open(this.screen); this.term.onData(data => this.write(data));
    this.observer = new ResizeObserver(() => this.resize()); this.observer.observe(this.screen);
    this.channel = new RemoteChannel(target, credential, temporary, {
      save: value => remoteMacsSave({name, ...target, credential: {id: value.id, key: value.key}}),
      remove: () => remoteMacsForget(target.relay, target.host),
    }, (text, reset) => {
      if (this.disposed) return;
      this.connection.textContent = text || "已连接远端 Mac";
      this.online(!text);
      this.versionNotice.hidden = !!text || this.channel.supportsSessionCreate;
      this.controls();
      if (reset) {this.stopped = true; this.ready = false; this.cancelPending(); this.noticeText(text); this.controls();}
    }, "Shifu 控制端");
    this.controls(); void this.refresh(); this.listTimer = window.setInterval(() => void this.refresh(), 5000);
  }

  private noticeText(text: string) { this.notice.textContent = text; this.notice.hidden = !text; }
  private controls() {
    this.send.disabled = !this.ready || !!this.pending || !this.reply.value.trim();
    this.keys.forEach(key => {key.disabled = !this.ready || !!this.pending;});
    const canCreate = this.channel?.authorized && this.channel.supportsSessionCreate && !this.stopped;
    this.newSession.disabled = !canCreate || this.creating || this.catalogBusy;
    this.createButton.disabled = !canCreate || this.creating || this.catalogBusy || !!this.pending || !this.project.value || !this.kind.value;
    this.project.disabled = this.kind.disabled = this.creating || this.catalogBusy;
  }
  private creationText(text: string) {this.createNotice.textContent = text; this.createNotice.hidden = !text;}
  private async showCreate() {
    this.createForm.hidden = false; this.creationText(""); this.catalogBusy = true; this.controls();
    this.project.replaceChildren(); this.kind.replaceChildren();
    try {
      const catalog = await this.channel.catalog();
      if (this.disposed) return;
      this.project.replaceChildren(...catalog.projects.map(p => new Option(`${p.name} — ${p.path}`, p.path)));
      this.project.title = this.project.value;
      this.kind.replaceChildren(...catalog.agents.filter(a => a.installed).map(a => new Option(a.name, a.kind)));
      if (!catalog.projects.length) this.creationText("远端还没有项目，请先在远端Shifu打开项目。");
      else if (!this.kind.options.length) this.creationText("远端未安装Claude或Codex，请先完成安装。");
    } catch (error) {if (!this.disposed) this.creationText(error instanceof Error ? error.message : String(error));}
    finally {this.catalogBusy = false; this.controls();}
  }
  private async createSession() {
    if (this.createButton.disabled) return;
    if (this.reply.value && !confirm("新建并进入远端会话会清空未发送的回复，继续吗？")) return;
    this.creating = true; this.creationText(""); this.controls();
    let created = false;
    try {
      const name = await this.channel.create(this.project.value, this.kind.value);
      created = true;
      if (this.disposed) return;
      const values = agentList(await this.channel.list());
      if (this.disposed) return;
      this.renderRows(values);
      const agent = values.find(a => a.name === name);
      if (!agent) {this.creationText("会话已创建，请刷新会话列表后进入。"); return;}
      this.open(agent, true); this.createForm.hidden = true;
    } catch (error) {if (!this.disposed) this.creationText(created ? "会话已创建，连接恢复后请刷新会话列表进入。" : error instanceof Error ? error.message : String(error));}
    finally {this.creating = false; this.controls();}
  }
  private cancelPending() {
    if (this.pending) this.noticeText("发送结果未确认，请查看远端终端后再决定是否重发。");
    this.pending = null; clearTimeout(this.ackTimer); this.controls();
  }
  private resize() {
    if (!this.root.isConnected || this.screen.clientHeight < 50) return;
    this.fit.fit();
    if (this.ready) this.socket?.send(JSON.stringify({type: "resize", cols: this.term.cols, rows: this.term.rows}));
  }
  private write(data: string) {
    if (this.ready && !this.pending) this.socket?.send(JSON.stringify({type: "input", data}));
  }
  private submit() {
    if (!this.ready || this.pending || !this.reply.value.trim()) return;
    if (new TextEncoder().encode(this.reply.value).length > 16 * 1024) {this.noticeText("每次回复请控制在 16 KB 以内。"); return;}
    this.pending = {id: ++this.next, text: this.reply.value}; this.controls();
    this.socket!.send(JSON.stringify({type: "paste", ...this.pending}));
    this.ackTimer = window.setTimeout(() => this.cancelPending(), 5000);
  }
  private connectTerminal() {
    if (!this.selected || this.stopped || this.disposed || this.ended) return;
    const ws = this.channel.open(this.selected.name); this.socket = ws; this.ready = false; this.controls();
    ws.onmessage = event => {
      if (this.socket !== ws) return;
      if (event.data instanceof ArrayBuffer) {this.term.write(new Uint8Array(event.data)); return;}
      const message = JSON.parse(event.data);
      if (message.type === "ready") {this.term.reset(); this.term.resize(message.cols, message.rows); this.ready = true; this.resize(); this.controls();}
      else if (message.type === "sent" && this.pending && this.pending.id === message.id) {
        if (this.reply.value === this.pending.text) this.reply.value = "";
        this.pending = null; clearTimeout(this.ackTimer); this.noticeText(""); this.controls();
      } else if (message.type === "error" || message.type === "ended") {
        this.ended = true; this.ready = false; this.noticeText(message.message || "远端会话已结束"); this.controls();
      }
    };
    ws.onclose = () => {
      if (this.socket !== ws) return;
      this.socket = null; this.ready = false; this.cancelPending();
      if (!this.disposed && !this.stopped && !this.ended) this.reconnectTimer = window.setTimeout(() => this.connectTerminal(), 800);
    };
  }
  private open(agent: Agent, discardDraft = false) {
    if (this.selected?.name === agent.name && this.socket) return;
    if (this.reply.value && !discardDraft && !confirm("切换远端会话会清空未发送的回复，继续吗？")) return;
    clearTimeout(this.reconnectTimer); const previous = this.socket; this.socket = null; previous?.close();
    this.cancelPending(); this.reply.value = ""; this.selected = agent; this.ended = false; this.title.textContent = agent.title; this.noticeText("");
    this.rows.querySelectorAll<HTMLButtonElement>("button[data-name]").forEach(row => row.setAttribute("aria-current", String(row.dataset.name === agent.name)));
    this.connectTerminal(); this.term.focus();
  }
  private async refresh() {
    if (this.listBusy || this.disposed || this.stopped) return;
    this.listBusy = true;
    try {
      const values = agentList(await this.channel.list());
      if (this.disposed) return;
      this.renderRows(values);
      // The connection header reports disconnects; don't overwrite an unconfirmed-send warning.
    } catch (e) {if (!this.disposed && !this.stopped && this.channel.authorized) this.noticeText(String(e));}
    finally {this.listBusy = false;}
  }
  private renderRows(values: Agent[]) {
    this.rows.replaceChildren();
    if (!values.length) this.rows.append(Object.assign(document.createElement("p"), {className: "remote-note", textContent: "远端暂无会话"}));
    for (const agent of values) {
      const row = button(""); row.className = "remote-control-session"; row.dataset.name = agent.name;
      row.setAttribute("aria-current", String(this.selected?.name === agent.name));
      row.append(Object.assign(document.createElement("span"), {textContent: agent.title}), Object.assign(document.createElement("small"), {textContent: agent.task || agent.kind}));
      row.onclick = () => this.open(agent); this.rows.append(row);
    }
  }
  close(): boolean {
    if (this.creating && !confirm("远端正在创建会话，断开后结果可能未确认，继续吗？")) return false;
    if (!this.creating && this.reply.value && !confirm(`断开“${this.name}”会清空未发送的回复，继续吗？`)) return false;
    this.disposed = true; clearTimeout(this.reconnectTimer); clearTimeout(this.ackTimer); clearInterval(this.listTimer);
    this.socket?.close(); this.channel.fail("已断开连接", true); this.observer.disconnect(); this.term.dispose(); this.root.remove(); this.online(false);
    this.local.forEach(({element, inert}) => {element.inert = inert;});
    if (active === this) active = null;
    return true;
  }
  matches(target: RelayTarget): boolean { return this.channel.target.relay === target.relay && this.channel.target.host === target.host; }
}

export function handleRemoteControlMenu(action: string): boolean {
  if (!active) return false;
  // Native menu accelerators must not modify the hidden local workspace.
  if (action === "close") active.close();
  return true;
}
export function leaveRemoteController(): boolean { return !active || active.close(); }

export function showRemoteController(anchor: HTMLElement, hostSettings: () => void, online: (value: boolean) => void): void {
  const pop = document.createElement("section"); pop.className = "popup remote-pop remote-control-pop";
  pop.setAttribute("role", "dialog"); pop.setAttribute("aria-label", "连接远程 Mac");
  pop.append(Object.assign(document.createElement("strong"), {textContent: "远程 Mac"}));
  const saved = document.createElement("div"); saved.className = "remote-control-saved";
  const error = Object.assign(document.createElement("p"), {className: "remote-error", hidden: true}); error.setAttribute("role", "alert");
  const form = document.createElement("form"); form.className = "remote-form";
  const name = Object.assign(document.createElement("input"), {placeholder: "例如：加州 Mac", maxLength: 80}); name.setAttribute("aria-label", "远程 Mac 名称");
  const address = Object.assign(document.createElement("input"), {type: "password", placeholder: "粘贴远端 Shifu 生成的配对地址", required: true, autocomplete: "off"}); address.setAttribute("aria-label", "远端配对地址");
  const field = (title: string, input: HTMLInputElement) => {const label = document.createElement("label"); label.append(Object.assign(document.createElement("span"), {textContent: title}), input); return label;};
  const connect = button("连接远程 Mac"); connect.type = "submit";
  form.append(field("名称", name), field("远端配对地址", address), connect);
  const note = Object.assign(document.createElement("p"), {className: "remote-note", textContent: "在远端 Mac 的 Shifu 中生成配对地址，连接后仍需远端批准。"});
  const host = button("允许其他设备访问本机…"); host.classList.add("remote-control-host"); host.onclick = hostSettings;
  pop.append(saved, error, form, note, host);
  if (!present(anchor, "remote-control", pop, {suspended: true})) return;
  const position = () => {pop.style.top = "auto"; const rect = anchor.getBoundingClientRect(); pop.style.bottom = `${window.innerHeight - rect.top + 4}px`; pop.style.left = `${Math.max(4, Math.min(rect.left, innerWidth - pop.offsetWidth - 4))}px`;};
  const showError = (e: unknown) => {error.textContent = String(e); error.hidden = false; position();};
  const open = (label: string, target: RelayTarget, credential: Credential, temporary: boolean) => {
    if (active && !active.close()) return;
    address.value = ""; closeNewPopover(); active = new RemoteView(label, target, credential, temporary, online);
  };
  const load = async () => {
    const macs = await remoteMacsLoad(); saved.replaceChildren();
    for (const mac of macs) {
      const row = document.createElement("div"); row.className = "remote-device";
      const enter = button(mac.name); enter.classList.add("remote-control-enter"); enter.onclick = () => open(mac.name, mac, mac.credential, false);
      const forget = button("忘记"); forget.onclick = () => {
        if (!confirm(`忘记“${mac.name}”后需要重新配对，继续吗？`)) return;
        if (active?.matches(mac) && !active.close()) return;
        void remoteMacsForget(mac.relay, mac.host).then(load).catch(showError);
      };
      row.append(enter, forget); saved.append(row);
    }
    position();
  };
  form.onsubmit = e => {
    e.preventDefault();
    try {const {target, credential} = parsePairLink(address.value.trim()); open(name.value.trim() || "远程 Mac", target, credential, true);}
    catch (e) {showError(e);}
  };
  position(); void load().catch(showError);
}
