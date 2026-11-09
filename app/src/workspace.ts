import { createDockview, themeDark } from "dockview";
import type {
  DockviewApi,
  DockviewGroupPanel,
  IContentRenderer,
  IHeaderActionsRenderer,
  ITabRenderer,
  IWatermarkRenderer,
  Position,
} from "dockview";
import { AGENT_MIME, layoutLoad, layoutSave } from "./api";
import { AgentTab } from "./tab";
import { TerminalPanel } from "./terminal";
import { FILE_PREFIX, FilePanel, isFilePanel, setFileOpener } from "./preview";
import { findAgent, getCurrentTask, reportError, subscribe } from "./store";
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
  /** The empty state's buttons. */
  addAgent(kind: string): void;
  requestNewTask(anchor: HTMLElement): void;
  activeChanged(name: string | null): void;
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
    this.element.append(add);
  }
  init(): void {}
  dispose(): void {}
}

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
      this.button("Claude", () => this.hooks.addAgent("claude")),
      this.button("Codex", () => this.hooks.addAgent("codex")),
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
  /** Agents moved to another task while the app runs; other tasks' saved layouts may still hold their panels. */
  private readonly relocated = new Set<string>();
  /** Agents moved into a task that was not on screen: their tab opens when it is shown. */
  private readonly joined = new Set<string>();
  private readonly taskDock: TaskDock;

  constructor(root: HTMLElement, hooks: WorkspaceHooks) {
    this.api = createDockview(root, {
      theme: themeDark,
      disableFloatingGroups: true,
      createComponent: (o): IContentRenderer => (o.name === "file" ? new FilePanel() : new TerminalPanel()),
      // File tabs take dockview's default tab (name, close button).
      createTabComponent: (o): ITabRenderer | undefined => (o.name === "agent" ? new AgentTab() : undefined),
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
      openAgents: (names) => names.forEach((n) => this.open(n)),
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
    for (const name of this.relocated) if (!task.agents.includes(name)) this.closePanel(name);
    for (const name of task.agents) if (this.joined.delete(name) && findAgent(name)) this.open(name);
    this.reconcile();
  }

  /** An agent was moved to another task (the tab goes with it). Into the task on screen: it opens
   *  here. Into another: its tab leaves this view and opens when that task is next shown. */
  moved(name: string, intoShown: boolean): void {
    this.relocated.add(name);
    if (intoShown) return this.open(name);
    this.closePanel(name);
    this.joined.add(name);
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

  closeActive(): void {
    this.api.activePanel?.api.close();
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
   * A file clicked in a terminal opens as a tab in that terminal's group, next to it (same row as the
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
