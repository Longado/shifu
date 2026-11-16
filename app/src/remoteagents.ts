export interface RemoteAgent {
  name: string;
  title: string;
  kind: string;
  task: string | null;
  task_id?: string | null;
  task_title?: string | null;
  status: string;
  cwd: string;
}

export const remoteStatuses: Record<string, string> = {
  starting: "启动中", idle: "空闲", running: "运行中", needs_approval: "待批准", needs_reply: "等待输入", exited: "已退出",
};

export function remoteAgents(value: unknown): RemoteAgent[] {
  if (!Array.isArray(value) || value.some(v => !v
    || ["name", "title", "kind", "status", "cwd"].some(k => typeof v[k] !== "string")
    || ["task", "task_id", "task_title"].some(k => !(v[k] == null || typeof v[k] === "string")))) {
    throw new Error("远端返回了无效的会话列表");
  }
  return value;
}

export interface RemoteTask {
  id: string;
  title: string;
  agents: RemoteAgent[];
}

export function remoteTaskGroups(agents: readonly RemoteAgent[], query: string): {tasks: RemoteTask[]; ungrouped: RemoteAgent[]} {
  const tasks = new Map<string, RemoteTask>();
  const ungrouped: RemoteAgent[] = [];
  const search = query.trim().toLocaleLowerCase();
  const matches = (...values: string[]) => values.some(value => value.toLocaleLowerCase().includes(search));
  for (const agent of agents) {
    if (!matches(agent.title, agent.kind, agent.name, agent.task || "", agent.task_title || "")) continue;
    if (!agent.task_id || !agent.task_title) { ungrouped.push(agent); continue; }
    let task = tasks.get(agent.task_id);
    if (!task) { task = {id: agent.task_id, title: agent.task_title, agents: []}; tasks.set(task.id, task); }
    task.agents.push(agent);
  }
  return {tasks: [...tasks.values()], ungrouped};
}
