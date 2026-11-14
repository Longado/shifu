import { createDockview, themeDark } from "dockview";
import type {
  DockviewApi,
  AddPanelOptions,
  DockviewGroupPanel,
  IContentRenderer,
  IHeaderActionsRenderer,
  ITabRenderer,
  IWatermarkRenderer,
  Position,
} from "dockview";
import { AGENT_MIME, layoutLoad, layoutSave } from "./api";
import { AgentTab, FileTab } from "./tab";
import { AGENTS } from "./models";
import { vendorIcon } from "./vendoricons";
import { TerminalPanel } from "./terminal";
import { FILE_PREFIX, FilePanel, isFilePanel, setFileOpener } from "./preview";
import { findAgent, getCurrentTask, getTasks, reportError, subscribe } from "./store";
import { agentLabel } from "./format";
import { showMenu } from "./menu";
import { type DockPort, TaskDock } from "./taskdock";

type Direction = "left" | "right" | "above" | "below" | "within";

/** Where a panel goes: into `group` as a tab (`within`), or split off one side of it. */
export interface Place {
  group?: DockviewGroupPanel;
  direction?: Direction;
  index?: number;
}

export interface WorkspaceHooks {
  /** The "+" in a group's tab bar: add an agent to the current task, opened in that group. */
  requestAddAgent(anchor: HTMLElement, group: DockviewGroupPanel): void;
  /** Split `group`: its front tab moves to the new side, or, when it is the only tab, a new agent opens there. */
  requestSplit(anchor: HTMLElement, group: DockviewGroupPanel, direction: "below" | "right"): void;
  /** The empty state's buttons. */
  addAgent(kind: string): void;
  requestNewTask(anchor: HTMLElement): void;
  activeChanged(name: string | null): void;
  selectTask(id: string): Promise<void>;
  endAgent(name: string): void;
  tabClosed(message: string): void;
}

interface ClosedPanel {
  taskId: string;
  groupId: string;
  index: number;
  options: Pick<AddPanelOptions, "id" | "component" | "tabComponent" | "title" | "params" | "renderer">;
}

/** A just-opened panel's agent may not be in the list yet; give the list this long to catch up. */
const GRACE_MS = 5000;

const POSITION_TO_DIRECTION: Record<Position, Direction> = {
  center: "within",
  left: "left",
  right: "right",
  top: "above",
  bottom: "below",
};
const DIRECTION_TO_POSITION: Record<Direction, Position> = {
  within: "center",
  left: "left",
  right: "right",
  above: "top",
  below: "bottom",
};

class GroupActions implements IHeaderActionsRenderer {
  readonly element = document.createElement("div");
  constructor(private readonly group: DockviewGroupPanel, private readonly hooks: WorkspaceHooks) {
    this.element.className = "group-actions";
    const add = document.createElement("button");
    add.className = "icon-btn";
    add.title = "在当前任务里加 agent";
    add.setAttribute("aria-label", "在当前任务里加 agent");
    add.innerHTML = plusIcon;
    add.addEventListener("click", () => this.hooks.requestAddAgent(add, this.group));
    const split = document.createElement("button");
    split.className = "icon-btn";
    split.title = "分屏";
    split.setAttribute("aria-label", "分屏");
    split.innerHTML = splitIcon;
    split.addEventListener("click", () => {
      const r = split.getBoundingClientRect();
      showMenu(r.left, r.bottom + 4, [
        { label: "上下分屏", hint: "⇧⌘D", run: () => this.hooks.requestSplit(split, this.group, "below") },
        { label: "左右分屏", hint: "⌘D", run: () => this.hooks.requestSplit(split, this.group, "right") },
      ]);
    });
    this.element.append(split, add);
  }
  init(): void {}
  dispose(): void {}
}

const splitIcon =
  '<svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.3"><rect x="2" y="2.5" width="10" height="9" rx="1.5"/><path d="M2 7h10"/></svg>';
const plusIcon =
  '<svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M7 2.5v9M2.5 7h9"/></svg>';

/** Nothing open: ask for a task if there is none, else for an agent in the current one. */
class EmptyState implements IWatermarkRenderer {
  readonly element = document.createElement("div");
  private readonly unsubscribe: () => void;
  constructor(private readonly hooks: WorkspaceHooks) {
    this.element.className = "empty";
    this.unsubscribe = subscribe(() => this.paint());
    this.paint();
  }
  private paint(): void {
    this.element.replaceChildren();
    if (!getCurrentTask()) {
      this.element.append(this.button("新建任务", (b) => this.hooks.requestNewTask(b)));
      return;
    }
    const label = document.createElement("div");
    label.className = "empty-label";
    label.textContent = "在这个任务里加一个 agent";
    const buttons = document.createElement("div");
    buttons.className = "empty-buttons";
    buttons.append(
      ...AGENTS.map((a) => {
        const b = this.button(a.name, () => this.hooks.addAgent(a.kind));
        b.prepend(vendorIcon(a.kind, "agent-mark"));
        return b;
      }),
    );
    this.element.append(label, buttons);
  }
  private button(text: string, onClick: (b: HTMLButtonElement) => void): HTMLButtonElement {
    const b = document.createElement("button");
    b.className = "btn primary";
    b.textContent = text;
    b.addEventListener("click", () => onClick(b));
    return b;
  }
  init(): void {}
  dispose(): void {
    this.unsubscribe();
  }
}

export class Workspace {
  readonly api: DockviewApi;
  /** Panels opened moments ago whose agent the list may not contain yet. */
  private readonly pending = new Map<string, number>();
  private readonly taskDock: TaskDock;
  private closed: ClosedPanel[] = [];

  constructor(root: HTMLElement, private readonly hooks: WorkspaceHooks) {
    this.api = createDockview(root, {
      theme: themeDark,
      disableFloatingGroups: true,
      defaultTabComponent: "file",
      dropOverlayModel: ({ location }) => location === "content"
        ? { activationSize: { value: 33, type: "percentage" } }
        : undefined,
      createComponent: (o): IContentRenderer => (o.name === "file" ? new FilePanel() : new TerminalPanel()),
      createTabComponent: (o): ITabRenderer => o.name === "agent"
        ? new AgentTab((id) => this.closeTab(id), hooks.endAgent)
        : new FileTab((id) => this.closeTab(id)),
      createRightHeaderActionComponent: (group) => new GroupActions(group, hooks),
      createWatermarkComponent: () => new EmptyState(hooks),
    });
    this.api.onUnhandledDragOver((e) => {
      const ev = e.nativeEvent;
      if (ev instanceof DragEvent && ev.dataTransfer?.types.includes(AGENT_MIME)) e.accept();
    });
    this.api.onDidDrop((e) => {
      const ev = e.nativeEvent;
      const name = ev instanceof DragEvent ? ev.dataTransfer?.getData(AGENT_MIME) : "";
      if (!name) return;
      this.open(name, { group: e.group, direction: POSITION_TO_DIRECTION[e.position], index: e.index });
    });
    this.api.onDidActivePanelChange((e) => hooks.activeChanged(e.panel && !isFilePanel(e.panel.id) ? e.panel.id : null));
    setFileOpener((path, line, fromGroup) => this.openFile(path, line, fromGroup as DockviewGroupPanel | undefined));

    const port: DockPort = {
      toJSON: () => JSON.stringify(this.api.toJSON()),
      clear: () => this.api.clear(),
      fromJSON: (json) => this.api.fromJSON(JSON.parse(json)),
      openAgents: (names) => names.filter((n) => findAgent(n)).forEach((n) => this.open(n)),
      closeAgents: (names) => names.forEach((n) => this.closePanel(n)),
      // dockview delivers layout-change events in a microtask; one macrotask later they are all in.
      settled: () => new Promise((resolve) => setTimeout(resolve, 0)),
    };
    this.taskDock = new TaskDock(
      port,
      { load: layoutLoad, save: layoutSave },
      reportError,
    );
    // The only listener there will ever be: switching tasks does not add one.
    this.api.onDidLayoutChange(() => this.taskDock.changed());
  }

  /** Show the task's saved layout, minus agents moved out of it and plus agents moved into it since. */
  async show(task: { id: string; agents: readonly string[] }): Promise<void> {
    await this.taskDock.show(task.id, task.agents.filter((n) => findAgent(n)));
    this.reconcile();
  }

  /** Save the move in affected layouts too, so restarting cannot bring an old copy back. */
  moved(name: string, toId: string, taskIds: readonly string[]): Promise<void> {
    this.closed = this.closed.filter((tab) => tab.options.id !== name);
    return this.taskDock.moveAgent(name, toId, taskIds);
  }

  /** Empty the dock for a task that is being deleted; its layout is not saved. */
  detach(): Promise<void> {
    return this.taskDock.detach();
  }

  activeAgent(): string | null {
    const id = this.api.activePanel?.id ?? null;
    return id && !isFilePanel(id) ? id : null;
  }

  /** No `place`: focus the agent's panel, or open it in the active group. With `place`: open or move it there. */
  open(name: string, place?: Place): void {
    const existing = this.api.getPanel(name);
    const group = place?.group ?? this.api.activeGroup;
    const direction = place?.direction ?? "within";
    if (existing) {
      if (place && group && !this.isOnlyPanelOf(existing.id, group, direction)) {
        existing.api.moveTo({ group, position: DIRECTION_TO_POSITION[direction], index: place.index });
      }
      existing.api.setActive();
      return;
    }
    this.pending.set(name, Date.now());
    setTimeout(() => {
      this.pending.delete(name);
      this.reconcile();
    }, GRACE_MS);
    this.api.addPanel({
      id: name,
      title: name,
      component: "terminal",
      tabComponent: "agent",
      params: { agent: name },
      renderer: "always",
      ...(group ? { position: { referenceGroup: group, direction, index: place?.index } } : {}),
    });
  }

  /** Splitting a group's only panel against its own group would leave nothing to split from. */
  private isOnlyPanelOf(id: string, group: DockviewGroupPanel, direction: Direction): boolean {
    return direction !== "within" && group.panels.length === 1 && group.panels[0].id === id;
  }

  /** Move `group`'s front tab into a new group below or to the right. False when it has nothing to move
   *  (one tab or none): the caller opens a new agent there instead. */
  split(direction: "below" | "right", group: DockviewGroupPanel | undefined = this.api.activeGroup): boolean {
    const panel = group?.activePanel;
    if (!group || !panel || group.panels.length < 2) return false;
    panel.api.moveTo({ group, position: DIRECTION_TO_POSITION[direction] });
    panel.api.setActive();
    return true;
  }

  /** The group's "+" button, where a popover for it should hang. */
  addButtonOf(group: DockviewGroupPanel | undefined): HTMLElement | null {
    return group?.element.querySelector<HTMLElement>(".group-actions .icon-btn:last-child") ?? null;
  }

  closeActive(): void {
    const panel = this.api.activePanel;
    if (panel) this.closeTab(panel.id);
  }

  /** Only explicit panel closes enter history; task switches, moves and ending an agent do not. */
  private closeTab(id: string): void {
    const panel = this.api.getPanel(id);
    const taskId = this.taskDock.shown;
    if (!panel || !taskId) return;
    const saved = panel.toJSON();
    const closed: ClosedPanel = {
      taskId,
      groupId: panel.group.id,
      index: panel.group.panels.indexOf(panel),
      options: {
        id,
        component: panel.api.component,
        tabComponent: saved.tabComponent,
        title: panel.title,
        params: { ...panel.params },
        renderer: saved.renderer,
      },
    };
    panel.api.close();
    this.closed.push(closed);
    if (this.closed.length > 20) this.closed.shift();
    const agent = findAgent(id);
    this.hooks.tabClosed(agent ? `已收起 ${agentLabel(agent)}` : `已关闭 ${panel.title ?? id}`);
  }

  /** Restore one tab without replacing any layout changes made after it was closed. */
  async reopenClosed(): Promise<void> {
    let tab: ClosedPanel | undefined;
    while ((tab = this.closed.pop())) {
      const taskId = tab.taskId;
      if (!getTasks().some((task) => task.id === taskId)) continue;
      const id = tab.options.id;
      if (!isFilePanel(id) && (!findAgent(id) || !getTasks().some((task) => task.agents.includes(id)))) continue;
      await this.hooks.selectTask(tab.taskId);
      const existing = this.api.getPanel(id);
      if (existing) {
        existing.api.setActive();
        return;
      }
      const group = this.api.getGroup(tab.groupId) ?? this.api.activeGroup;
      this.api.addPanel({
        ...tab.options,
        ...(group ? { position: { referenceGroup: group.id, direction: "within", index: tab.index } } : {}),
      });
      return;
    }
  }

  /** Close panels whose agent is gone from the list. */
  reconcile(): void {
    for (const panel of [...this.api.panels]) {
      if (isFilePanel(panel.id)) continue;
      if (findAgent(panel.id)) this.pending.delete(panel.id);
      else if (!this.pending.has(panel.id)) panel.api.close();
    }
  }

  /**
   * A file clicked in a terminal opens as a tab in that terminal's group, next to it (用户 11-09: same row as the
   * conversations, one shown at a time). The same file again just comes forward.
   */
  openFile(path: string, line: number | undefined, fromGroup?: DockviewGroupPanel): void {
    const id = FILE_PREFIX + path;
    const existing = this.api.getPanel(id);
    if (existing) {
      existing.api.setActive();
      existing.api.updateParameters({ path, line });
      return;
    }
    const group = fromGroup ?? this.api.activeGroup;
    this.api.addPanel({
      id,
      title: path.split("/").pop() ?? path,
      component: "file",
      params: { path, line },
      ...(group ? { position: { referenceGroup: group, direction: "within" as const } } : {}),
    });
  }

  /** Close the panel of an agent that was just ended. */
  closePanel(name: string): void {
    this.api.getPanel(name)?.api.close();
  }
}
