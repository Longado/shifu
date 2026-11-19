import { type AgentView, displayKey, needsYou, sortAgents, type ViewKey } from "./format";

/** Mirrors `Task` / `TaskFile` in app/src-tauri/src/tasks.rs. */
export interface Task {
  id: string;
  project: string;
  name: string;
  /** `name` was made by the app, so the first agent's topic is shown instead. A hand-made name sets this false. */
  auto_name: boolean;
  created_at: number;
  agents: string[];
}

export interface TaskFile {
  current: string | null;
  tasks: Task[];
  /** The dragged order of project folders, by path. Absent in files written before dragging existed. */
  project_order?: string[];
  task_order?: Record<string, string[]>;
  /** The dragged order of each task's conversations, by task id. */
  agent_order?: Record<string, string[]>;
}

/** Reply of the commands that start an agent: the new state and the agent's name. */
export interface Changed {
  file: TaskFile;
  agent: string | null;
}

export interface Project {
  path: string;
  name: string;
  /** A folder's saved order, then newest first for any unranked tasks. */
  tasks: Task[];
}

export const projectName = (path: string): string => path.split("/").filter(Boolean).pop() ?? path;

const newestFirst = (a: Task, b: Task) => b.created_at - a.created_at || b.id.localeCompare(a.id);

/** A folder with tasks is a project. The dragged order comes first; folders it does not name follow newest-first,
 *  so a project created after the last drag still shows up rather than disappearing. */
export function projectsOf(
  tasks: readonly Task[],
  order: readonly string[] = [],
  taskOrder: Readonly<Record<string, readonly string[]>> = {},
): Project[] {
  const byPath = new Map<string, Task[]>();
  for (const t of tasks) byPath.set(t.project, [...(byPath.get(t.project) ?? []), t]);
  const rank = (path: string) => {
    const at = order.indexOf(path);
    return at < 0 ? order.length : at;
  };
  const projects = [...byPath]
    .map(([path, list]) => ({ path, name: projectName(path), tasks: [...list].sort(newestFirst) }))
    .sort((a, b) => rank(a.path) - rank(b.path) || newestFirst(a.tasks[0], b.tasks[0]));
  for (const project of projects) {
    const ids = taskOrder[project.path];
    if (!ids?.length) continue;
    const ranks = new Map(ids.map((id, index) => [id, index]));
    project.tasks.sort((a, b) => (ranks.get(a.id) ?? ids.length) - (ranks.get(b.id) ?? ids.length));
  }
  return projects;
}

export const taskOfAgent = (tasks: readonly Task[], name: string): Task | undefined =>
  tasks.find((t) => t.agents.includes(name));

/** The task's agents that still exist, most urgent first. */
export function taskAgents(task: Task, agents: readonly AgentView[]): AgentView[] {
  return sortAgents(agents.filter((a) => task.agents.includes(a.name)));
}

/** A dragged order wins; conversations it does not name yet follow in the status-first default. */
export function inDraggedOrder(agents: readonly AgentView[], order: readonly string[] | undefined): AgentView[] {
  if (!order?.length) return [...agents];
  const rank = (a: AgentView) => { const i = order.indexOf(a.name); return i < 0 ? order.length : i; };
  return agents.map((a, i) => ({ a, i })).sort((x, y) => rank(x.a) - rank(y.a) || x.i - y.i).map(({ a }) => a);
}

/** A task stays active while any member works; attention ordering belongs to the child rows. */
export function taskStatus(task: Task, agents: readonly AgentView[], nowSecs = Math.floor(Date.now() / 1000)): ViewKey | null {
  const mine = taskAgents(task, agents);
  if (mine.some((agent) => agent.status === "running")) return "running";
  const starting = mine.filter((agent) => agent.status === "starting");
  if (starting.length) return starting.some((agent) => displayKey(agent, nowSecs) === "starting") ? "starting" : "unready";
  if (mine.some((agent) => agent.status === "needs_approval")) return "needs_approval";
  // Reading a question does not answer it, even when a completion comes first in unread ordering.
  if (mine.some((agent) => displayKey(agent, nowSecs) === "asked")) return "asked";
  const first = mine[0];
  return first ? displayKey(first, nowSecs) : null;
}

/** The name to show for a task: one the user typed, else the topic of its first agent that has one, else `name`. */
export function taskLabel(task: Task, agents: readonly AgentView[]): string {
  if (!task.auto_name) return task.name;
  for (const name of task.agents) {
    const agent = agents.find((a) => a.name === name);
    const title = agent?.label || agent?.title;
    if (title) return title;
  }
  return task.name;
}

/** A task with an agent that needs the user (approval or a new reply): its name is bold. */
export const taskNeedsYou = (task: Task, agents: readonly AgentView[]): boolean =>
  taskAgents(task, agents).some(needsYou);

export interface Waiting {
  agent: AgentView;
  task: Task;
}

/** "在等你": approvals first, then new replies, the one waiting longest first in each. */
export function waitingList(tasks: readonly Task[], agents: readonly AgentView[]): Waiting[] {
  const out: Waiting[] = [];
  for (const agent of agents) {
    const task = taskOfAgent(tasks, agent.name);
    if (task && needsYou(agent)) out.push({ agent, task });
  }
  const approval = (w: Waiting) => (w.agent.status === "needs_approval" ? 0 : 1);
  return out.sort(
    (a, b) => approval(a) - approval(b) || a.agent.status_since - b.agent.status_since || a.agent.name.localeCompare(b.agent.name),
  );
}
