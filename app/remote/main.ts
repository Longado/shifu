import { Terminal } from "@xterm/xterm";
import "./style.css";

interface Agent { name: string; title: string; kind: string; task: string | null; status: string; cwd: string }
const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const workspace = el("workspace"), loginView = el("login-view"), rows = el("session-rows");
const loginForm = el<HTMLFormElement>("login-form"), codeInput = el<HTMLInputElement>("access-code");
const loginError = el("login-error"), listError = el("list-error"), pane = el("terminal-pane");
const viewport = el("terminal-viewport"), host = el("terminal"), connection = el("connection"), notice = el("notice");
const reply = el<HTMLTextAreaElement>("reply"), send = el<HTMLButtonElement>("send");
const statusNames: Record<string, string> = { needs_approval: "等你批准", needs_reply: "等你回复", running: "运行中", idle: "空闲", starting: "启动中", exited: "已退出" };
const term = new Terminal({ cols: 100, rows: 24, fontSize: 14, lineHeight: 1.2, fontFamily: "Menlo, Consolas, monospace", cursorBlink: true, screenReaderMode: true, scrollback: 5000, theme: { background: "#171d20", foreground: "#e5e8e5", cursor: "#b8d4c9", selectionBackground: "#3b5351" } });
let mounted = false, authenticated = false, refreshing = false, selected: Agent | null = null;
let socket: WebSocket | null = null, reconnectTimer = 0, retryDelay = 800, ready = false, ended = false;
let nextId = 0, pending: { id: number; text: string } | null = null, acknowledgmentTimer = 0;

function setNotice(text: string) { notice.textContent = text; notice.hidden = !text; }
function setConnection(text: string, state = "") { connection.textContent = text; connection.dataset.state = state; }
function controls() {
  const enabled = ready && !pending;
  send.disabled = !enabled || !reply.value.trim();
  document.querySelectorAll<HTMLButtonElement>("#keys button").forEach(button => button.disabled = !enabled);
}
function discardPending() {
  if (pending) setNotice("发送结果未确认，请查看终端后再决定是否重发。");
  pending = null; clearTimeout(acknowledgmentTimer); controls();
}
function disconnect() {
  clearTimeout(reconnectTimer);
  const old = socket; socket = null; ready = false;
  old?.close(); discardPending(); controls();
}
function loginScreen() {
  authenticated = false; disconnect();
  workspace.hidden = true; loginView.hidden = false;
}
function fitRows() {
  if (!mounted || viewport.clientHeight < 100) return;
  const count = Math.min(200, Math.max(6, Math.floor((viewport.clientHeight - 24) / 17)));
  term.resize(term.cols, count);
  const screen = host.querySelector<HTMLElement>(".xterm-screen");
  if (screen) host.style.width = `${screen.offsetWidth + 18}px`;
  if (ready && socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: "resize", cols: term.cols, rows: count }));
}

function connectTerminal() {
  if (!selected || !authenticated || ended) return;
  const name = selected.name;
  const ws = new WebSocket(`${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/api/terminal/${encodeURIComponent(name)}`);
  socket = ws; ws.binaryType = "arraybuffer";
  setConnection("连接中"); ready = false; controls();
  ws.onmessage = event => {
    if (socket !== ws) return;
    if (event.data instanceof ArrayBuffer) { term.write(new Uint8Array(event.data)); return; }
    const message = JSON.parse(event.data);
    if (message.type === "ready") {
      term.reset(); term.resize(message.cols, message.rows); ready = true; retryDelay = 800;
      setConnection("已连接", "ready"); fitRows(); controls();
    } else if (message.type === "sent" && pending && pending.id === message.id) {
      if (reply.value === pending.text) reply.value = "";
      pending = null; clearTimeout(acknowledgmentTimer); setNotice(""); controls();
    } else if (message.type === "error") {
      setNotice(message.message); ended = true; ready = false; controls();
    } else if (message.type === "ended") {
      ended = true; ready = false; setConnection("会话已结束"); controls();
    }
  };
  ws.onclose = () => {
    if (socket !== ws) return;
    socket = null; ready = false; discardPending(); controls();
    if (ended) {setConnection("会话已结束"); return;}
    if (!authenticated || selected?.name !== name) return;
    setConnection("重新连接中");
    reconnectTimer = window.setTimeout(connectTerminal, retryDelay);
    retryDelay = Math.min(10000, retryDelay * 2);
    void refreshAgents();
  };
}

function openAgent(agent: Agent) {
  if (selected?.name === agent.name && socket) {workspace.classList.add("has-terminal"); return;}
  if (selected?.name !== agent.name && reply.value.trim() && !confirm("切换对话会清空当前未发送的回复，继续吗？")) return;
  disconnect(); selected = agent; ended = false; retryDelay = 800; reply.value = ""; setNotice("");
  pane.hidden = false; el("empty-selection").hidden = true; workspace.classList.add("has-terminal");
  el("terminal-title").textContent = agent.title;
  if (!mounted) {term.open(host); mounted = true;}
  fitRows();
  document.querySelectorAll<HTMLElement>(".session-row").forEach(row => row.setAttribute("aria-current", String(row.dataset.name === agent.name)));
  connectTerminal();
}

async function refreshAgents(): Promise<void> {
  if (refreshing) return; refreshing = true;
  try {
    const response = await fetch("/api/agents", { cache: "no-store" });
    if (response.status === 401) {loginScreen(); return;}
    if (!response.ok) throw new Error("读取对话失败");
    const agents: Agent[] = await response.json();
    authenticated = true; loginView.hidden = true; workspace.hidden = false; listError.hidden = true;
    rows.replaceChildren();
    if (!agents.length) {const p = document.createElement("p"); p.textContent = "电脑上还没有 Shifu 对话"; p.className = "task-heading"; rows.append(p);}
    const groups = new Map<string, { title: string; project: string; cwd: string; agents: Agent[] }>();
    for (const agent of agents) {
      const project = agent.cwd.split("/").filter(Boolean).at(-1) || "其他对话";
      const key = `${agent.cwd}\0${agent.task || ""}`;
      const group = groups.get(key) || { title: agent.task || project, project: agent.task ? project : "", cwd: agent.cwd, agents: [] };
      group.agents.push(agent); groups.set(key, group);
    }
    for (const { title, project, cwd, agents } of groups.values()) {
      const heading = document.createElement("h2"); heading.className = "task-heading"; heading.title = cwd;
      const taskName = document.createElement("span"); taskName.textContent = title; heading.append(taskName);
      if (project && project !== title) {const folder = document.createElement("span"); folder.className = "project-name"; folder.textContent = project; heading.append(folder);}
      rows.append(heading);
      for (const agent of agents) {
        const button = document.createElement("button"); button.className = "session-row"; button.dataset.name = agent.name;
        button.setAttribute("aria-current", String(selected?.name === agent.name)); button.disabled = agent.status === "exited";
        const name = document.createElement("span"); name.className = "session-title"; name.textContent = agent.title;
        const meta = document.createElement("span"); meta.className = "session-meta";
        const kind = document.createElement("span"); kind.textContent = agent.kind;
        const state = document.createElement("span"); state.className = `session-state${agent.status.startsWith("needs_") ? " waiting" : ""}`; state.textContent = statusNames[agent.status] || agent.status;
        meta.append(kind, state); button.append(name, meta); button.onclick = () => openAgent(agent); rows.append(button);
        if (selected?.name === agent.name) {
          selected = agent; el("terminal-title").textContent = agent.title;
          if (agent.status === "exited") {ended = true; disconnect(); setConnection("会话已结束");}
        }
      }
    }
  } catch {
    if (authenticated) {listError.textContent = "电脑暂时无法连接，正在重试。"; listError.hidden = false;}
    else {loginError.textContent = "电脑暂时无法连接，请稍后重试。"; loginError.hidden = false;}
  } finally { refreshing = false; }
}

loginForm.onsubmit = async event => {
  event.preventDefault();
  const button = loginForm.querySelector<HTMLButtonElement>("button")!; button.disabled = true; loginError.hidden = true;
  try {
    const response = await fetch("/api/login", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code: codeInput.value.trim() }) });
    if (!response.ok) throw new Error(response.status === 401 ? "访问码不正确" : "连接未成功，请稍后重试");
    codeInput.value = ""; await refreshAgents();
    if (authenticated && selected && !socket && !ended) connectTerminal();
  } catch (error) {loginError.textContent = error instanceof Error ? error.message : "连接失败"; loginError.hidden = false;}
  finally {button.disabled = false;}
};
el("logout").onclick = async () => {
  try {
    const response = await fetch("/api/logout", { method: "POST" });
    if (!response.ok) throw new Error();
    loginScreen(); selected = null; reply.value = ""; setNotice(""); workspace.classList.remove("has-terminal"); pane.hidden = true;
  } catch {listError.textContent = "退出未完成，请连接电脑后重试。"; listError.hidden = false;}
};
el("back").onclick = () => {workspace.classList.remove("has-terminal");};
el("refresh").onclick = () => {void refreshAgents();};
reply.oninput = controls;
el<HTMLFormElement>("reply-form").onsubmit = event => {
  event.preventDefault();
  if (!ready || pending || !reply.value.trim() || socket?.readyState !== WebSocket.OPEN) return;
  if (new TextEncoder().encode(reply.value).length > 16 * 1024) {setNotice("每次回复请控制在 16 KB 以内。"); return;}
  pending = { id: ++nextId, text: reply.value }; controls();
  socket.send(JSON.stringify({ type: "paste", ...pending }));
  acknowledgmentTimer = window.setTimeout(discardPending, 5000);
};
reply.onkeydown = event => {if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {event.preventDefault(); el<HTMLFormElement>("reply-form").requestSubmit();}};
const keys: Record<string, string> = { escape: "\x1b", tab: "\t", up: "\x1b[A", down: "\x1b[B", enter: "\r", interrupt: "\x03" };
function write(data: string) {if (ready && !pending && socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: "input", data }));}
document.querySelectorAll<HTMLButtonElement>("#keys button").forEach(button => button.onclick = () => write(keys[button.dataset.key!]));
term.onData(write);
new ResizeObserver(fitRows).observe(viewport);
window.visualViewport?.addEventListener("resize", () => {
  workspace.style.height = `${window.visualViewport!.height}px`; fitRows();
});
window.addEventListener("online", () => {if (authenticated && selected && !socket && !ended) {clearTimeout(reconnectTimer); connectTerminal();}});
setInterval(() => {void refreshAgents();}, 3000);
void refreshAgents(); controls();
