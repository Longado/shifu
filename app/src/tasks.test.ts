import { describe, expect, it } from "vitest";
import type { AgentView, Mark, Status } from "./format";
import { projectsOf, type Task, taskLabel, taskNeedsYou, taskStatus, waitingList } from "./tasks";

function agent(name: string, status: Status, since = 1, mark?: Mark, title: string | null = null): AgentView {
  return { name, kind: "claude", cwd: "/w", dir_name: "w", branch: null, title, status, status_since: since, last_message: null, exited: status === "exited", mark };
}

function task(id: string, project: string, created_at: number, agents: string[] = [], auto_name = true): Task {
  return { id, project, name: id, auto_name, created_at, agents };
}

describe("projectsOf", () => {
  it("groups by folder, newest task first inside, project with the newest task first", () => {
    const got = projectsOf([task("t1", "/w/a", 10), task("t2", "/w/b", 20), task("t3", "/w/a", 30), task("t4", "/w/b", 5)]);
    expect(got.map((p) => [p.name, p.tasks.map((t) => t.id)])).toEqual([
      ["a", ["t3", "t1"]],
      ["b", ["t2", "t4"]],
    ]);
  });

  it("is empty without tasks", () => {
    expect(projectsOf([])).toEqual([]);
  });
});

describe("taskStatus", () => {
  const t = task("t", "/w", 1, ["a", "b", "c", "gone"]);
  it("is the most urgent status among the task's agents", () => {
    expect(taskStatus(t, [agent("a", "idle"), agent("b", "needs_reply", 1, "new"), agent("c", "running")])).toBe("new");
    expect(taskStatus(t, [agent("a", "exited"), agent("b", "idle")])).toBe("idle");
    expect(taskStatus(t, [agent("a", "needs_reply", 1, "new"), agent("b", "needs_approval", 99)])).toBe("needs_approval");
    // a parked reply ranks below a running agent
    expect(taskStatus(t, [agent("a", "needs_reply", 1, "parked"), agent("b", "running")])).toBe("running");
    expect(taskStatus(t, [agent("a", "needs_reply", 1, "parked"), agent("b", "idle")])).toBe("parked");
  });
  it("ignores other tasks' agents and is null with none of its own", () => {
    expect(taskStatus(t, [agent("x", "needs_approval")])).toBeNull();
    expect(taskStatus(task("e", "/w", 1), [agent("a", "running")])).toBeNull();
  });
});

describe("waitingList", () => {
  const tasks = [task("t1", "/w/a", 1, ["a1", "a2"]), task("t2", "/w/b", 2, ["b1", "b2", "b3"])];
  it("lists approvals first, then new replies, the longest wait first in each; parked ones stay out", () => {
    const got = waitingList(tasks, [
      agent("a1", "needs_reply", 50, "new"),
      agent("a2", "running", 1),
      agent("b1", "needs_approval", 90),
      agent("b2", "needs_reply", 20, "new"),
      agent("b3", "needs_reply", 1, "parked"),
      agent("orphan", "needs_reply", 1, "new"),
    ]);
    expect(got.map((w) => [w.agent.name, w.task.id])).toEqual([["b1", "t2"], ["b2", "t2"], ["a1", "t1"]]);
  });

  it("puts a longer-waiting approval above a newer one", () => {
    const got = waitingList(tasks, [agent("a1", "needs_approval", 80), agent("b1", "needs_approval", 30)]);
    expect(got.map((w) => w.agent.name)).toEqual(["b1", "a1"]);
  });
});

describe("taskNeedsYou", () => {
  const t = task("t", "/w", 1, ["a", "b"]);
  it("is true with an approval or a new reply, in any task", () => {
    expect(taskNeedsYou(t, [agent("a", "running"), agent("b", "needs_approval")])).toBe(true);
    expect(taskNeedsYou(t, [agent("a", "needs_reply", 1, "new")])).toBe(true);
  });
  it("is false with only parked replies or other states", () => {
    expect(taskNeedsYou(t, [agent("a", "needs_reply", 1, "parked"), agent("b", "idle")])).toBe(false);
    expect(taskNeedsYou(t, [agent("x", "needs_approval")])).toBe(false);
  });
});

describe("taskLabel", () => {
  const agents = [agent("a", "idle", 1, undefined, null), agent("b", "idle", 1, undefined, "修仙模拟器"), agent("c", "idle", 1, undefined, "别的话题")];
  it("a name the user typed always wins", () => {
    expect(taskLabel(task("手起的", "/w", 1, ["b"], false), agents)).toBe("手起的");
  });
  it("an auto name gives way to the first agent's topic", () => {
    expect(taskLabel(task("任务 3", "/w", 1, ["b", "c"]), agents)).toBe("修仙模拟器");
  });
  it("an auto name follows the first agent's tab name too", () => {
    const renamed = agents.map((a) => (a.name === "a" ? { ...a, label: "排版" } : a));
    expect(taskLabel(task("任务 3", "/w", 1, ["a", "c"]), renamed)).toBe("排版");
  });
  it("skips an agent without a topic, and falls back to the original name when none has one", () => {
    expect(taskLabel(task("任务 3", "/w", 1, ["a", "c"]), agents)).toBe("别的话题");
    expect(taskLabel(task("任务 3", "/w", 1, ["a"]), agents)).toBe("任务 3");
    expect(taskLabel(task("任务 3", "/w", 1, []), agents)).toBe("任务 3");
  });
});
