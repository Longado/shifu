import { type AgentView, type ReviewUsage, agentLabel } from "./format";

interface ReceiptLink { agent: string; id: string; path: string }
export interface ProgressSummary { text: string; detail: string; attention: boolean; receipts: ReceiptLink[] }

function reviewDetail(review: ReviewUsage): string {
  if (!review.reported_judgments) return `托管判断 ${review.recorded_judgments} 次，用量尚未报告`;
  const meters = [
    ["输入", review.input_tokens], ["其中缓存输入", review.cached_input_tokens], ["输出", review.output_tokens],
  ] as const;
  const tokens = meters.flatMap(([label, value]) => value === null ? [] : [`${label} ${value.toLocaleString("zh-CN")}`]);
  const coverage = `托管判断 ${review.recorded_judgments} 次，${review.reported_judgments} 次报告用量`;
  return tokens.length ? `${coverage}\n已报告的 token（${tokens.join("，")}）` : coverage;
}

/** Counts are supplied with project receipts by the controller, never inferred from a running badge. */
export function progressSummary(agents: readonly AgentView[]): ProgressSummary | null {
  const items = agents.flatMap((agent): ProgressSummary[] => {
    const progress = agent.progress;
    if (agent.progress_error) return [{ text: "成果记录待核", detail: `${agentLabel(agent)}\n${agent.progress_error}`, attention: true, receipts: [] }];
    if (!progress) return [];
    const attention = progress.phase === "stalled" || progress.phase === "invalid" || progress.error !== null;
    const count = progress.completed === null ? "成果待核" : `本轮 ${progress.completed}/${progress.target}`;
    const text = progress.phase === "stopped" ? `${count}，托管已结束` : progress.phase === "stalled" ? `${count}，需调整` : count;
    const last = progress.last_progress_at > 0 ? `上次新增成果：${new Date(progress.last_progress_at * 1000).toLocaleString()}` : "尚无新增成果";
    const receipt = progress.completed && !progress.error && progress.phase !== "invalid" ? progress.receipt : null;
    const proof = receipt ? `已计入成果 ${receipt.id}\n${receipt.evidence}\nSHA256 ${receipt.sha256}` : null;
    const detail = [agentLabel(agent), progress.batch, last, progress.error, progress.review && reviewDetail(progress.review), proof]
      .filter(Boolean).join("\n");
    const receipts = receipt ? [{ agent: agent.name, id: receipt.id, path: receipt.evidence }] : [];
    return [{ text, detail, attention, receipts }];
  });
  if (!items.length) return null;
  if (items.length === 1) return items[0];
  const waiting = items.filter((item) => item.attention).length;
  return { text: `${items.length} 项托管${waiting ? `，${waiting} 项需调整` : ""}`, detail: items.map((item) => `${item.text}\n${item.detail}`).join("\n\n"),
    attention: waiting > 0, receipts: items.flatMap(item => item.receipts) };
}
