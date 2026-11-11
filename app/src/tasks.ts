import { type AgentView, needsYou, sortAgents, type ViewKey, viewKey } from "./format";

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
}

/** Reply of the commands that start an agent: the new state and the agent's name. */
export interface Changed {
  file: TaskFile;
  agent: string | null;
}

export interface Project {
  path: string;
  name: string;
  /** Newest first. */
  tasks: Task[];
}

export const projectName = (path: string): string => path.split("/").filter(Boolean).pop() ?? path;

const newestFirst = (a: Task, b: Task) => b.created_at - a.created_at || b.id.localeCompare(a.id);

/** A folder with tasks is a project; the one with the newest task comes first. */
export function projectsOf(tasks: readonly Task[]): Project[] {
  const byPath = new Map<string, Task[]>();
  for (const t of tasks) byPath.set(t.project, [...(byPath.get(t.project) ?? []), t]);
  return [...byPath]
    .map(([path, list]) => ({ path, name: projectName(path), tasks: [...list].sort(newestFirst) }))
    .sort((a, b) => newestFirst(a.tasks[0], b.tasks[0]));
}

export const taskOfAgent = (tasks: readonly Task[], name: string): Task | undefined =>
  tasks.find((t) => t.agents.includes(name));

/** The task's agents that still exist, most urgent first. */
export function taskAgents(task: Task, agents: readonly AgentView[]): AgentView[] {
  return sortAgents(agents.filter((a) => task.agents.includes(a.name)));
}

/** A task stays active while any member works; attention ordering belongs to the child rows. */
export function taskStatus(task: Task, agents: readonly AgentView[]): ViewKey | null {
  const mine = taskAgents(task, agents);
  if (mine.some((agent) => agent.status === "running")) return "running";
  if (mine.some((agent) => agent.status === "starting")) return "starting";
  const first = mine[0];
  return first ? viewKey(first) : null;
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
