import { describe, expect, it } from "vitest";
import { progressSummary } from "./supervision";
import type { AgentView } from "./format";
const agent = (changes: Partial<AgentView> = {}): AgentView => ({
  name: "codex-task", kind: "codex", cwd: "/work", dir_name: "work", branch: null,
  title: "Task", status: "running", status_since: 1, last_message: null, exited: false, ...changes,
});
const progress = { run_id: "run", batch: "batch1", completed: 1, target: 3, last_progress_at: 10,
  deadline: "2030-01-01T12:00:00Z", phase: "stalled" as const, error: null };
describe("receipt progress on existing tasks", () => {
  it("does not turn a running badge or assistant reply into a completed outcome", () => {
    expect(progressSummary([agent({ last_message: "all done" })])).toBeNull();
  });
  it("keeps the actual shortfall visible while the agent is running", () => {
    expect(progressSummary([agent({ progress })])).toMatchObject({ text: "本轮 1/3，需调整", attention: true });
  });
  it("shows missing evidence rather than a healthy zero", () => {
    expect(progressSummary([agent({ progress: { ...progress, completed: null, phase: "invalid", error: "receipt changed" } })])).toMatchObject({ text: "成果待核", attention: true });
  });
  it("does not sum separate agents' batch targets", () => {
    const summary = progressSummary([agent({ progress }), agent({ name: "second", progress: { ...progress, phase: "met", completed: 3 } })]);
    expect(summary?.text).toBe("2 项托管，1 项需调整");
  });
  it("distinguishes cutoff from meeting a target", () => {
    expect(progressSummary([agent({ progress: { ...progress, phase: "stopped" } })])?.text).toBe("本轮 1/3，托管已结束");
  });
  it("does not present the observation start as an actual new outcome", () => {
    const summary = progressSummary([agent({ progress: { ...progress, completed: 0, last_progress_at: 0 } })]);
    expect(summary?.detail).toContain("尚无新增成果");
    expect(summary?.detail).not.toContain("上次新增成果");
  });
  it("labels review coverage and keeps cached tokens inside the input figure", () => {
    const review = { recorded_judgments: 3, reported_judgments: 2, input_tokens: 1200,
      cached_input_tokens: 400, output_tokens: 0 };
    const summary = progressSummary([agent({ progress: { ...progress, review } })]);
    expect(summary?.text).toBe("本轮 1/3，需调整");
    expect(summary?.detail).toContain("托管判断 3 次，2 次报告用量");
    expect(summary?.detail).toContain("输入 1,200，其中缓存输入 400，输出 0");
    const missing = progressSummary([agent({ progress: { ...progress, review: {
      recorded_judgments: 1, reported_judgments: 0, input_tokens: null, cached_input_tokens: null, output_tokens: null,
    } } })]);
    expect(missing?.detail).toContain("用量尚未报告");
    expect(missing?.detail).not.toContain("输入 0");
  });
});
