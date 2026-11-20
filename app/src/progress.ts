/** 进度页: every project with a GOALS.md checklist, how much of it is done, and which agents work there now. */
import { taskGoalAssign, taskGoalSave, taskGoalTell, type GoalNotification } from "./api";
import { type AgentView, agentLabel, statusLabel, viewKey, sortAgents } from "./format";
import { getAgentOrder, reportError } from "./store";
import { inDraggedOrder, projectName, type Task, taskAgents, taskLabel } from "./tasks";

function goalNotificationText(rows: GoalNotification[]): string {
  const labels = {
    queued: "已排队，等待发送",
    written: "已写入终端，可打开对话查看",
    unknown: "发送结果未确认，请在原对话核对",
    unavailable: "当前收不到消息",
  };
  return rows.map(row => `${row.previous ? "原负责人 " : ""}${row.agent} ${labels[row.state]}`).join("。");
}

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
  todos?: TodoRow[];
  todos_error?: string | null;
}
/** Mirrors `TodoRow` in app/src-tauri/src/goals.rs: one task in the Workspace's .data/tasks.db. */
export interface TodoRow {
  project: string;
  title: string;
  status: string;
  due: string | null;
}

/** Mirrors `TaskGoal` in app/src-tauri/src/taskgoals.rs: a task's 终点／重点／清单 file. */
export interface TaskGoal {
  path: string;
  /** Empty when the task has no file yet. */
  text: string;
  read_error?: string;
}

export const GOAL_TEMPLATE = "# 终点\n\n\n# 重点\n\n\n# 清单\n\n\n# 记录\n\n";

export interface ParsedTaskGoal {
  /** What done looks like. */
  end: string;
  /** The one thing to push now. */
  focus: string;
  items: GoalItem[];
  /** What the task's conversations wrote down for each other: findings, decisions, pitfalls. Oldest first. */
  notes: string[];
}

/** ` @对话名` at the end of a checklist line: the conversation that line was given to. */
const OWNER = /\s@(\S+)$/;

/** Text under `# 终点` and `# 重点`; one note per line under `# 记录`; checklist lines from anywhere in the file,
 *  each without the owner it may end in. */
export function parseTaskGoal(text: string): ParsedTaskGoal {
  const sections: Record<string, string[]> = {};
  let current = "";
  for (const line of text.split("\n")) {
    const heading = /^#+\s*(.+?)\s*$/.exec(line);
    if (heading) current = heading[1];
    else if (line.trim() && !ITEM.test(line)) (sections[current] ??= []).push(line.trim());
  }
  return {
    end: (sections["终点"] ?? []).join(" "),
    focus: (sections["重点"] ?? []).join(" "),
    items: parseGoals(text, "").items.map((item) => {
      const owner = OWNER.exec(item.text);
      const rest = owner ? item.text.slice(0, owner.index).trim() : "";
      return owner && rest ? { ...item, text: rest, owner: owner[1] } : item;
    }),
    notes: (sections["记录"] ?? []).map((l) => l.replace(/^[-*+]\s+/, "").replace(/\*\*|__|`/g, "").trim()).filter(Boolean),
  };
}

/** A task file says something beyond its empty headings. */
const written = (g: ParsedTaskGoal): boolean => Boolean(g.end || g.focus || g.items.length || g.notes.length);

export interface GoalItem {
  done: boolean;
  text: string;
  /** Task files only: the name of the conversation this line was given to. */
  owner?: string;
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
  owner?: string;
  tag?: "正在做" | "下一步" | "停在这" | "卡住";
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

export interface TodoProject {
  project: string;
  done: number;
  total: number;
  /** Earliest due date (YYYY-MM-DD) among the open tasks. */
  nextDue: string | null;
  /** Open tasks only: doing, then blocked, then todo; within each, earliest due first. */
  rows: Row[];
}

const OPEN_ORDER: Record<string, number> = { doing: 0, blocked: 1, todo: 2 };
const byDue = (a: string | null, b: string | null): number => (a ?? "9999").localeCompare(b ?? "9999");
/** "2026-11-20" -> "11-20". */
const shortDate = (d: string): string => d.slice(5, 10);

/** tasks.db rows grouped by project; projects with the nearest deadline first, undated ones by name after. */
export function todoProjects(rows: readonly TodoRow[]): TodoProject[] {
  const groups = new Map<string, TodoRow[]>();
  for (const r of rows) groups.set(r.project, [...(groups.get(r.project) ?? []), r]);
  return [...groups].map(([project, tasks]) => {
    const open = tasks
      .filter((t) => t.status in OPEN_ORDER)
      .sort((a, b) => OPEN_ORDER[a.status] - OPEN_ORDER[b.status] || byDue(a.due, b.due));
    const dues = open.flatMap((t) => (t.due ? [t.due] : [])).sort();
    return {
      project,
      done: tasks.filter((t) => t.status === "done").length,
      total: tasks.length,
      nextDue: dues[0] ?? null,
      rows: open.map((t): Row => {
        const row: Row = { done: false, text: t.due ? `${t.title}  ${shortDate(t.due)} 截止` : t.title };
        if (t.status === "doing") row.tag = "正在做";
        else if (t.status === "blocked") row.tag = "卡住";
        return row;
      }),
    };
  }).sort((a, b) => byDue(a.nextDue, b.nextDue) || a.project.localeCompare(b.project));
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

interface Card {
  /** Identifies the card across refreshes (keeps it open) and shows as its tooltip. */
  key: string;
  title: string;
  sub: string | null;
  rows: Row[];
  agents: AgentView[];
  /** Defaults to the rows' own count; the task cards count finished tasks they no longer list. */
  done?: number;
  total?: number;
  /** A Shifu task's card: its 终点／重点 show under the title, and opening it offers editing and telling. */
  goal?: { id: string; parsed: ParsedTaskGoal; text: string; readError?: string };
}

export class ProgressPage {
  readonly element = el("div", "progress-page");
  private goals: Goals = { files: [], agents: {} };
  private agents: readonly AgentView[] = [];
  /** Cards start folded; these are the ones opened by a click. */
  private readonly open = new Set<string>();
  private taskGoals: Record<string, TaskGoal> = {};
  /** The task whose file is being edited on the page. */
  private editing: { id: string; form: HTMLFormElement } | null = null;
  /** The last thing a 让会话起草／告诉会话／交给 click did, per task. */
  private readonly told = new Map<string, string>();
  private readonly pendingActions = new Set<string>();
  /** The checklist row whose conversations are listed to pick from: `<task id>:<row>`. */
  private picking: string | null = null;
  /** GOALS.md lists, the Workspace to-do table and WorkBuddy: folded under one line unless opened. */
  private others = false;
  /** Tasks without a written file: folded under one line unless opened. */
  private bareOpen = false;

  constructor(
    private readonly openAgent: (name: string) => void,
    /** Tasks in sidebar order. */
    private readonly tasks: () => Task[] = () => [],
    private readonly reloadTaskGoals: () => void = () => {},
  ) {
    this.element.hidden = true;
  }

  setTaskGoals(goals: Record<string, TaskGoal>): void {
    this.taskGoals = goals;
    // Polling must not throw away what the user is typing.
    if (this.editing === null) this.render();
  }

  setGoals(goals: Goals): void {
    this.goals = goals;
    if (this.editing === null) this.render();
  }

  setAgents(agents: readonly AgentView[]): void {
    this.agents = agents;
    if (!this.element.hidden && this.editing === null) this.render();
  }

  private async submitAction(key: string, submit: () => Promise<void>): Promise<void> {
    if (this.pendingActions.has(key)) return;
    this.pendingActions.add(key);
    if (!this.editing) this.render();
    try {
      await submit();
    } catch (error) {
      reportError(error);
    } finally {
      this.pendingActions.delete(key);
      if (!this.editing) this.render();
    }
  }

  render(): void {
    const active = document.activeElement;
    const focusedKey = active && this.element.contains(active) && active.classList.contains("goal-head")
      ? (active as HTMLButtonElement).title : null;
    const projects = projectProgress(this.goals, this.agents);
    const body = el("div", "progress-body");
    body.append(el("h1", "progress-title", "进度"));
    const todos = todoProjects(this.goals.todos ?? []);
    const sessions = wbShown(this.goals.workbuddy ?? []);
    // Tasks that say where they are going come first; the rest fold under one line until someone writes theirs.
    const cards = this.tasks().map((task) => {
      const text = this.taskGoals[task.id]?.text ?? "";
      const readError = this.taskGoals[task.id]?.read_error;
      const parsed = parseTaskGoal(text);
      const agents = inDraggedOrder(taskAgents(task, this.agents), getAgentOrder()[task.id]).filter((a) => !a.exited && a.status !== "exited");
      const title = taskLabel(task, this.agents);
      return { written: Boolean(readError) || written(parsed), card: () => this.card({
        key: `task:${task.id}`, title, sub: null,
        rows: goalRows(parsed.items), agents, goal: { id: task.id, parsed, text, readError },
      }) };
    });
    for (const c of cards.filter((c) => c.written)) body.append(c.card());
    const bare = cards.filter((c) => !c.written);
    if (bare.length > 0) body.append(this.bareHead(bare.length));
    if (this.bareOpen) for (const c of bare) body.append(c.card());
    const other = projects.length + todos.length + sessions.length;
    const otherError = this.goals.todos_error || this.goals.workbuddy_error;
    if (cards.length === 0 && other === 0 && !otherError) body.append(el("div", "progress-empty", "还没有任务"));
    if (other > 0 || otherError) body.append(this.othersHead(projects.length, todos.length, sessions.length));
    if (this.others) this.renderOthers(body, projects, todos, sessions);
    this.element.replaceChildren(body);
    // Toggling and polling rebuild the cards; keep keyboard navigation on the same heading.
    if (focusedKey !== null) {
      Array.from(this.element.querySelectorAll<HTMLButtonElement>(".goal-head"))
        .find((head) => head.title === focusedKey)?.focus({ preventScroll: true });
    }
  }

  /** The one line the tasks without a written file fold into. */
  private bareHead(count: number): HTMLElement {
    const head = el("button", "goal-head progress-bare") as HTMLButtonElement;
    head.type = "button";
    head.title = "bare";
    head.setAttribute("aria-expanded", String(this.bareOpen));
    const name = el("div", "goal-name");
    name.append(el("span", "goal-caret", this.bareOpen ? "▾" : "▸"), `${count} 个任务还没写终点`);
    head.append(name);
    head.addEventListener("click", () => { this.bareOpen = !this.bareOpen; this.render(); });
    return head;
  }

  /** The one line the other lists fold into; says how many of each it holds. */
  private othersHead(projects: number, todos: number, sessions: number): HTMLElement {
    const head = el("button", "goal-head progress-others") as HTMLButtonElement;
    head.type = "button";
    head.title = "others";
    head.setAttribute("aria-expanded", String(this.others));
    const name = el("div", "goal-name");
    name.append(el("span", "goal-caret", this.others ? "▾" : "▸"), "其他清单");
    const parts = [["项目清单", projects], ["待办", todos], ["WorkBuddy", sessions]].filter(([, n]) => n).map(([l, n]) => `${l} ${n}`);
    head.append(name, el("div", "goal-frac", parts.join("　")));
    head.addEventListener("click", () => { this.others = !this.others; this.render(); });
    return head;
  }

  private renderOthers(body: HTMLElement, projects: ProjectProgress[], todos: TodoProject[], sessions: WbSession[]): void {
    if (projects.length > 0) body.append(el("h2", "progress-section", "项目清单（GOALS.md）"));
    for (const p of projects) body.append(this.card({ key: p.dir, title: p.list.title, sub: null, rows: goalRows(p.list.items), agents: p.agents }));

    if (todos.length > 0 || this.goals.todos_error) {
      body.append(el("h2", "progress-section", "待办"));
      if (this.goals.todos_error) body.append(el("div", "progress-empty", this.goals.todos_error));
      for (const t of todos) {
        const sub = `${t.rows.length} 件没做完${t.nextDue ? `  最近 ${t.nextDue.slice(5, 10)} 截止` : ""}`;
        body.append(this.card({ key: `todo:${t.project}`, title: t.project, sub, rows: t.rows, agents: [], done: t.done, total: t.total }));
      }
    }

    if (sessions.length > 0 || this.goals.workbuddy_error) {
      body.append(el("h2", "progress-section", "WorkBuddy"));
      if (this.goals.workbuddy_error) body.append(el("div", "progress-empty", this.goals.workbuddy_error));
      const now = Date.now();
      for (const s of sessions) {
        const sub = `${WB_STATUS[s.status] ?? s.status}  ${wbWhen(s.updated_at, now)}${s.tasks.length === 0 ? "  还没拆成任务" : ""}`;
        body.append(this.card({ key: s.id, title: s.title || "（没有标题）", sub, rows: wbRows(s.tasks, s.status === "completed"), agents: [] }));
      }
    }
  }

  private card({ key, title, sub, rows, agents, goal, done = rows.filter((r) => r.done).length, total = rows.length }: Card): HTMLElement {
    const opened = this.open.has(key);
    const box = el("section", opened ? "goal-project open" : "goal-project");
    const head = el("button", "goal-head");
    head.setAttribute("aria-expanded", String(opened));
    head.title = key;
    head.addEventListener("click", () => {
      if (!this.open.delete(key)) this.open.add(key);
      this.render();
    });
    const name = el("div", "goal-name");
    name.append(el("span", "goal-caret", opened ? "▾" : "▸"), title);
    const unwritten = goal && !written(goal.parsed);
    head.append(name, el("div", "goal-frac", !unwritten && total > 0 ? `${done}/${total}` : ""));
    box.append(head);
    if (goal?.readError) {
      box.append(el("div", "goal-sub", opened ? goal.readError : "任务文件读取失败"));
      if (opened && this.editing?.id === goal.id) box.append(this.editing.form);
      return box;
    }
    if (sub) box.append(el("div", "goal-sub", sub));
    if (goal && !unwritten) {
      const { end, focus } = goal.parsed;
      // Folded: the one thing to push now (or where it is going). Opened: both, labelled.
      if (!opened) {
        if (focus || end) box.append(el("div", "goal-gist", focus || end));
      } else {
        if (end) box.append(this.goalLine("终点", end));
        if (focus) box.append(this.goalLine("重点", focus));
      }
    }

    if (!opened) return box;
    // Two conversations can carry the same topic; then their names tell them apart.
    const labels = agents.map(agentLabel);
    const called = (a: AgentView): string => {
      const label = agentLabel(a);
      return labels.filter((l) => l === label).length > 1 ? `${label} · ${a.name}` : label;
    };
    if (goal && this.editing?.id === goal.id) {
      box.append(this.editing.form);
      return box;
    }
    // Only Claude and Codex conversations can be handed a line: the others cannot be sent a queued message.
    // By name, not by status: the list must not reorder while the user is choosing from it.
    const hands = goal
      ? agents.filter((a) => a.kind === "claude" || a.kind === "codex").sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }))
      : [];
    const list = el("ol", "goal-items");
    rows.forEach((row, index) => {
      const li = el("li", row.done ? "goal-item done" : row.tag ? "goal-item next" : "goal-item");
      const mark = el("span", "goal-mark");
      if (row.done) mark.innerHTML = CHECK;
      li.append(mark, el("span", "goal-text", row.text));
      if (row.tag) li.append(el("span", "goal-tag", row.tag));
      list.append(li);
      if (!goal || (row.done && !row.owner)) return;
      const owner = agents.find((a) => a.name === row.owner);
      const who = owner ? called(owner) : row.owner;
      if (row.done || hands.length === 0) {
        if (!who) return;
        // A finished line's conversation opens on click, to see how it was done.
        const done = el("button", "goal-owner goal-give", who) as HTMLButtonElement;
        done.type = "button";
        if (owner) done.addEventListener("click", () => this.openAgent(owner.name));
        else done.disabled = true;
        li.append(done);
        return;
      }
      const pickKey = `${goal.id}:${index}`;
      const give = el("button", who ? "goal-owner goal-give" : "goal-give", who ?? "交给…") as HTMLButtonElement;
      give.type = "button";
      give.disabled = this.pendingActions.has(pickKey);
      give.title = who ? "换一个对话来做" : "把这一条交给任务里的一个对话";
      give.addEventListener("click", () => {
        this.picking = this.picking === pickKey ? null : pickKey;
        this.render();
      });
      li.append(give);
      if (this.picking !== pickKey) return;
      const pick = el("li", "goal-pick");
      for (const a of hands) {
        const chip = el("button", "goal-person") as HTMLButtonElement;
        chip.type = "button";
        chip.disabled = this.pendingActions.has(pickKey);
        chip.append(el("span", `dot s-${viewKey(a)}`), el("span", "goal-person-name", called(a)), el("span", "goal-person-state", statusLabel(viewKey(a))));
        chip.addEventListener("click", () => void this.submitAction(pickKey, async () => {
          const saved = await taskGoalAssign(goal.id, index, row.text, a.name);
          if (this.picking === pickKey) this.picking = null;
          this.told.set(goal.id, `已保存分工。「${row.text}」由${called(a)}负责。${goalNotificationText(saved.notifications)}`);
          if (this.taskGoals[goal.id]?.text === goal.text) {
            this.taskGoals = { ...this.taskGoals, [goal.id]: saved.goal };
          }
          this.reloadTaskGoals();
        }));
        pick.append(chip);
      }
      list.append(pick);
    });
    box.append(list);
    if (goal && goal.parsed.notes.length > 0) {
      box.append(el("div", "goal-notes-label", "记录"));
      const notes = el("ul", "goal-notes");
      for (const note of goal.parsed.notes) notes.append(el("li", "goal-note", note));
      box.append(notes);
    }
    if (goal) box.append(this.goalActions(goal.id, !unwritten));
    return box;
  }

  private goalLine(label: string, text: string): HTMLElement {
    const line = el("div", "goal-line");
    line.append(el("span", "goal-line-label", label), el("span", "goal-line-text", text));
    return line;
  }

  private goalActions(id: string, isWritten: boolean): HTMLElement {
    const bar = el("div", "goal-actions");
    const button = (label: string, run: () => void, disabled = false) => {
      const b = el("button", "goal-action", label) as HTMLButtonElement;
      b.type = "button";
      b.disabled = disabled;
      b.addEventListener("click", run);
      bar.append(b);
    };
    button(isWritten ? "编辑" : "自己写", () => {
      this.editing = { id, form: this.editor(id, this.taskGoals[id]?.text ?? "") };
      this.render();
    });
    const key = `tell:${id}`;
    button(isWritten ? "告诉这个任务的会话" : "让会话起草", () => void this.submitAction(key, async () => {
      const result = await taskGoalTell(id, !isWritten);
      this.told.set(id, `${isWritten ? "" : "起草请求已处理。"}${goalNotificationText(result.notifications)}`);
      this.reloadTaskGoals();
    }), this.pendingActions.has(key));
    const note = this.told.get(id);
    if (note) bar.append(el("span", "goal-told", note));
    return bar;
  }

  private editor(id: string, text: string): HTMLFormElement {
    const form = el("form", "goal-editor") as HTMLFormElement;
    const area = el("textarea", "goal-editor-text") as HTMLTextAreaElement;
    area.value = text.trim() ? text : GOAL_TEMPLATE;
    area.rows = 12;
    area.spellcheck = false;
    const save = el("button", "goal-action primary", "保存") as HTMLButtonElement;
    save.type = "submit";
    const cancel = el("button", "goal-action", "取消") as HTMLButtonElement;
    cancel.type = "button";
    const done = () => {
      if (this.editing?.form !== form) return;
      this.editing = null;
      this.render();
    };
    cancel.addEventListener("click", done);
    // The file the typing is on top of. The task's conversations tick and annotate the same file.
    let expected = text;
    const conflict = el("div", "goal-conflict");
    conflict.hidden = true;
    form.addEventListener("submit", async (e) => {
      e.preventDefault();
      if (save.disabled) return;
      const submitted = area.value;
      save.disabled = true;
      form.setAttribute("aria-busy", "true");
      try {
        const { saved, goal } = await taskGoalSave(id, expected, submitted);
        this.taskGoals = { ...this.taskGoals, [id]: goal };
        this.reloadTaskGoals();
        if (this.editing?.form !== form) {
          if (!this.editing) this.render();
          return;
        }
        expected = goal.text;
        if (saved) {
          if (area.value === submitted) { done(); return; }
          conflict.replaceChildren(el("div", "goal-conflict-note", "上一版已保存。可以继续编辑，再保存刚输入的内容。"));
        } else {
          const now = el("textarea", "goal-editor-text") as HTMLTextAreaElement;
          now.value = goal.text;
          now.readOnly = true;
          now.rows = 8;
          now.setAttribute("aria-label", "文件现在的内容");
          conflict.replaceChildren(el("div", "goal-conflict-note", "会话在你编辑时改了这份文件，还没保存。下面是文件现在的内容，把要留的并进上面再保存。"), now);
        }
        conflict.hidden = false;
      } catch (error) {
        reportError(error);
      } finally {
        save.disabled = false;
        form.setAttribute("aria-busy", "false");
      }
    });
    const bar = el("div", "goal-actions");
    bar.append(save, cancel);
    form.append(area, conflict, bar);
    queueMicrotask(() => area.focus());
    return form;
  }
}
