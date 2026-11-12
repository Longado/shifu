import { getPanelData } from "dockview";
import { AGENT_MIME, appName, searchConversations } from "./api";
import { findAgents, type Found, highlight, type Hit } from "./search";
import { type AgentView, agentLabel, ageText, statusLabel, statusLine, viewKey } from "./format";
import { type Mark, vendorIcon } from "./vendoricons";
import { type MenuItem, showMenu } from "./menu";
import { getAgents, getCurrentId, getTasks, onError, reportError, subscribe } from "./store";
import { ENDS } from "./tab";
import {
  type Project,
  projectsOf,
  type Task,
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
  openAgent(name: string): void;
  openRight(name: string): void;
  openBelow(name: string): void;
  revealAgent(name: string): void;
  resume(name: string): void;
  killAgent(name: string): void;
  /** An agent's tab or row was dropped on another task: it now belongs there. */
  moveAgent(name: string, taskId: string): void;
  renameTask(id: string, name: string): void;
  revealProject(path: string): void;
  deleteTask(id: string): void;
  /** `project` preselects a folder (the "+" on a project row). */
  requestNewTask(anchor: HTMLElement, project?: string): void;
  /** The 进度 row: every project's GOALS.md checklist in the main area. */
  showProgress(): void;
  /** The 项目经理 row: its task; started (after asking which agent, the first time) when it has none. */
  openPm(anchor: HTMLElement): void;
  /** Forget the project manager's agent and model; the next start asks again. */
  repickPm(): void;
  openWaimai(): void;
}

const ERROR_SHOW_MS = 8000;
/** Wait this long after the last key before reading transcripts. */
const SEARCH_DEBOUNCE_MS = 200;
const REFRESH_AGE_MS = 15000;
const KIND_NAMES: Record<string, string> = { claude: "Claude", codex: "Codex", codebuddy: "CodeBuddy", kimi: "Kimi", antigravity: "Antigravity" };

const MARKED = new Set(["claude", "codex", "codebuddy", "kimi", "antigravity"]);

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
  click(): void;
  menu(): MenuItem[];
  /** Rename in place; `name` is the element showing the name. */
  edit(name: HTMLElement): void;
}

/** One agent row. Built once and updated in place so a drag in progress is not cancelled by a refresh.
 *  With `taskHooks` it is a task's whole line: one line (name and state), plus the last reply when `current`. */
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

  constructor(readonly agentName: string, private readonly actions: SidebarActions, private readonly taskHooks?: TaskHooks) {
    this.element.draggable = true;
    const head = el("div", "row-head");
    head.append(this.dot, this.mark, this.name, this.stateRight);
    this.sub.append(this.where, this.state);
    this.element.append(head, this.sub, this.message);
    this.element.classList.toggle("row-task", !!taskHooks);
    this.stateRight.hidden = !taskHooks;
    this.sub.hidden = !!taskHooks;

    this.element.addEventListener("click", () => (taskHooks ? taskHooks.click() : actions.openAgent(agentName)));
    if (taskHooks) this.element.addEventListener("dblclick", () => taskHooks.edit(this.name));
    this.element.addEventListener("dragstart", (e) => {
      e.dataTransfer?.setData(AGENT_MIME, agentName);
      // Without a text type some WebKit versions refuse to start the drag.
      e.dataTransfer?.setData("text/plain", agentName);
      if (e.dataTransfer) e.dataTransfer.effectAllowed = "copyMove";
    });
    this.element.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu(e.clientX, e.clientY, taskHooks ? taskHooks.menu() : this.menuItems());
    });
  }

  private agent(): AgentView | undefined {
    return getAgents().find((a) => a.name === this.agentName);
  }

  private menuItems(): MenuItem[] {
    const name = this.agentName;
    const items: MenuItem[] = [
      { label: "打开", run: () => this.actions.openAgent(name) },
      { label: "在右边打开", run: () => this.actions.openRight(name) },
      { label: "在下面打开", run: () => this.actions.openBelow(name) },
      { label: "在 Finder 中显示", run: () => this.actions.revealAgent(name) },
    ];
    if (this.agent()?.exited) items.push({ label: "恢复会话", run: () => this.actions.resume(name) });
    const who = KIND_NAMES[this.agent()?.kind ?? ""] ?? "它";
    items.push({ label: `结束 ${who}`, hint: ENDS, confirmLabel: "确认结束", confirmHint: "再点一次就结束", danger: true, run: () => this.actions.killAgent(name) });
    return items;
  }

  /** `highlight`: the active panel (agent row) or the current task (task line). `label`: the name to show. */
  update(agent: AgentView, highlight: boolean, now: number, label: string): void {
    this.dot.className = `dot s-${viewKey(agent)}`;
    paintMark(this.mark, agent.kind);
    this.element.classList.toggle("active", highlight);
    this.name.textContent = label;
    // The session name is only a hover away.
    this.element.title = agent.name;
    this.where.textContent = agent.branch ? `${agent.dir_name}  ${agent.branch}` : agent.dir_name;
    this.where.title = agent.cwd;
    this.state.textContent = statusLine(agent, now);
    this.stateRight.textContent = statusLine(agent, now);
    const text = agent.last_message?.replace(/\s+/g, " ").trim() ?? "";
    this.message.textContent = text;
    this.message.hidden = !text || (!!this.taskHooks && !highlight);
  }
}

/** "在等你": one agent that needs the user, in whatever task it is. */
class WaitingRow {
  readonly element = el("div", "wait-row");
  private readonly dot = el("span", "dot");
  private readonly task = el("span", "wait-task");
  private readonly kind = el("span", "row-mark-box");
  private readonly age = el("span", "wait-age");

  constructor(actions: SidebarActions, private waiting: Waiting) {
    this.element.append(this.dot, this.task, this.kind, this.age);
    this.element.addEventListener("click", () => actions.focusAgent(this.waiting.task.id, this.waiting.agent.name));
  }

  update(waiting: Waiting, now: number): void {
    this.waiting = waiting;
    this.dot.className = `dot s-${viewKey(waiting.agent)}`;
    this.task.textContent = agentLabel(waiting.agent);
    paintMark(this.kind, waiting.agent.kind);
    this.age.textContent = ageText(now - waiting.agent.status_since);
    this.element.title = `${statusLabel(viewKey(waiting.agent))}  ${waiting.agent.name}`;
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

/** A task: one line. With one agent the line is that agent's (dot, state, last reply); with more, the current
 *  task opens its agents underneath. */
class TaskView {
  readonly element = el("div", "task");
  private readonly head = el("div", "task-row");
  private readonly dot = el("span", "dot");
  private readonly nameBox = el("span", "task-name");
  private readonly soloBox = el("div", "task-solo");
  private readonly agentBox = el("div", "task-agents");
  private readonly rows = new Map<string, Row>();
  private readonly solo = new Map<string, Row>();
  private task: Task;
  private editing = false;

  constructor(private readonly actions: SidebarActions, task: Task) {
    this.task = task;
    this.head.append(this.dot, this.nameBox);
    this.element.append(this.head, this.soloBox, this.agentBox);
    this.head.addEventListener("click", () => actions.selectTask(this.task.id));
    this.head.addEventListener("dblclick", () => this.startEdit(this.nameBox));
    this.head.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu(e.clientX, e.clientY, this.menuItems());
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
    const mine = taskAgents(task, agents);
    const label = taskLabel(task, agents);
    this.element.classList.toggle("unread", taskNeedsYou(task, agents));

    // One agent: the task line is the agent's line.
    const single = mine.length === 1;
    this.head.hidden = single;
    const hooks: TaskHooks = {
      click: () => this.actions.selectTask(task.id),
      menu: () => this.menuItems(),
      edit: (name) => this.startEdit(name),
    };
    const soloRows = cached(this.solo, single ? [mine[0].name] : [], (name) => new Row(name, this.actions, hooks));
    if (single) soloRows[0].update(mine[0], current, ctx.now, label);
    syncChildren(this.soloBox, soloRows.map((r) => r.element));

    this.dot.className = `dot s-${taskStatus(task, agents) ?? "none"}`;
    this.head.classList.toggle("current", current);
    this.nameBox.textContent = label;
    this.head.title = task.project;

    const open = current && !single ? mine : [];
    const rows = cached(this.rows, open.map((a) => a.name), (name) => new Row(name, this.actions));
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

  constructor(private readonly actions: SidebarActions, private readonly path: string, toggle: () => void) {
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
    const views = cached(this.tasks, project.tasks.map((t) => t.id), (id) => new TaskView(this.actions, project.tasks.find((t) => t.id === id)!));
    project.tasks.forEach((t, i) => views[i].update(t, ctx));
    syncChildren(this.list, views.map((v) => v.element));
  }
}

export class Sidebar {
  readonly element = el("aside", "sidebar");
  private readonly waitingLabel = el("div", "side-label", "在等你");
  private readonly waitingBox = el("div", "waiting");
  private readonly projectBox = el("div", "projects");
  private readonly errorBox = el("div", "sidebar-error");
  private readonly waitingRows = new Map<string, WaitingRow>();
  private readonly projectViews = new Map<string, ProjectView>();
  private readonly collapsed = new Set<string>();
  private activeName: string | null = null;
  private errorTimer: number | undefined;
  readonly newButton = el("button", "icon-btn new-task");
  private readonly progressRow = el("div", "row nav-row", "进度");
  private readonly pmRow = el("button", "row nav-row pm-row");
  private readonly pmDot = el("span", "dot");
  private readonly pmNote = el("span", "pm-note");
  /** The project manager's folder; its task is pinned at the top instead of listed under a project. */
  private pmDir: string | null = null;
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
    const pmIcon = el("span", "pm-icon");
    pmIcon.innerHTML = '<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2.5" y="5.5" width="13" height="10" rx="2"/><path d="M6.5 5.5v-2h5v2M2.5 9.5a16 16 0 0 0 13 0M9 9v3"/></svg>';
    pmIcon.append(this.pmDot);
    this.pmRow.append(pmIcon, el("span", "pm-name", "项目经理"), this.pmNote);
    this.pmRow.setAttribute("aria-label", "项目经理");
    this.pmRow.title = "常驻的助理：问它哪些在等你、某件事做到哪了，也能替你给某个对话传话";
    this.pmRow.addEventListener("click", () => actions.openPm(this.pmRow));
    this.pmRow.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu(e.clientX, e.clientY, [
        { label: "换 agent 或模型", hint: "下次开项目经理时重新选", run: () => actions.repickPm() },
      ]);
    });
    // Ordering opens its own window from the sidebar toolbar.
    const waimai = el("button", "icon-btn waimai-entry");
    const waimaiIcon = el("span", "waimai-icon");
    waimaiIcon.innerHTML = '<svg width="24" height="24" viewBox="0 0 16 16" fill="none" shape-rendering="crispEdges" aria-hidden="true"><path fill="#36483e" d="M6 2h4v1h2v1h1v2h2v3h-1v2h-2v2h-2v1H6v-1H4v-2H2V9H1V6h2V4h1V3h2z"/><path fill="#fff4cf" d="M6 3h4v1h2v1h1v2H3V5h1V4h2z"/><path fill="#d6c99c" d="M5 5h1v1H5zM8 4h1v1H8zM10 6h2v1h-2z"/><path fill="#e7a652" d="M2 7h12v2h-1v2h-2v1H5v-1H3V9H2z"/><path fill="#bd713d" d="M2 8h12v1H2zM10 10h3v1h-2v1H8v-1h2z"/><path fill="#f9d68b" d="M3 9h2v1H3zM5 10h2v1H5z"/><path fill="#d6c99c" d="M6 13h4v1H6z"/></svg>';
    waimaiIcon.setAttribute("aria-hidden", "true");
    waimai.append(waimaiIcon);
    waimai.setAttribute("aria-label", "点餐");
    waimai.title = "点餐";
    waimai.addEventListener("click", () => actions.openWaimai());
    this.newButton.before(waimai);
    scroll.append(this.results, this.pmRow, this.progressRow, this.waitingBox, this.projectBox);
    this.errorBox.hidden = true;
    this.element.append(header, this.searchBar(), scroll, this.errorBox);
    // Cancelling with Escape ends the drag without a drop or a final dragleave on the target.
    document.addEventListener("dragend", () => {
      this.projectBox.querySelectorAll(".drop-target").forEach((target) => target.classList.remove("drop-target"));
    });

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

  setPmDir(dir: string): void {
    this.pmDir = dir;
    this.render();
  }

  private renderPm(currentId: string | null): void {
    const pm = getTasks().find((t) => t.project === this.pmDir);
    const own = new Set(pm?.agents ?? []);
    const waiting = waitingList(getTasks(), getAgents()).filter((w) => !own.has(w.agent.name)).length;
    this.pmNote.textContent = waiting ? String(waiting) : "";
    this.pmNote.hidden = waiting === 0;
    this.pmNote.title = `${waiting} 件事等你`;
    this.pmRow.setAttribute("aria-label", waiting ? `项目经理，${waiting} 件事等你` : "项目经理");
    const status = pm ? taskStatus(pm, getAgents()) : null;
    this.pmDot.className = `dot ${status ? `s-${status}` : "pm-idle"}`;
    this.pmRow.classList.toggle("active", !!pm && pm.id === currentId);
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
    this.pmRow.hidden = searching;
    this.progressRow.hidden = searching;
    this.projectBox.hidden = searching;
    if (searching) {
      this.waitingBox.hidden = true;
      this.renderResults(findAgents(this.query, getTasks(), getAgents(), this.hits));
      return;
    }
    const ctx: Ctx = { now: nowSecs(), currentId: getCurrentId(), activeAgent: this.activeName };
    this.renderPm(ctx.currentId);
    const waiting = waitingList(getTasks(), getAgents());
    const rows = cached(this.waitingRows, waiting.map((w) => w.agent.name), (name) => new WaitingRow(this.actions, waiting.find((w) => w.agent.name === name)!));
    waiting.forEach((w, i) => rows[i].update(w, ctx.now));
    this.waitingBox.hidden = waiting.length === 0;
    // The label stays first; the rows follow it.
    syncChildren(this.waitingBox, [this.waitingLabel, ...rows.map((r) => r.element)]);

    const projects = projectsOf(getTasks().filter((t) => t.project !== this.pmDir));
    const views = cached(this.projectViews, projects.map((p) => p.path), (path) => new ProjectView(this.actions, path, () => this.toggle(path)));
    projects.forEach((p, i) => views[i].update(p, this.collapsed.has(p.path), ctx));
    syncChildren(this.projectBox, views.map((v) => v.element));
  }

  private toggle(path: string): void {
    if (!this.collapsed.delete(path)) this.collapsed.add(path);
    this.render();
  }

  private showError(message: string): void {
    this.errorBox.textContent = message;
    this.errorBox.hidden = false;
    window.clearTimeout(this.errorTimer);
    this.errorTimer = window.setTimeout(() => (this.errorBox.hidden = true), ERROR_SHOW_MS);
  }
}
