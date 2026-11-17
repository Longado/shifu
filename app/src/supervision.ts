import { type AgentView, agentLabel } from "./format";

export interface ProgressSummary { text: string; detail: string; attention: boolean }

/** Counts are supplied with project receipts by the controller, never inferred from a running badge. */
export function progressSummary(agents: readonly AgentView[]): ProgressSummary | null {
  const items = agents.flatMap((agent): ProgressSummary[] => {
    const progress = agent.progress;
    if (agent.progress_error) return [{ text: "成果记录待核", detail: `${agentLabel(agent)}\n${agent.progress_error}`, attention: true }];
    if (!progress) return [];
    const attention = progress.phase === "stalled" || progress.phase === "invalid" || progress.error !== null;
    const count = progress.completed === null ? "成果待核" : `本轮 ${progress.completed}/${progress.target}`;
    const text = progress.phase === "stopped" ? `${count}，托管已结束` : progress.phase === "stalled" ? `${count}，需调整` : count;
    const last = new Date(progress.last_progress_at * 1000).toLocaleString();
    return [{ text, detail: `${agentLabel(agent)}\n${progress.batch ?? ""}\n上次新增成果：${last}${progress.error ? `\n${progress.error}` : ""}`, attention }];
  });
  if (!items.length) return null;
  if (items.length === 1) return items[0];
  const waiting = items.filter((item) => item.attention).length;
  return { text: `${items.length} 项托管${waiting ? `，${waiting} 项需调整` : ""}`, detail: items.map((item) => `${item.text}\n${item.detail}`).join("\n\n"), attention: waiting > 0 };
}
