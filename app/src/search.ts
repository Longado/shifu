import { type AgentView, agentLabel } from "./format";
import { projectName, type Task, taskLabel } from "./tasks";

/** Mirrors `Hit` in app/src-tauri/src/search.rs: an agent whose conversation mentions the query. */
export interface Hit {
  agent: string;
  count: number;
  snippet: string;
  user: boolean;
}

export interface Found {
  task: Task;
  agent: AgentView;
  /** The name, task or folder matched. */
  byName: boolean;
  /** A line of the conversation (or the last reply) around the match; null when only the name matched. */
  snippet: string | null;
  /** Messages in the conversation that match. */
  count: number;
}

const has = (text: string | null | undefined, q: string) => !!text && text.toLowerCase().includes(q);

/** Agents matching `query` by name (topic, own name, task, folder) or by what was said (backend `hits`, else the
 *  last reply). Name matches first; within each, the most recently active first. Agents without a task are left
 *  out: there is nowhere to show them. */
export function findAgents(query: string, tasks: readonly Task[], agents: readonly AgentView[], hits: readonly Hit[]): Found[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const found: Found[] = [];
  for (const task of tasks) {
    for (const name of task.agents) {
      const agent = agents.find((a) => a.name === name);
      if (!agent) continue;
      const byName =
        has(agentLabel(agent), q) || has(agent.name, q) || has(taskLabel(task, agents), q) || has(projectName(task.project), q);
      const hit = hits.find((h) => h.agent === name);
      const snippet = hit?.snippet ?? (has(agent.last_message, q) ? agent.last_message : null);
      if (byName || snippet) found.push({ task, agent, byName, snippet, count: hit?.count ?? (snippet ? 1 : 0) });
    }
  }
  return found.sort((a, b) => Number(b.byName) - Number(a.byName) || b.agent.status_since - a.agent.status_since);
}

/** `text` as nodes with each occurrence of `query` (any case) in a <mark>. */
export function highlight(text: string, query: string): Node[] {
  const q = query.trim().toLowerCase();
  if (!q) return [document.createTextNode(text)];
  const lower = text.toLowerCase();
  const nodes: Node[] = [];
  let from = 0;
  for (let at = lower.indexOf(q); at !== -1; at = lower.indexOf(q, from)) {
    if (at > from) nodes.push(document.createTextNode(text.slice(from, at)));
    const mark = document.createElement("mark");
    mark.textContent = text.slice(at, at + q.length);
    nodes.push(mark);
    from = at + q.length;
  }
  if (from < text.length) nodes.push(document.createTextNode(text.slice(from)));
  return nodes;
}
