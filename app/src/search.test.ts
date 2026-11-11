import { describe, expect, it } from "vitest";
import type { AgentView } from "./format";
import { findAgents } from "./search";
import type { Task } from "./tasks";

function agent(name: string, since: number, title: string | null, last: string | null = null): AgentView {
  return { name, kind: "claude", cwd: "/w", dir_name: "w", branch: null, title, status: "idle", status_since: since, last_message: last, exited: false };
}
const task = (id: string, name: string, agents: string[]): Task => ({ id, project: "/u/Workspace", name, auto_name: false, created_at: 1, agents });

describe("findAgents", () => {
  const agents = [agent("a", 10, "赌石模拟器云端同步"), agent("b", 30, "股市", "分屏已经做好"), agent("c", 20, "Kimi 接入"), agent("orphan", 99, "分屏")];
  const tasks = [task("t1", "游戏", ["a"]), task("t2", "Shifu 分屏", ["b", "c"])];

  it("puts name matches first, then conversation matches, newest first within each", () => {
    const got = findAgents("分屏", tasks, agents, [{ agent: "a", count: 3, snippet: "…讲到分屏…", user: true }]);
    // b and c match through their task's name; a only through what was said; orphan has no task.
    expect(got.map((f) => [f.agent.name, f.byName, f.snippet, f.count])).toEqual([
      ["b", true, "分屏已经做好", 1],
      ["c", true, null, 0],
      ["a", false, "…讲到分屏…", 3],
    ]);
  });

  it("ignores case and surrounding spaces, and finds nothing for an empty query", () => {
    expect(findAgents("  kimi ", tasks, agents, []).map((f) => f.agent.name)).toEqual(["c"]);
    expect(findAgents("workspace", tasks, agents, []).length).toBe(3);
    expect(findAgents("   ", tasks, agents, [])).toEqual([]);
  });
});
