import { describe, expect, it } from "vitest";
import type { AgentView } from "./format";
import { goalRows, parseGoals, percent, projectProgress, wbRows, wbShown, wbWhen, type WbSession } from "./progress";

const agent = (name: string, over: Partial<AgentView> = {}): AgentView => ({
  name,
  kind: "claude",
  cwd: "/w",
  dir_name: "w",
  branch: null,
  title: null,
  status: "running",
  status_since: 0,
  last_message: null,
  exited: false,
  ...over,
});

describe("parseGoals", () => {
  it("takes the first heading and every checkbox line, nested or not", () => {
    const g = parseGoals("intro\n# Shifu 代替 Orca\n\n## 已做\n- [x] **A1** 桌面版\n  - [X] 子项\n* [ ] `C2` 花园\n- 普通列表\n", "/w/shifu");
    expect(g.title).toBe("Shifu 代替 Orca");
    expect(g.items).toEqual([
      { done: true, text: "A1 桌面版" },
      { done: true, text: "子项" },
      { done: false, text: "C2 花园" },
    ]);
  });

  it("falls back to the folder name", () => {
    expect(parseGoals("- [ ] x", "/w/01_项目/jade").title).toBe("jade");
  });
});

describe("projectProgress", () => {
  const goals = {
    files: [
      { dir: "/w/a", text: "- [x] 1\n- [ ] 2\n- [ ] 3" },
      { dir: "/w/b", text: "- [x] 1" },
      { dir: "/w/prose", text: "# 只有文字\n没有勾选框" },
    ],
    agents: { one: "/w/b", gone: "/w/b", two: "/w/a" },
  };

  it("drops projects without checkboxes, counts, places live agents and puts busy projects first", () => {
    const agents = [agent("one"), agent("gone", { exited: true, status: "exited" }), agent("two"), agent("three")];
    const p = projectProgress(goals, agents);
    expect(p.map((x) => [x.dir, x.done, x.total, x.agents.map((a) => a.name)])).toEqual([
      ["/w/a", 1, 3, ["two"]],
      ["/w/b", 1, 1, ["one"]],
    ]);
    expect(percent(1, 3)).toBe(33);
  });
});

describe("WorkBuddy", () => {
  const t = (subject: string, status: string) => ({ subject, status });

  it("marks what it is doing; with nothing in progress the first pending step is next", () => {
    expect(wbRows([t("a", "completed"), t("b", "in_progress"), t("c", "pending")]).map((r) => [r.done, r.tag])).toEqual([
      [true, undefined],
      [false, "正在做"],
      [false, undefined],
    ]);
    expect(wbRows([t("a", "completed"), t("b", "pending"), t("c", "pending")]).map((r) => r.tag)).toEqual([undefined, "下一步", undefined]);
    expect(wbRows([t("a", "completed"), t("b", "in_progress"), t("c", "in_progress")], true).map((r) => r.tag)).toEqual([undefined, "停在这", undefined]);
    expect(goalRows([{ done: true, text: "x" }, { done: false, text: "y" }]).map((r) => r.tag)).toEqual([undefined, "下一步"]);
  });

  it("shows conversations with a task list or still running, at most five", () => {
    const s = (id: string, status: string, n: number): WbSession => ({ id, title: id, status, updated_at: 0, tasks: Array(n).fill(t("x", "pending")) });
    const shown = wbShown([s("running", "working", 0), s("chat", "completed", 0), ...["a", "b", "c", "d", "e"].map((id) => s(id, "completed", 2))]);
    expect(shown.map((x) => x.id)).toEqual(["running", "a", "b", "c", "d"]);
  });

  it("dates: clock time today, day otherwise", () => {
    const now = new Date(2026, 10, 9, 15, 0).getTime();
    expect(wbWhen(new Date(2026, 10, 9, 14, 5).getTime(), now)).toBe("今天 14:05");
    expect(wbWhen(new Date(2026, 7, 13, 9, 0).getTime(), now)).toBe("8月13日");
  });
});
