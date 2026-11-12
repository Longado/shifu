import { resourcesGet } from "./api";
import { agentLabel } from "./format";
import { findAgent, getTasks, reportError } from "./store";

/** Mirrors `Resources` in app/src-tauri/src/resources.rs. */
export interface Resources {
  agents: { name: string; bytes: number; processes: number; cpu: number }[];
  shifu: { app: number; web: number; tmux: number; cpu: number };
  /** Terminals in Shifu's tmux that no agent owns. */
  strays: { session: string; bytes: number; cpu: number }[];
  host_total: number;
  host_used_percent: number;
  /** 1 normal, 2 warning, 4 critical: the kernel's own judgement. */
  pressure: number;
}

const REFRESH_MS = 5000;

/** Binary units, as Activity Monitor counts them: 1.5 GB, 480 MB; four-digit MB read as GB. */
export function formatBytes(bytes: number): string {
  const mb = bytes / 1024 ** 2;
  return mb >= 1000 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

/** `ps` %CPU, where 100 is one core busy; idle shows nothing so the busy ones stand out. */
export function formatCpu(cpu: number): string {
  return cpu < 1 ? "" : `${Math.round(cpu)}%`;
}

export function summary(r: Resources): string {
  const total = r.agents.reduce((s, a) => s + a.bytes, 0);
  return `${r.agents.length} 个对话 · ${formatBytes(total)}`;
}

export interface MemoryActions {
  focusAgent(taskId: string, name: string): void;
  openAgent(name: string): void;
}

/** At the window's lower right: how many agents are open and the memory they take; the machine's share on the right,
 *  amber or red when the kernel says memory is tight. A click lists each agent, biggest first, with its CPU; a
 *  click on an agent opens it. Shifu's own share is split like Orca's (app, page, tmux), and terminals no agent
 *  owns are listed in amber so their memory is not hidden . */
export class MemoryBar {
  readonly element = document.createElement("div");
  private readonly line = document.createElement("button");
  private readonly list = document.createElement("div");
  private last: Resources | null = null;
  private unfolded = false;

  constructor(private readonly actions: MemoryActions) {
    this.element.className = "membar";
    this.line.className = "membar-line";
    this.list.className = "membar-list";
    this.list.id = "process-monitor";
    this.line.setAttribute("aria-controls", this.list.id);
    this.element.append(this.list, this.line);
    this.element.hidden = true;
    this.line.title = "每个对话占的内存（和活动监视器的「内存」一列同一个数），点一下看明细";
    this.line.addEventListener("click", () => {
      this.unfolded = !this.unfolded;
      this.render();
    });
    window.addEventListener("pointerdown", (e) => {
      if (this.unfolded && !this.element.contains(e.target as Node)) {
        this.unfolded = false;
        this.render();
      }
    });
    window.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && this.unfolded) {
        this.unfolded = false;
        this.render();
        this.line.focus();
      }
    });
    const refresh = () => void resourcesGet().then((r) => {
      this.last = r;
      this.render();
    }).catch(reportError);
    refresh();
    window.setInterval(refresh, REFRESH_MS);
  }

  private open(name: string): void {
    this.unfolded = false;
    this.render();
    const task = getTasks().find((t) => t.agents.includes(name));
    if (task) this.actions.focusAgent(task.id, name);
    else this.actions.openAgent(name);
  }

  private render(): void {
    const r = this.last;
    if (!r) return;
    this.element.hidden = false;
    const host = document.createElement("span");
    host.className = `membar-host${r.pressure >= 4 ? " critical" : r.pressure >= 2 ? " tight" : ""}`;
    host.textContent = `本机 ${r.host_used_percent}%`;
    host.title = `本机 ${formatBytes(r.host_total)} 内存用了 ${r.host_used_percent}%`;
    const text = document.createElement("span");
    text.className = "membar-text";
    text.textContent = summary(r);
    if (r.strays.length) {
      const stray = document.createElement("span");
      stray.className = "membar-stray-count";
      stray.textContent = ` · 无主 ${r.strays.length}`;
      text.append(stray);
    }
    const icon = document.createElement("span");
    icon.innerHTML = '<svg width="15" height="15" viewBox="0 0 15 15" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M1 8h3l2-5 3 9 2-4h3"/></svg>';
    this.line.replaceChildren(icon, text, host);
    this.line.setAttribute("aria-label", `进程监控，${summary(r)}，本机内存 ${r.host_used_percent}%`);
    this.line.setAttribute("aria-expanded", String(this.unfolded));
    this.list.hidden = !this.unfolded;
    if (!this.unfolded) return;
    const row = (name: string, bytes: number, cpu: number, hint: string, cls = "") => {
      const e = document.createElement("div");
      e.className = `membar-row ${cls}`.trim();
      const n = document.createElement("span");
      n.className = "membar-name";
      n.textContent = name;
      const c = document.createElement("span");
      c.className = "membar-cpu";
      c.textContent = formatCpu(cpu);
      const b = document.createElement("span");
      b.className = "membar-bytes";
      b.textContent = formatBytes(bytes);
      e.append(n, c, b);
      e.title = hint;
      return e;
    };
    const head = row("", 0, 0, "CPU：100% 是一个核满负荷；内存：和活动监视器同一个数", "membar-head");
    head.children[1].textContent = "CPU";
    head.children[2].textContent = "内存";
    const agents = r.agents.map((a) => {
      const agent = findAgent(a.name);
      const e = row(agent ? agentLabel(agent) : a.name, a.bytes, a.cpu, `${a.name}，${a.processes} 个进程（它自己和它开的工具、子进程）。点一下打开`, "membar-agent");
      e.addEventListener("click", () => this.open(a.name));
      e.tabIndex = 0;
      e.setAttribute("role", "button");
      e.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          this.open(a.name);
        }
      });
      return e;
    });
    const strays = r.strays.map((s) =>
      row(`无主终端 ${s.session}`, s.bytes, s.cpu, "Shifu 的 tmux 里有这个终端，但没有哪个对话认领它。确认没用的话可以在终端里关掉", "membar-stray"),
    );
    const s = r.shifu;
    const shifu = [
      row("Shifu 自己", s.app + s.web + s.tmux, s.cpu, "应用、它的网页进程和托管对话的 tmux 加起来"),
      row("程序", s.app, 0, "Shifu 主进程", "membar-sub"),
      row("页面", s.web, 0, "WebKit 替 Shifu 跑页面、图形和网络的几个进程", "membar-sub"),
      row("tmux", s.tmux, 0, "托管所有对话终端的 tmux 服务", "membar-sub"),
    ];
    this.list.replaceChildren(head, ...agents, ...strays, ...shifu);
  }
}
