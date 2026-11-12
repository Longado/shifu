import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import "dockview/dist/styles/dockview.css";
import "./style.css";
import { showAppearancePopover } from "./appearance";
import { applyStoredTheme } from "./theme";
import { ProgressPage } from "./progress";
import { pickArgs, pickFor, refreshModels } from "./models";
import {
  agentKill,
  agentResume,
  agentReveal,
  agentsList,
  goalsGet,
  jevLogTail,
  notifySend,
  pathReveal,
  pmDir,
  pmOpen,
  waimaiOpen,
  seenLoad,
  seenSave,
  setAttention,
  taskAddAgent,
  taskDelete,
  taskMoveAgent,
  taskNew,
  taskRename,
  taskSetCurrent,
  tasksLoad,
} from "./api";
import { baselineSeen } from "./attention";
import type { AgentView } from "./format";
import { closeMenu } from "./menu";
import { waitForVerdict } from "./jev";
import { currentlyNeeding, nextNotices, type Notice, type Notified } from "./notify";
import { closeNewPopover, showAddAgentPopover, showNewTaskPopover } from "./newpopover";
import { startPtyBus } from "./ptybus";
import { Sidebar } from "./sidebar";
import { MemoryBar } from "./memory";
import { LimitsBar } from "./limitsbar";
import { PowerButton } from "./power";
import {
  getAgents,
  getCurrentId,
  getCurrentTask,
  getTasks,
  initSeen,
  onAttentionChange,
  onSeenChange,
  reportError,
  setActiveAgent,
  setAgents,
  setCurrent,
  setTaskFile,
  setWindowFocused,
  subscribe,
} from "./store";
import { projectsOf, taskOfAgent, waitingList } from "./tasks";
import { Workspace } from "./workspace";
import type { DockviewGroupPanel } from "dockview";

async function main() {
  applyStoredTheme();
  // The picker uses last launch's list until this answers (a couple of seconds).
  void refreshModels().catch(reportError);
  await startPtyBus();
  const initial = await agentsList();
  // First start with this version: everything that exists counts as seen. The baseline is written right away,
  // so a restart before any reply is seen does not make them all new.
  const stored = await seenLoad();
  const seen = stored ?? baselineSeen(initial);
  if (!stored) await seenSave(seen).catch(reportError);
  initSeen(seen);
  onSeenChange((next) => void seenSave(next).catch(reportError));
  onAttentionChange((count) => void setAttention(count).catch(reportError));
  setWindowFocused(document.hasFocus());
  window.addEventListener("focus", () => setWindowFocused(true));
  window.addEventListener("blur", () => setWindowFocused(false));
  setAgents(initial);
  // Notifications (APP-SPEC-B4): what already needs the user at start-up is not announced.
  let notified: Notified = currentlyNeeding(getAgents());
  // A new Claude reply waits (at most 3 s) for Jev's verdict; when Jev says it was a report there is no notification.
  const deliver = async (n: Notice) => {
    if (n.jev) {
      const verdict = await waitForVerdict(n.jev.session, n.jev.since, {
        readTail: jevLogTail,
        sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
      });
      if (verdict === "report") return;
    }
    await notifySend(n.agent, n.title, n.body);
  };
  subscribe(() => {
    const next = nextNotices(notified, getAgents(), document.hasFocus());
    notified = next.notified;
    for (const n of next.notices) void deliver(n).catch(reportError);
  });
  const file = await tasksLoad();
  setTaskFile(file);

  const app = document.getElementById("app")!;
  const dock = document.createElement("div");
  dock.className = "dock";

  /** Make `id` the current task: its layout (or its agents as tabs) replaces the one on screen. */
  const showTask = async (id: string) => {
    const task = getTasks().find((t) => t.id === id);
    if (!task) return;
    setCurrent(id);
    taskSetCurrent(id).catch(reportError);
    await workspace.show(task);
  };
  const selectTask = async (id: string) => {
    if (getCurrentId() !== id) await showTask(id);
  };
  const focusAgent = async (taskId: string, name: string) => {
    hideProgress();
    await selectTask(taskId);
    workspace.open(name);
  };

  const createTask = async (project: string, name: string, kind: string) => {
    const { file } = await taskNew(project, name, kind, pickArgs(pickFor(kind)));
    // The new agent has to be in the list before its task is shown, or the panel would be dropped as stale.
    setAgents(await agentsList());
    setTaskFile(file);
    if (file.current) await showTask(file.current);
  };

  /** Add an agent to the current task and open it (in `group` if given, or split off it toward `direction`). */
  const addAgent = async (kind: string, group?: DockviewGroupPanel, direction?: "below" | "right") => {
    const id = getCurrentId();
    if (!id) throw new Error("先新建一个任务");
    const { file, agent } = await taskAddAgent(id, kind, pickArgs(pickFor(kind)));
    setTaskFile(file);
    if (agent && getCurrentId() === id) workspace.open(agent, { group, direction });
  };

  /** Which agent the project manager is (its model is remembered per agent, like everywhere else). Asked the first
   *  time it starts; later starts, after it was ended, use the same without asking . */
  const PM_KIND_KEY = "shifu.pmKind";
  const startPm = async (kind: string) => {
    const { file, agent } = await pmOpen(kind, pickArgs(pickFor(kind)));
    // A just-started agent has to be in the list before its task is shown, or its panel is dropped as stale.
    if (agent) setAgents(await agentsList());
    setTaskFile(file);
    const dir = await pmDir();
    const task = file.tasks.find((t) => t.project === dir);
    if (task) await selectTask(task.id);
  };
  const openPm = async (anchor: HTMLElement) => {
    const dir = await pmDir();
    const running = getTasks().find((t) => t.project === dir && t.agents.length > 0);
    if (running) return selectTask(running.id);
    const kind = localStorage.getItem(PM_KIND_KEY);
    if (kind) return startPm(kind);
    showAddAgentPopover(anchor, async (picked) => {
      localStorage.setItem(PM_KIND_KEY, picked);
      await startPm(picked);
    });
  };

  const deleteTask = async (id: string) => {
    const task = getTasks().find((t) => t.id === id);
    const wasCurrent = getCurrentId() === id;
    // Its panels go first; its layout must not be saved again after the file is deleted.
    if (wasCurrent) await workspace.detach();
    try {
      const file = await taskDelete(id);
      setTaskFile(file);
      setAgents(await agentsList());
      if (wasCurrent) {
        setCurrent(file.current);
        if (file.current) await showTask(file.current);
      }
    } catch (e) {
      reportError(e);
      if (wasCurrent && task) await workspace.show(task);
    }
  };

  const requestNewTask = (anchor: HTMLElement, project?: string) =>
    void showNewTaskPopover(anchor, {
      projects: projectsOf(getTasks()).map((p) => p.path),
      preselect: project ?? getCurrentTask()?.project,
      create: createTask,
    });

  const sidebar = new Sidebar({
    selectTask: (id) => {
      hideProgress();
      void selectTask(id).catch(reportError);
    },
    focusAgent: (taskId, name) => void focusAgent(taskId, name).catch(reportError),
    openAgent: (name) => {
      hideProgress();
      workspace.open(name);
    },
    openRight: (name) => workspace.open(name, { direction: "right" }),
    openBelow: (name) => workspace.open(name, { direction: "below" }),
    revealAgent: (name) => void agentReveal(name).catch(reportError),
    resume: (name) => void agentResume(name).catch(reportError),
    moveAgent: (name, toId) =>
      void taskMoveAgent(name, toId)
        .then((file) => {
          setTaskFile(file);
          return workspace.moved(name, toId, file.tasks.map((task) => task.id));
        })
        .catch(reportError),
    killAgent: (name) =>
      void agentKill(name)
        .then((file) => {
          setTaskFile(file);
          workspace.closePanel(name);
        })
        .catch(reportError),
    renameTask: (id, name) => void taskRename(id, name).then(setTaskFile).catch(reportError),
    revealProject: (path) => void pathReveal(path).catch(reportError),
    deleteTask: (id) => void deleteTask(id),
    requestNewTask,
    showProgress: () => showProgress(),
    openPm: (anchor) => {
      hideProgress();
      void openPm(anchor).catch(reportError);
    },
    openWaimai: () => void waimaiOpen().catch(reportError),
    repickPm: () => localStorage.removeItem(PM_KIND_KEY),
  });
  void pmDir().then((dir) => sidebar.setPmDir(dir)).catch(reportError);
  const body = document.createElement("div");
  body.className = "app-body";
  body.append(sidebar.element, dock);
  const statusbar = document.createElement("footer");
  statusbar.className = "statusbar";
  statusbar.setAttribute("aria-label", "用量与进程监控");
  const usage = document.createElement("div");
  usage.className = "status-usage";
  usage.append(new LimitsBar().element);
  statusbar.append(usage, new MemoryBar({
    focusAgent: (id, name) => void focusAgent(id, name).catch(reportError),
    openAgent: (name) => { hideProgress(); workspace.open(name); },
  }).element, new PowerButton().element);
  app.append(body, statusbar);

  // 进度 covers the task layout without touching it; picking any task or agent brings the layout back.
  const progress = new ProgressPage((name) => {
    const task = taskOfAgent(getTasks(), name);
    if (task) void focusAgent(task.id, name).catch(reportError);
  });
  dock.append(progress.element);
  subscribe(() => progress.setAgents(getAgents()));
  let progressTimer: number | undefined;
  const refreshGoals = () => void goalsGet().then((g) => progress.setGoals(g)).catch(reportError);
  function showProgress(): void {
    progress.setAgents(getAgents());
    progress.element.hidden = false;
    sidebar.setProgressActive(true);
    refreshGoals();
    window.clearInterval(progressTimer);
    progressTimer = window.setInterval(refreshGoals, 5000);
  }
  function hideProgress(): void {
    if (progress.element.hidden) return;
    progress.element.hidden = true;
    sidebar.setProgressActive(false);
    window.clearInterval(progressTimer);
  }

  const workspace = new Workspace(dock, {
    requestAddAgent: (anchor, group) => showAddAgentPopover(anchor, (kind) => addAgent(kind, group)),
    requestSplit: (anchor, group, direction) => split(direction, group, anchor),
    addAgent: (kind) => void addAgent(kind).catch(reportError),
    requestNewTask: (anchor) => requestNewTask(anchor),
    activeChanged: (name) => {
      sidebar.setActive(name);
      setActiveAgent(name);
    },
  });
  if (file.current) await showTask(file.current);

  /** Split a group (the active one by default): its front tab moves over, or a new agent opens on that side. */
  function split(direction: "below" | "right", group = workspace.api.activeGroup, anchor?: HTMLElement): void {
    if (!group || workspace.split(direction, group)) return;
    const at = anchor ?? workspace.addButtonOf(group) ?? sidebar.newButton;
    showAddAgentPopover(at, (kind) => addAgent(kind, group, direction));
  }

  // The sidebar can be hidden (button by the traffic lights, Cmd+B); the choice survives a restart.
  const SIDEBAR_KEY = "shifu.sidebarHidden";
  const sideToggle = document.createElement("button");
  sideToggle.className = "icon-btn side-toggle";
  sideToggle.innerHTML =
    '<svg width="15" height="15" viewBox="0 0 15 15" fill="none" stroke="currentColor" stroke-width="1.3"><rect x="1.5" y="2.5" width="12" height="10" rx="1.8"/><path d="M5.5 2.5v10"/></svg>';
  const lightsDrag = document.createElement("div");
  lightsDrag.className = "lights-drag";
  lightsDrag.dataset.tauriDragRegion = "";
  document.body.append(lightsDrag, sideToggle);
  const setSidebarHidden = (hidden: boolean) => {
    document.body.classList.toggle("side-hidden", hidden);
    sideToggle.title = hidden ? "显示侧栏  ⌘B" : "隐藏侧栏  ⌘B";
    sideToggle.setAttribute("aria-label", hidden ? "显示侧栏" : "隐藏侧栏");
    localStorage.setItem(SIDEBAR_KEY, hidden ? "1" : "0");
  };
  sideToggle.addEventListener("click", () => setSidebarHidden(!document.body.classList.contains("side-hidden")));
  // With the sidebar hidden the traffic lights and this button sit over the top-left group's tab bar; that group
  // (the first in document order) leaves them room.
  const markCorner = () => {
    dock.querySelectorAll(".under-lights").forEach((g) => g.classList.remove("under-lights"));
    dock.querySelector(".dv-groupview")?.classList.add("under-lights");
  };
  workspace.api.onDidLayoutChange(markCorner);
  markCorner();
  setSidebarHidden(localStorage.getItem(SIDEBAR_KEY) === "1");

  await listen<AgentView[]>("agents", ({ payload }) => {
    setAgents(payload);
    workspace.reconcile();
  });

  // The backend brings the window forward when a notification is clicked, then names the agent to jump to.
  await listen<{ agent: string }>("notify-jump", ({ payload }) => {
    closeMenu();
    closeNewPopover();
    const task = taskOfAgent(getTasks(), payload.agent);
    if (task) void focusAgent(task.id, payload.agent).catch(reportError);
  });
  await listen<string>("notify-error", ({ payload }) => reportError(payload));

  // Cmd+N / Cmd+T / Cmd+W / Cmd+J come from the native menu (the default Cmd+W would close the window).
  await listen<string>("menu", ({ payload }) => {
    closeMenu();
    if (payload === "new-task") requestNewTask(sidebar.newButton);
    else if (payload === "new")
      showAddAgentPopover(sidebar.newButton, (kind) => addAgent(kind, workspace.api.activeGroup));
    else if (payload === "close") workspace.closeActive();
    else if (payload === "split-down") split("below");
    else if (payload === "split-right") split("right");
    else if (payload === "sidebar") sideToggle.click();
    else if (payload === "search") {
      setSidebarHidden(false);
      sidebar.focusSearch();
    }
    else if (payload === "appearance") showAppearancePopover(sidebar.newButton);
    else if (payload === "jump") {
      closeNewPopover();
      const first = waitingList(getTasks(), getAgents())[0];
      if (first) void focusAgent(first.task.id, first.agent.name).catch(reportError);
    }
  });
}

main().catch((e) => {
  document.body.textContent = `Shifu 没能启动: ${e instanceof Error ? e.message : String(e)}`;
});
