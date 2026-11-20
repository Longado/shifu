import { progressSummary } from "./supervision";
import { getPanelData } from "dockview";
import { AGENT_MIME, appName, searchConversations } from "./api";
import { findAgents, type Found, highlight, type Hit } from "./search";
import { type AgentView, agentLabel, ageText, displayKey, statusHint, statusLabel, statusLine, viewKey, waitingFor } from "./format";
import { type Mark, vendorIcon } from "./vendoricons";
import { type MenuItem, showMenu } from "./menu";
import { getAgentOrder, getAgents, getCurrentId, getProjectOrder, getTaskOrder, getTasks, onError, reportError, subscribe } from "./store";
import { ENDS } from "./tab";
import { bindSidebarReorder } from "./sidebarreorder";
import {
  type Project,
  projectsOf,
  type Task,
  inDraggedOrder,
  taskAgents,
  taskLabel,
  taskNeedsYou,
  taskStatus,
  waitingList,
  type Waiting,
} from "./tasks";

export interface SidebarActions {
  selectTask(id: string): void;
  /** A "在等你" row: show its task and focus the agent. */
  focusAgent(taskId: string, name: string): void;
  openRight(name: string): void;
  openBelow(name: string): void;
  revealAgent(name: string): void;
  resume(name: string): void;
  killAgent(name: string): void;
  /** An agent's tab or row was dropped on another task: it now belongs there. */
  moveAgent(name: string, taskId: string): void;
  /** A project folder was dropped on another: this is the whole new sidebar order, by path. */
  reorderProject(paths: string[]): void;
  reorderTask(project: string, ids: string[]): void;
  reorderAgents(task: string, names: string[]): void;
  renameTask(id: string, name: string): void;
  revealProject(path: string): void;
  deleteTask(id: string): void;
  /** `project` preselects a folder (the "+" on a project row). */
  requestNewTask(anchor: HTMLElement, project?: string): void;
  /** The 进度 row: every project's GOALS.md checklist in the main area. */
  showProgress(): void;
  /** A pinned role's row (项目经理, 每日分析): its task; started (after asking which agent, the first time) when it has none. */
  openRole(role: Role, anchor: HTMLElement): void;
  /** Forget the role's agent and model; the next start asks again. */
  repickRole(role: Role): void;
}

export type Role = "pm" | "daily";

const ROLE_ICONS: Record<Role, string> = {
  pm: '<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2.5" y="5.5" width="13" height="10" rx="2"/><path d="M6.5 5.5v-2h5v2M2.5 9.5a16 16 0 0 0 13 0M9 9v3"/></svg>',
  daily: '<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2.5" y="3.5" width="13" height="12" rx="2"/><path d="M2.5 7.5h13M6 2v3M12 2v3M6 12.5l2 1.5 4-3.5"/></svg>',
};

/** A row pinned at the top of the sidebar for a resident role. Its task (the one whose project is the role's own
 *  folder) is shown here instead of under a project. */
class PinnedRow {
  readonly element = el("button", "row nav-row pm-row");
  private readonly dot = el("span", "dot");
  private readonly note = el("span", "pm-note");
  dir: string | null = null;

  constructor(actions: SidebarActions, role: Role, private readonly name: string, private readonly description: string) {
    const icon = el("span", "pm-icon");
    icon.innerHTML = ROLE_ICONS[role];
    icon.append(this.dot);
    this.element.append(icon, el("span", "pm-name", name), this.note);
    this.element.setAttribute("aria-label", name);
    this.element.title = description;
    this.note.hidden = true;
    this.element.addEventListener("click", () => actions.openRole(role, this.element));
    this.element.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu(e.clientX, e.clientY, [
        { label: "换 agent 或模型", hint: `下次开${name}时重新选`, run: () => actions.repickRole(role) },
      ]);
    });
  }

  /** `waiting`: how many things wait for 用户 elsewhere (only the project manager shows it). */
  update(currentId: string | null, waiting = 0): void {
    const task = this.dir === null ? undefined : getTasks().find((t) => t.project === this.dir);
    this.note.textContent = waiting ? String(waiting) : "";
    this.note.hidden = waiting === 0;
    this.note.title = `${waiting} 件事等你`;
    const status = task ? taskStatus(task, getAgents()) : null;
    const hint = status === "unready" || status === "asked" ? `${statusLabel(status)}：${statusHint(status)}` : "";
    this.element.title = hint || this.description;
    this.element.setAttribute("aria-label", [this.name, waiting ? `${waiting} 件事等你` : "", hint].filter(Boolean).join("，"));
    this.dot.className = `dot ${status ? `s-${status}` : "pm-idle"}`;
    this.element.classList.toggle("active", !!task && task.id === currentId);
  }
}

const ERROR_SHOW_MS = 8000;
/** Wait this long after the last key before reading transcripts. */
const SEARCH_DEBOUNCE_MS = 200;
const REFRESH_AGE_MS = 15000;
const KIND_NAMES: Record<string, string> = { claude: "Claude", codex: "Codex", codebuddy: "CodeBuddy", kimi: "Kimi", antigravity: "Antigravity", stepfun: "StepFun", qwen: "Qwen Code" };

const MARKED = new Set(["claude", "codex", "codebuddy", "kimi", "antigravity", "stepfun"]);

/** The agent's own mark (Claude, Codex, CodeBuddy, Kimi), named on hover; empty for other programs. Redrawn only
 *  when the kind changes, so a refresh does not rebuild the SVG. */
function paintMark(box: HTMLElement, kind: string): void {
  if (box.dataset.kind === kind) return;
  box.dataset.kind = kind;
  box.title = KIND_NAMES[kind] ?? kind;
  box.replaceChildren(...(MARKED.has(kind) ? [vendorIcon(kind as Mark, "row-mark")] : []));
}

const nowSecs = () => Math.floor(Date.now() / 1000);

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text = ""): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  e.className = className;
  if (text) e.textContent = text;
  return e;
}

/** Make `parent`'s children exactly `wanted`, in order, moving only what is out of place so a drag in progress
 *  is not cancelled by a refresh. */
function syncChildren(parent: HTMLElement, wanted: readonly HTMLElement[]): void {
  const keep = new Set<Element>(wanted);
  for (const child of Array.from(parent.children)) if (!keep.has(child)) child.remove();
  wanted.forEach((e, i) => {
    if (parent.children[i] !== e) parent.insertBefore(e, parent.children[i] ?? null);
  });
}

/** Rows built once per key and reused; keys that are no longer wanted are dropped. */
function cached<V>(cache: Map<string, V>, keys: readonly string[], make: (key: string) => V): V[] {
  for (const key of [...cache.keys()]) if (!keys.includes(key)) cache.delete(key);
  return keys.map((key) => {
    let v = cache.get(key);
    if (!v) {
      v = make(key);
      cache.set(key, v);
    }
    return v;
  });
}

const chevron =
  '<svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M3 2l3 3-3 3"/></svg>';
const plus =
  '<svg width="12" height="12" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M7 2.5v9M2.5 7h9"/></svg>';

/** When a row stands for a whole task (a task with one agent), the task's actions replace the agent's. */
interface TaskHooks {
  menu(): MenuItem[];
  /** Rename in place; `name` is the element showing the name. */
  edit(name: HTMLElement): void;
}

/** One agent row. Built once and updated in place so a drag in progress is not cancelled by a refresh.
 *  With `taskHooks` it is a task's whole line: one line (name and state), plus the last reply when `current`. */
/** The conversation row being dragged; a drag's data cannot be read until the drop. */
let draggingRow: { task: string; agent: string } | null = null;

class Row {
  readonly element = el("div", "row");
  readonly name = el("span", "row-name");
  private readonly dot = el("span", "dot");
  private readonly mark = el("span", "row-mark-box");
  private readonly stateRight = el("span", "row-state");
  private readonly where = el("span", "row-where");
  private readonly state = el("span", "row-state");
  private readonly sub = el("div", "row-sub");
  private readonly message = el("div", "row-msg");

  constructor(
    readonly agentName: string,
    private readonly actions: SidebarActions,
    private readonly taskId: string,
    private readonly taskHooks?: TaskHooks,
  ) {
    this.element.draggable = !taskHooks;
    this.element.dataset.agent = agentName;
    const head = el("div", "row-head");
    head.append(this.dot, this.mark, this.name, this.stateRight);
    this.sub.append(this.where, this.state);
    this.element.append(head, this.sub, this.message);
    this.element.classList.toggle("row-task", !!taskHooks);
    this.stateRight.hidden = !taskHooks;
    this.sub.hidden = !!taskHooks;

    this.element.addEventListener("click", () => this.open());
    if (taskHooks) this.element.addEventListener("dblclick", () => taskHooks.edit(this.name));
    this.element.addEventListener("dragstart", (e) => {
      // A single agent may stand in for a task heading. Dragging that heading sorts the task.
      if (taskHooks) {
        e.preventDefault();
        return;
      }
      draggingRow = { task: taskId, agent: agentName };
      e.dataTransfer?.setData(AGENT_MIME, agentName);
      // Without a text type some WebKit versions refuse to start the drag.
      e.dataTransfer?.setData("text/plain", agentName);
      if (e.dataTransfer) e.dataTransfer.effectAllowed = "copyMove";
    });
    this.element.addEventListener("dragend", () => { draggingRow = null; });
    this.element.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu(e.clientX, e.clientY, taskHooks ? taskHooks.menu() : this.menuItems());
    });
  }

  private agent(): AgentView | undefined {
    return getAgents().find((a) => a.name === this.agentName);
  }

  private open(): void {
    if (this.taskHooks) this.actions.selectTask(this.taskId);
    else this.actions.focusAgent(this.taskId, this.agentName);
  }

  private menuItems(): MenuItem[] {
    const name = this.agentName;
    const items: MenuItem[] = [
      { label: "打开", run: () => this.open() },
      { label: "在右边打开", run: () => this.actions.openRight(name) },
      { label: "在下面打开", run: () => this.actions.openBelow(name) },
      { label: "在 Finder 中显示", run: () => this.actions.revealAgent(name) },
    ];
    if (this.agent()?.exited) items.push({ label: "恢复会话", run: () => this.actions.resume(name) });
    const who = KIND_NAMES[this.agent()?.kind ?? ""] ?? "它";
    items.push({ label: `结束 ${who}`, hint: ENDS, confirmLabel: "确认结束", confirmHint: ENDS, danger: true, run: () => this.actions.killAgent(name) });
    return items;
  }

  /** `highlight`: the active panel (agent row) or the current task (task line). `label`: the name to show. */
  update(agent: AgentView, highlight: boolean, now: number, label: string): void {
    const status = displayKey(agent, now);
    this.dot.className = `dot s-${status}`;
    paintMark(this.mark, agent.kind);
    this.element.classList.toggle("active", highlight);
    this.name.textContent = label;
    // The session name is only a hover away.
    const hint = statusHint(status);
    this.element.title = hint ? `${agent.name}\n${statusLabel(status)}：${hint}` : agent.name;
    this.where.textContent = agent.branch ? `${agent.dir_name}  ${agent.branch}` : agent.dir_name;
    this.where.title = agent.cwd;
    this.state.textContent = statusLine(agent, now);
    this.stateRight.textContent = statusLine(agent, now);
    const text = agent.last_message?.replace(/\s+/g, " ").trim() ?? "";
    this.message.textContent = text;
    this.message.hidden = status === "asked" || !text || (!!this.taskHooks && !highlight);
  }
}

/** "在等你": one agent that needs the user, in whatever task it is. */
class WaitingRow {
  readonly element = el("div", "wait-row");
  private readonly dot = el("span", "dot");
  private readonly task = el("span", "wait-task");
  private readonly kind = el("span", "row-mark-box");
  private readonly age = el("span", "wait-age");
  private readonly what = el("div", "wait-what");

  constructor(actions: SidebarActions, private waiting: Waiting) {
    const head = el("div", "wait-head");
    head.append(this.dot, this.task, this.kind, this.age);
    this.element.append(head, this.what);
    this.element.addEventListener("click", () => actions.focusAgent(this.waiting.task.id, this.waiting.agent.name));
  }

  update(waiting: Waiting, now: number): void {
    this.waiting = waiting;
    this.dot.className = `dot s-${viewKey(waiting.agent)}`;
    this.task.textContent = agentLabel(waiting.agent);
    paintMark(this.kind, waiting.agent.kind);
    this.age.textContent = ageText(now - waiting.agent.status_since);
    const what = waitingFor(waiting.agent);
    this.what.textContent = what;
    this.what.hidden = !what;
    this.element.title = [`${statusLabel(viewKey(waiting.agent))}  ${waiting.agent.name}`, what].filter(Boolean).join("\n");
  }
}

interface Ctx {
  now: number;
  currentId: string | null;
  activeAgent: string | null;
}

/** The agent being dragged: a dock tab's name; "" for a sidebar row (its name can only be read on drop); null when
 *  what is dragged is not an agent (a file tab, text, a file from Finder). */
function draggedAgent(e: DragEvent): string | null {
  const panel = getPanelData()?.panelId;
  if (panel) return getAgents().some((a) => a.name === panel) ? panel : null;
  return e.dataTransfer?.types.includes(AGENT_MIME) ? "" : null;
}

/** A task with one agent uses its agent's row. A task with several agents can disclose them underneath. */
class TaskView {
  readonly element = el("div", "task");
  private readonly head = el("div", "task-row");
  private readonly dot = el("span", "dot");
  private readonly nameBox = el("span", "task-name");
  private readonly soloBox = el("div", "task-solo");
  private readonly agentBox = el("div", "task-agents");
  private readonly progressBox = el("details", "task-progress");
  private readonly progressLabel = el("summary", "task-progress-label");
  private readonly progressDetail = el("div", "task-progress-detail");
  private readonly grip = el("button", "task-reorder-handle");
  private readonly disclosure = el("button", "task-disclosure");
  private readonly rows = new Map<string, Row>();
  private readonly solo = new Map<string, Row>();
  private task: Task;
  private editing = false;
  private expanded: boolean | undefined;
  private current = false;

  constructor(private readonly actions: SidebarActions, task: Task, render: () => void) {
    this.task = task;
    this.element.dataset.taskId = task.id;
    this.grip.type = "button";
    this.grip.title = "拖动调整任务顺序";
    this.grip.innerHTML = '<svg width="10" height="14" viewBox="0 0 10 14" fill="currentColor" aria-hidden="true"><circle cx="3" cy="3" r="1"/><circle cx="7" cy="3" r="1"/><circle cx="3" cy="7" r="1"/><circle cx="7" cy="7" r="1"/><circle cx="3" cy="11" r="1"/><circle cx="7" cy="11" r="1"/></svg>';
    this.agentBox.id = `task-agents-${task.id}`;
    this.disclosure.type = "button";
    this.disclosure.innerHTML = chevron;
    this.disclosure.setAttribute("aria-controls", this.agentBox.id);
    this.disclosure.addEventListener("click", (e) => {
      e.stopPropagation();
      this.expanded = this.disclosure.getAttribute("aria-expanded") !== "true";
      render();
    });
    this.disclosure.addEventListener("dblclick", (e) => e.stopPropagation());
    this.head.append(this.dot, this.nameBox, this.disclosure);
    this.progressBox.append(this.progressLabel, this.progressDetail);
    this.element.append(this.head, this.soloBox, this.grip, this.progressBox, this.agentBox);
    // Clicking the row of the open task folds or unfolds its conversations; clicking another task opens it unfolded.
    this.head.addEventListener("click", () => {
      if (this.current) {
        this.expanded = this.disclosure.getAttribute("aria-expanded") !== "true";
        render();
        return;
      }
      this.expanded = true;
      actions.selectTask(this.task.id);
    });
    this.head.addEventListener("dblclick", () => this.startEdit(this.nameBox));
    this.head.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu(e.clientX, e.clientY, this.menuItems());
    });
    // Drag a conversation up or down within its own task to set their order.
    this.agentBox.addEventListener("dragover", (e) => {
      if (draggingRow?.task !== this.task.id) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
      this.showAgentDrop(this.agentDropIndex(e.clientY));
    });
    this.agentBox.addEventListener("dragleave", (e) => {
      if (!this.agentBox.contains(e.relatedTarget as Node | null)) this.showAgentDrop(null);
    });
    this.agentBox.addEventListener("drop", (e) => {
      const dragged = draggingRow;
      this.showAgentDrop(null);
      if (dragged?.task !== this.task.id) return;
      e.preventDefault();
      e.stopPropagation();
      const names = this.agentRows().map((row) => row.dataset.agent!);
      const from = names.indexOf(dragged.agent);
      const boundary = this.agentDropIndex(e.clientY);
      const to = boundary > from ? boundary - 1 : boundary;
      if (from < 0 || from === to) return;
      names.splice(to, 0, names.splice(from, 1)[0]);
      actions.reorderAgents(this.task.id, names);
    });
    // Drop an agent's tab, or its row, on a task to move it into that task (用户 11-09: for one opened in the
    // wrong task). The whole task, its agent lines included, is the target.
    this.element.addEventListener("dragover", (e) => {
      const name = draggedAgent(e);
      if (name === null || this.task.agents.includes(name)) return;
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
      this.element.classList.add("drop-target");
    });
    this.element.addEventListener("dragleave", (e) => {
      if (!this.element.contains(e.relatedTarget as Node | null)) this.element.classList.remove("drop-target");
    });
    this.element.addEventListener("drop", (e) => {
      this.element.classList.remove("drop-target");
      const name = e.dataTransfer?.getData(AGENT_MIME) || getPanelData()?.panelId;
      if (!name || this.task.agents.includes(name) || !getAgents().some((a) => a.name === name)) return;
      e.preventDefault();
      e.stopPropagation();
      actions.moveAgent(name, this.task.id);
    });
  }

  private agentRows(): HTMLElement[] {
    return Array.from(this.agentBox.children) as HTMLElement[];
  }

  /** Where a drop at this height lands, as an index between rows. */
  private agentDropIndex(y: number): number {
    const rows = this.agentRows();
    const before = rows.findIndex((row) => { const r = row.getBoundingClientRect(); return y < r.top + r.height / 2; });
    return before < 0 ? rows.length : before;
  }

  private showAgentDrop(index: number | null): void {
    const rows = this.agentRows();
    rows.forEach((row) => row.removeAttribute("data-sidebar-insert"));
    if (index === null || !rows.length) return;
    const row = rows[Math.min(index, rows.length - 1)];
    row.dataset.sidebarInsert = index >= rows.length ? "after" : "before";
  }

  private menuItems(): MenuItem[] {
    const count = taskAgents(this.task, getAgents()).length;
    const id = this.task.id;
    return [
      { label: "重命名", run: () => this.startEdit(this.currentNameElement()) },
      { label: "在 Finder 中显示", run: () => this.actions.revealProject(this.task.project) },
      {
        label: "删除任务",
        confirmLabel: count > 0 ? `结束这个任务的 ${count} 个 agent 并删掉任务` : "删掉任务",
        danger: true,
        run: () => this.actions.deleteTask(id),
      },
    ];
  }

  /** The element that shows the name now: the task line, or the agent line standing in for it. */
  private currentNameElement(): HTMLElement {
    return this.head.hidden ? ([...this.solo.values()][0]?.name ?? this.nameBox) : this.nameBox;
  }

  /** Rename in place. Enter confirms, Esc cancels. The input sits in a form so WebKit decides what Enter means:
   * the Enter that picks a Pinyin candidate does not submit, a plain Enter does. */
  private startEdit(target: HTMLElement): void {
    if (this.editing) return;
    this.editing = true;
    const shown = taskLabel(this.task, getAgents());
    const form = el("form", "inline-form");
    const input = el("input", "task-edit");
    input.value = shown;
    form.append(input);
    target.replaceWith(form);
    input.focus();
    input.select();
    let done = false;
    const finish = (save: boolean) => {
      if (done) return;
      done = true;
      this.editing = false;
      const name = input.value.trim();
      form.replaceWith(target);
      if (save && name && name !== shown) this.actions.renameTask(this.task.id, name);
    };
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      finish(true);
    });
    input.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && !e.isComposing) finish(false);
    });
    input.addEventListener("blur", () => finish(true));
    input.addEventListener("click", (e) => e.stopPropagation());
  }

  update(task: Task, ctx: Ctx): void {
    this.task = task;
    const agents = getAgents();
    const current = task.id === ctx.currentId;
    this.current = current;
    const mine = taskAgents(task, agents);
    const label = taskLabel(task, agents);
    const progress = progressSummary(mine);
    this.progressBox.hidden = progress === null;
    const text = progress?.text ?? "";
    const detail = progress?.detail ?? "";
    if (this.progressLabel.textContent !== text) this.progressLabel.textContent = text;
    if (this.progressDetail.textContent !== detail) {
      this.progressLabel.title = detail;
      this.progressDetail.textContent = detail;
    }
    if (!progress) this.progressBox.open = false;
    this.progressBox.classList.toggle("attention", progress?.attention === true);
    this.element.classList.toggle("unread", taskNeedsYou(task, agents));
    this.grip.setAttribute("aria-label", `调整「${label}」的顺序，使用上下方向键`);

    // One agent: the task line is the agent's line.
    const single = mine.length === 1;
    this.head.hidden = single;
    const hooks: TaskHooks = {
      menu: () => this.menuItems(),
      edit: (name) => this.startEdit(name),
    };
    const soloRows = cached(this.solo, single ? [mine[0].name] : [], (name) => new Row(name, this.actions, task.id, hooks));
    if (single) soloRows[0].update(mine[0], current, ctx.now, label);
    syncChildren(this.soloBox, soloRows.map((r) => r.element));

    const status = taskStatus(task, agents, ctx.now);
    this.dot.className = `dot s-${status ?? "none"}`;
    this.head.classList.toggle("current", current);
    this.nameBox.textContent = label;
    const hint = status === "unready" || status === "asked" ? `${statusLabel(status)}：${statusHint(status)}` : "";
    this.head.title = hint ? `${task.project}\n${hint}` : task.project;

    const expandable = mine.length > 1;
    const expanded = expandable && (this.expanded ?? current);
    this.disclosure.hidden = !expandable;
    this.disclosure.setAttribute("aria-expanded", String(expanded));
    const disclosureLabel = `${expanded ? "收起" : "展开"}「${label}」的会话`;
    this.disclosure.title = disclosureLabel;
    this.disclosure.setAttribute("aria-label", disclosureLabel);
    this.agentBox.hidden = !expanded;
    const open = expanded ? inDraggedOrder(mine, getAgentOrder()[task.id]) : [];
    const rows = cached(this.rows, open.map((a) => a.name), (name) => new Row(name, this.actions, task.id));
    open.forEach((agent, i) => rows[i].update(agent, agent.name === ctx.activeAgent, ctx.now, agentLabel(agent)));
    syncChildren(this.agentBox, rows.map((r) => r.element));
  }
}

/** A folder with tasks. Collapsing is kept only in memory. */
class ProjectView {
  readonly element = el("div", "project");
  private readonly head = el("div", "project-row");
  private readonly arrow = el("span", "project-arrow");
  private readonly name = el("span", "project-name");
  private readonly list = el("div", "project-tasks");
  private readonly tasks = new Map<string, TaskView>();

  constructor(
    private readonly actions: SidebarActions,
    private readonly path: string,
    toggle: () => void,
    private readonly render: () => void,
  ) {
    this.element.dataset.projectPath = path;
    this.list.dataset.projectPath = path;
    this.arrow.innerHTML = chevron;
    const add = el("button", "icon-btn project-add");
    add.innerHTML = plus;
    add.title = "在这个项目里新建任务";
    add.setAttribute("aria-label", "在这个项目里新建任务");
    add.addEventListener("click", (e) => {
      e.stopPropagation();
      actions.requestNewTask(add, path);
    });
    this.head.append(this.arrow, this.name, add);
    this.head.addEventListener("click", toggle);
    this.element.append(this.head, this.list);
  }

  update(project: Project, collapsed: boolean, ctx: Ctx): void {
    this.name.textContent = project.name;
    this.head.title = this.path;
    this.element.classList.toggle("collapsed", collapsed);
    const views = cached(this.tasks, project.tasks.map((t) => t.id), (id) =>
      new TaskView(this.actions, project.tasks.find((t) => t.id === id)!, this.render),
    );
    project.tasks.forEach((t, i) => views[i].update(t, ctx));
    const focused = this.list.querySelector<HTMLButtonElement>(".task-reorder-handle:focus");
    syncChildren(this.list, views.map((v) => v.element));
    if (focused?.isConnected && document.activeElement !== focused) focused.focus({ preventScroll: true });
  }
}

export class Sidebar {
  readonly element = el("aside", "sidebar");
  private readonly waitingLabel = el("div", "side-label", "在等你");
  private readonly waitingBox = el("div", "waiting");
  private readonly projectBox = el("div", "projects");
  private readonly errorBox = el("div", "sidebar-error");
  private readonly noticeBox = el("div", "sidebar-error");
  private readonly waitingRows = new Map<string, WaitingRow>();
  private readonly projectViews = new Map<string, ProjectView>();
  private readonly collapsed = new Set<string>();
  private activeName: string | null = null;
  private errorTimer: number | undefined;
  readonly newButton = el("button", "icon-btn new-task");
  private readonly progressRow = el("div", "row nav-row", "进度");
  private readonly pinned: Record<Role, PinnedRow>;
  private readonly searchInput = el("input", "side-search-input");
  private readonly results = el("div", "search-results");
  private query = "";
  private hits: Hit[] = [];
  private searchSeq = 0;
  private searchTimer: number | undefined;

  constructor(private readonly actions: SidebarActions) {
    const header = el("div", "sidebar-head");
    header.dataset.tauriDragRegion = "";
    const brand = el("span", "brand", "Shifu");
    brand.dataset.tauriDragRegion = "";
    // The acceptance copy calls itself "Shifu 测试", so it is never taken for the real one.
    appName().then((n) => (brand.textContent = n)).catch(reportError);
    header.append(brand, this.newButton);
    this.newButton.title = "新建任务  ⌘N";
    this.newButton.setAttribute("aria-label", "新建任务");
    this.newButton.innerHTML = '<svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" aria-hidden="true"><path d="M8 3v10M3 8h10"/></svg>';
    this.newButton.addEventListener("click", () => actions.requestNewTask(this.newButton));
    const scroll = el("div", "side-scroll");
    this.waitingBox.append(this.waitingLabel);
    this.progressRow.addEventListener("click", () => actions.showProgress());
    this.pinned = {
      pm: new PinnedRow(actions, "pm", "项目经理", "常驻的助理：问它哪些在等你、某件事做到哪了，也能替你给某个对话传话"),
      daily: new PinnedRow(actions, "daily", "每日分析", "汇总各项目推进与阻碍；安全、云机和用量报告需自行配置来源"),
    };
    // Hidden until its folder is known (an older backend without the role leaves it hidden).
    this.pinned.daily.element.hidden = true;
    scroll.append(this.results, this.pinned.pm.element, this.pinned.daily.element, this.progressRow, this.waitingBox, this.projectBox);
    this.errorBox.hidden = true;
    this.noticeBox.hidden = true;
    this.noticeBox.setAttribute("role", "alert");
    this.element.append(header, this.searchBar(), scroll, this.noticeBox, this.errorBox);
    bindSidebarReorder(this.projectBox, scroll, actions.reorderProject, actions.reorderTask);

    subscribe(() => this.render());
    this.render();
    onError((m) => this.showError(m));
    // Wait times tick up without any event from the backend.
    window.setInterval(() => this.render(), REFRESH_AGE_MS);
  }

  /** Name, task, folder and what was said in each open conversation. ⌘K comes here. */
  private searchBar(): HTMLElement {
    const bar = el("div", "side-search");
    bar.innerHTML =
      '<svg width="13" height="13" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="6" cy="6" r="4.2"/><path d="M9.2 9.2l3 3"/></svg>';
    const input = this.searchInput;
    input.type = "search";
    input.placeholder = "搜索对话  ⌘K";
    input.spellcheck = false;
    input.setAttribute("aria-label", "搜索对话");
    input.addEventListener("input", () => this.setQuery(input.value));
    input.addEventListener("keydown", (e) => {
      if (e.isComposing) return;
      if (e.key === "Escape") {
        input.value = "";
        this.setQuery("");
        input.blur();
      } else if (e.key === "Enter") {
        (this.results.querySelector(".search-row") as HTMLElement | null)?.click();
      }
    });
    bar.append(input);
    return bar;
  }

  focusSearch(): void {
    this.searchInput.focus();
    this.searchInput.select();
  }

  private setQuery(query: string): void {
    this.query = query;
    this.hits = [];
    this.render();
    window.clearTimeout(this.searchTimer);
    if (!query.trim()) return;
    const seq = ++this.searchSeq;
    this.searchTimer = window.setTimeout(() => {
      searchConversations(query)
        .then((hits) => {
          if (seq !== this.searchSeq) return;
          this.hits = hits;
          this.render();
        })
        .catch(reportError);
    }, SEARCH_DEBOUNCE_MS);
  }

  private renderResults(found: Found[]): void {
    const rows = found.map((f) => {
      const row = el("div", "search-row");
      const head = el("div", "search-head");
      const name = el("span", "search-name");
      name.append(...highlight(agentLabel(f.agent), this.query));
      const mark = el("span", "row-mark-box");
      paintMark(mark, f.agent.kind);
      head.append(mark, name);
      // A one-agent task is usually named after its agent; the same words twice say nothing.
      const taskName = taskLabel(f.task, getAgents());
      if (taskName !== agentLabel(f.agent)) {
        const task = el("span", "search-task");
        task.append(...highlight(taskName, this.query));
        head.append(task);
      }
      row.append(head);
      if (f.snippet) {
        const line = el("div", "search-snippet");
        line.append(...highlight(f.snippet, this.query));
        if (f.count > 1) line.append(el("span", "search-count", ` ${f.count} 处`));
        row.append(line);
      }
      row.title = f.agent.name;
      row.addEventListener("click", () => this.actions.focusAgent(f.task.id, f.agent.name));
      return row;
    });
    if (rows.length === 0) rows.push(el("div", "search-empty", "开着的对话里没找到"));
    this.results.replaceChildren(...rows);
  }

  setRoleDir(role: Role, dir: string): void {
    this.pinned[role].dir = dir;
    this.render();
  }

  private renderPinned(currentId: string | null): void {
    const pm = getTasks().find((t) => t.project === this.pinned.pm.dir);
    const own = new Set(pm?.agents ?? []);
    const waiting = waitingList(getTasks(), getAgents()).filter((w) => !own.has(w.agent.name)).length;
    this.pinned.pm.update(currentId, waiting);
    this.pinned.daily.update(currentId);
  }

  setProgressActive(on: boolean): void {
    this.progressRow.classList.toggle("active", on);
  }

  setActive(name: string | null): void {
    this.activeName = name;
    this.render();
  }

  private render(): void {
    const searching = this.query.trim() !== "";
    this.results.hidden = !searching;
    this.pinned.pm.element.hidden = searching;
    this.pinned.daily.element.hidden = searching || this.pinned.daily.dir === null;
    this.progressRow.hidden = searching;
    this.projectBox.hidden = searching;
    if (searching) {
      this.waitingBox.hidden = true;
      this.renderResults(findAgents(this.query, getTasks(), getAgents(), this.hits));
      return;
    }
    const ctx: Ctx = { now: nowSecs(), currentId: getCurrentId(), activeAgent: this.activeName };
    this.renderPinned(ctx.currentId);
    const waiting = waitingList(getTasks(), getAgents());
    const rows = cached(this.waitingRows, waiting.map((w) => w.agent.name), (name) => new WaitingRow(this.actions, waiting.find((w) => w.agent.name === name)!));
    waiting.forEach((w, i) => rows[i].update(w, ctx.now));
    this.waitingBox.hidden = waiting.length === 0;
    // The label stays first; the rows follow it.
    syncChildren(this.waitingBox, [this.waitingLabel, ...rows.map((r) => r.element)]);

    const projects = this.shownProjects();
    const views = cached(this.projectViews, projects.map((p) => p.path), (path) =>
      new ProjectView(this.actions, path, () => this.toggle(path), () => this.render()),
    );
    projects.forEach((p, i) => views[i].update(p, this.collapsed.has(p.path), ctx));
    syncChildren(this.projectBox, views.map((v) => v.element));
  }

  /** Project folders with their tasks in the order the sidebar shows them; the pinned role folders left out. */
  shownProjects(): Project[] {
    const pinnedDirs = new Set([this.pinned.pm.dir, this.pinned.daily.dir]);
    return projectsOf(getTasks().filter((t) => !pinnedDirs.has(t.project)), getProjectOrder(), getTaskOrder());
  }

  private toggle(path: string): void {
    if (!this.collapsed.delete(path)) this.collapsed.add(path);
    this.render();
  }

  /** A problem that lasts until the user fixes it outside the app; unlike an error it does not fade. */
  setNotice(message: string): void {
    this.noticeBox.textContent = message;
    this.noticeBox.hidden = false;
  }

  private showError(message: string): void {
    this.errorBox.textContent = message;
    this.errorBox.hidden = false;
    window.clearTimeout(this.errorTimer);
    this.errorTimer = window.setTimeout(() => (this.errorBox.hidden = true), ERROR_SHOW_MS);
  }
}
