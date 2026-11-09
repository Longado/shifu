/** 进度页: every project with a GOALS.md checklist, how much of it is done, and which agents work there now. */
import { type AgentView, agentLabel, statusLabel, viewKey, sortAgents } from "./format";
import { projectName } from "./tasks";

export interface GoalFile {
  dir: string;
  text: string;
}
/** Mirrors `WbSession` in app/src-tauri/src/workbuddy.rs: a WorkBuddy conversation and the tasks it split into. */
export interface WbSession {
  id: string;
  title: string;
  status: string;
  /** Unix milliseconds. */
  updated_at: number;
  tasks: { subject: string; status: string }[];
}
/** Mirrors `Goals` in app/src-tauri/src/goals.rs. */
export interface Goals {
  files: GoalFile[];
  /** Agent name -> project dir. */
  agents: Record<string, string>;
  workbuddy?: WbSession[];
  workbuddy_error?: string | null;
}

export interface GoalItem {
  done: boolean;
  text: string;
}
export interface GoalList {
  title: string;
  items: GoalItem[];
}

const ITEM = /^\s*[-*+] \[([ xX])\]\s+(.+)$/;

/** Title from the first `# ` heading (else the folder name); items from `- [ ]` / `- [x]` lines anywhere. */
export function parseGoals(text: string, dir: string): GoalList {
  const lines = text.split("\n");
  const heading = lines.find((l) => l.startsWith("# "));
  const items = lines.flatMap((l) => {
    const m = ITEM.exec(l);
    return m ? [{ done: m[1] !== " ", text: m[2].replace(/\*\*|`/g, "").trim() }] : [];
  });
  return { title: heading ? heading.slice(2).trim() : projectName(dir), items };
}

export interface ProjectProgress {
  dir: string;
  list: GoalList;
  done: number;
  total: number;
  /** Agents placed in this project that have not exited, in sidebar order. */
  agents: AgentView[];
}

/** Projects with at least one checkbox; the ones with agents at work first, then by folder. */
export function projectProgress(goals: Goals, agents: readonly AgentView[]): ProjectProgress[] {
  return goals.files
    .map((f) => {
      const list = parseGoals(f.text, f.dir);
      const here = agents.filter((a) => !a.exited && a.status !== "exited" && goals.agents[a.name] === f.dir);
      return { dir: f.dir, list, done: list.items.filter((i) => i.done).length, total: list.items.length, agents: sortAgents(here) };
    })
    .filter((p) => p.total > 0)
    .sort((a, b) => b.agents.length - a.agents.length || a.dir.localeCompare(b.dir));
}

/** How many WorkBuddy conversations the page shows. */
export const WB_SHOWN = 5;

/** A checklist row as the page draws it; `tag` marks the step being worked on or the next one. */
export interface Row {
  done: boolean;
  text: string;
  tag?: "正在做" | "下一步" | "停在这";
}

/** GOALS.md: the first unchecked item is the next step. */
export function goalRows(items: readonly GoalItem[]): Row[] {
  const next = items.findIndex((i) => !i.done);
  return items.map((i, n) => (n === next ? { ...i, tag: "下一步" } : { ...i }));
}

/** WorkBuddy: tasks it marked in progress are 正在做; with none, the first pending one is 下一步. A conversation that
 *  has ended can still have steps marked in progress (it stopped there), so it only marks where it stopped. */
export function wbRows(tasks: WbSession["tasks"], ended = false): Row[] {
  if (ended) {
    const stop = tasks.findIndex((t) => t.status !== "completed");
    return tasks.map((t, n) => (n === stop ? { done: false, text: t.subject, tag: "停在这" } : { done: t.status === "completed", text: t.subject }));
  }
  const doing = tasks.some((t) => t.status === "in_progress");
  const next = doing ? -1 : tasks.findIndex((t) => t.status !== "completed");
  return tasks.map((t, n) => {
    const row: Row = { done: t.status === "completed", text: t.subject };
    if (t.status === "in_progress") row.tag = "正在做";
    else if (n === next) row.tag = "下一步";
    return row;
  });
}

/** Conversations worth showing: the ones with a task list or still running, newest first. */
export function wbShown(sessions: readonly WbSession[]): WbSession[] {
  return sessions.filter((s) => s.tasks.length > 0 || s.status === "working").slice(0, WB_SHOWN);
}

const WB_STATUS: Record<string, string> = { planning: "在规划", working: "进行中", completed: "已结束" };

/** "今天 14:15" or "8月13日". */
export function wbWhen(ms: number, nowMs: number): string {
  const d = new Date(ms);
  const today = new Date(nowMs);
  const pad = (n: number) => String(n).padStart(2, "0");
  return d.toDateString() === today.toDateString() ? `今天 ${pad(d.getHours())}:${pad(d.getMinutes())}` : `${d.getMonth() + 1}月${d.getDate()}日`;
}

export const percent = (done: number, total: number): number => (total === 0 ? 0 : Math.round((done / total) * 100));

function el(tag: string, className: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

const CHECK = `<svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 6.2 5 8.5l4.5-5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`;

export class ProgressPage {
  readonly element = el("div", "progress-page");
  private goals: Goals = { files: [], agents: {} };
  private agents: readonly AgentView[] = [];

  constructor(private readonly openAgent: (name: string) => void) {
    this.element.hidden = true;
  }

  setGoals(goals: Goals): void {
    this.goals = goals;
    this.render();
  }

  setAgents(agents: readonly AgentView[]): void {
    this.agents = agents;
    if (!this.element.hidden) this.render();
  }

  render(): void {
    const projects = projectProgress(this.goals, this.agents);
    const body = el("div", "progress-body");
    body.append(el("h1", "progress-title", "进度"));
    if (projects.length === 0) body.append(el("div", "progress-empty", "还没有项目写 GOALS.md 勾选清单"));
    for (const p of projects) body.append(this.card(p.list.title, p.dir, null, goalRows(p.list.items), p.agents));

    const sessions = wbShown(this.goals.workbuddy ?? []);
    if (sessions.length > 0 || this.goals.workbuddy_error) {
      body.append(el("h2", "progress-section", "WorkBuddy"));
      if (this.goals.workbuddy_error) body.append(el("div", "progress-empty", this.goals.workbuddy_error));
      const now = Date.now();
      for (const s of sessions) {
        const sub = `${WB_STATUS[s.status] ?? s.status}  ${wbWhen(s.updated_at, now)}${s.tasks.length === 0 ? "  还没拆成任务" : ""}`;
        body.append(this.card(s.title || "（没有标题）", s.id, sub, wbRows(s.tasks, s.status === "completed"), []));
      }
    }
    this.element.replaceChildren(body);
  }

  private card(title: string, tooltip: string, sub: string | null, rows: Row[], agents: AgentView[]): HTMLElement {
    const done = rows.filter((r) => r.done).length;
    const box = el("section", "goal-project");
    const head = el("div", "goal-head");
    const name = el("div", "goal-name", title);
    name.title = tooltip;
    const count = el("div", "goal-count");
    if (rows.length > 0) count.append(el("span", "goal-frac", `${done}/${rows.length}`), el("span", "goal-pct", `${percent(done, rows.length)}%`));
    head.append(name, count);
    box.append(head);
    if (sub) box.append(el("div", "goal-sub", sub));

    if (rows.length > 0) {
      const track = el("div", "goal-track");
      const fill = el("div", "goal-fill");
      fill.style.width = `${percent(done, rows.length)}%`;
      track.append(fill);
      box.append(track);
    }

    if (agents.length > 0) {
      const people = el("div", "goal-people");
      for (const a of agents) {
        const chip = el("button", "goal-person");
        const key = viewKey(a);
        chip.append(el("span", `dot s-${key}`), el("span", "goal-person-name", agentLabel(a)), el("span", "goal-person-state", statusLabel(key)));
        chip.title = a.last_message ?? a.name;
        chip.addEventListener("click", () => this.openAgent(a.name));
        people.append(chip);
      }
      box.append(people);
    }

    const list = el("ol", "goal-items");
    for (const row of rows) {
      const li = el("li", row.done ? "goal-item done" : row.tag ? "goal-item next" : "goal-item");
      const mark = el("span", "goal-mark");
      if (row.done) mark.innerHTML = CHECK;
      li.append(mark, el("span", "goal-text", row.text));
      if (row.tag) li.append(el("span", "goal-tag", row.tag));
      list.append(li);
    }
    box.append(list);
    return box;
  }
}
