import { describe, expect, it } from "vitest";
import type { AgentView, Mark, Status } from "./format";
import { inDraggedOrder, projectsOf, type Task, taskLabel, taskNeedsYou, taskStatus, waitingList, waitingPresentation } from "./tasks";

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

  it("a dragged order comes first, and folders it does not name follow newest-first", () => {
    const tasks = [task("t1", "/w/a", 10), task("t2", "/w/b", 20), task("t3", "/w/c", 30)];
    expect(projectsOf(tasks, ["/w/a", "/w/b"]).map((p) => p.path)).toEqual(["/w/a", "/w/b", "/w/c"]);
    // A folder made after the last drag is not named yet, and still shows up instead of disappearing.
    expect(projectsOf(tasks, ["/w/c"]).map((p) => p.path)).toEqual(["/w/c", "/w/b", "/w/a"]);
  });

  it("an order naming a folder with no tasks left is ignored", () => {
    const tasks = [task("t1", "/w/a", 10), task("t2", "/w/b", 20)];
    expect(projectsOf(tasks, ["/w/gone", "/w/a"]).map((p) => p.path)).toEqual(["/w/a", "/w/b"]);
  });

  it("without an order the newest task's folder still comes first", () => {
    const tasks = [task("t1", "/w/a", 10), task("t2", "/w/b", 20)];
    expect(projectsOf(tasks, []).map((p) => p.path)).toEqual(["/w/b", "/w/a"]);
  });

  it("only changes task order within its own folder and still shows new tasks", () => {
    const tasks = [task("t1", "/w/a", 10), task("t2", "/w/b", 40), task("t3", "/w/a", 50), task("t4", "/w/a", 30)];
    const got = projectsOf(tasks, [], { "/w/a": ["t1", "t3", "t-gone"] });
    expect(got.map((p) => [p.name, p.tasks.map((t) => t.id)])).toEqual([
      ["a", ["t1", "t3", "t4"]],
      ["b", ["t2"]],
    ]);
  });
});

describe("taskStatus", () => {
  const t = task("t", "/w", 1, ["a", "b", "c", "gone"]);
  it("shows active work first, otherwise the most urgent remaining status", () => {
    expect(taskStatus(t, [agent("a", "idle"), agent("b", "needs_reply", 1, "new"), agent("c", "running")])).toBe("running");
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

  it("keeps the parent running even when another child is done, asking, or awaiting approval", () => {
    for (const waiting of [agent("a", "needs_reply", 1, "new"), agent("a", "needs_approval"),
      { ...agent("a", "needs_reply", 1, "new"), last_message: "继续吗？" }]) {
      expect(taskStatus(t, [waiting, agent("b", "running")])).toBe("running");
      expect(taskNeedsYou(t, [waiting, agent("b", "running")])).toBe(true);
      expect(waitingList([t], [waiting, agent("b", "running")]).map(w => w.agent.name)).toEqual(["a"]);
    }
  });

  it("does not show a completion check while a child is still starting", () => {
    expect(taskStatus(t, [agent("a", "needs_reply", 1, "new"), agent("b", "starting")], 30)).toBe("starting");
    expect(taskStatus(t, [agent("a", "starting"), agent("b", "running")])).toBe("running");
  });

  it("shows a startup check only when every starting child has passed the deadline", () => {
    const blocked = agent("a", "starting", 1000);
    expect(taskStatus(t, [blocked], 1059)).toBe("starting");
    expect(taskStatus(t, [blocked], 1060)).toBe("unready");
    expect(taskStatus(t, [blocked, agent("b", "starting", 1050)], 1060)).toBe("starting");
    expect(taskStatus(t, [blocked, agent("b", "starting", 1050)], 1110)).toBe("unready");
    expect(taskStatus(t, [blocked, agent("b", "running")], 1110)).toBe("running");
    expect(taskStatus(t, [agent("a", "idle"), agent("b", "needs_approval")], 1110)).toBe("needs_approval");
    expect(taskNeedsYou(t, [blocked])).toBe(false);
    expect(waitingList([t], [blocked])).toEqual([]);
  });

  it("keeps a read follow-up in the waiting area without adding unread attention", () => {
    const followup = { ...agent("a", "needs_reply", 1, "parked"), last_message: "继续吗？" };
    expect(taskStatus(t, [followup])).toBe("asked");
    expect(taskNeedsYou(t, [followup])).toBe(false);
    expect(waitingList([t], [followup]).map(w => w.agent.name)).toEqual(["a"]);
    expect(taskStatus(t, [followup, agent("b", "running")])).toBe("running");
    expect(taskStatus(t, [followup, agent("b", "needs_approval")])).toBe("needs_approval");
  });

  it("keeps a pending follow-up visible when another member has finished, regardless of read order", () => {
    const followup = { ...agent("a", "needs_reply", 20, "parked"), last_message: "继续吗？" };
    for (const mark of ["new", "parked"] as const) {
      for (const since of [1, 30]) {
        const completed = { ...agent("b", "needs_reply", since, mark), last_message: "已经完成。" };
        for (const members of [[completed, followup], [followup, completed]]) {
          expect(taskStatus(t, members)).toBe("asked");
          expect(taskNeedsYou(t, members)).toBe(mark === "new");
          const expected = mark === "new" ? [completed, followup].sort((a, b) => a.status_since - b.status_since || a.name.localeCompare(b.name)) : [followup];
          expect(waitingList([t], members).map(w => w.agent.name)).toEqual(expected.map(a => a.name));
        }
      }
    }
  });

  it("still prioritizes work, startup and approval over a follow-up mixed with completion", () => {
    const followup = { ...agent("a", "needs_reply", 20, "parked"), last_message: "继续吗？" };
    const completed = agent("b", "needs_reply", 1, "new");
    for (const status of ["running", "starting", "needs_approval"] as const) {
      expect(taskStatus(t, [followup, completed, agent("c", status, 1000)], 1059)).toBe(status);
    }
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

  it("keeps a question after it is read and removes it once the user answers", () => {
    const question = { ...agent("a1", "needs_reply", 30, "new"), waiting_for: "选 A 还是 B？" };
    expect(waitingList(tasks, [question]).map(w => w.agent.name)).toEqual(["a1"]);
    const read = { ...question, mark: "parked" as const };
    expect(waitingList(tasks, [read]).map(w => w.agent.name)).toEqual(["a1"]);
    expect(taskNeedsYou(tasks[0], [read])).toBe(false);
    expect(waitingList(tasks, [{ ...read, status: "running" as const, waiting_for: null }])).toEqual([]);
    expect(waitingList(tasks, [{ ...read, last_message: "完成了。", waiting_for: "完成了。" }])).toEqual([]);
  });

  it("keeps read questions from any local agent while excluding read completions and sample questions", () => {
    for (const kind of ["codex", "claude", "kimi"]) {
      const question = { ...agent("a1", "needs_reply", 30, "parked"), kind, waiting_for: "请回答“要用哪份？”" };
      const completed = { ...agent("a2", "needs_reply", 1, "parked"), kind, last_message: "完成了。" };
      const example = { ...agent("b1", "needs_reply", 1, "parked"), kind, last_message: "可以这样问：\n“设备编号是多少？”" };
      expect(waitingList(tasks, [example, completed, question]).map(w => w.agent.name)).toEqual(["a1"]);
    }
  });

  it("shows a running Codex conversation's explicit questions once and removes each as it is answered", () => {
    const running = { ...agent("a1", "running", 30), kind: "codex", pending_questions: ["先做哪个版本？", "要合并吗？"] };
    expect(waitingList(tasks, [running]).map(w => w.agent.name)).toEqual(["a1"]);
    expect(waitingPresentation(running)).toEqual({ status: "asked", text: "先做哪个版本？\n要合并吗？" });
    expect(taskStatus(tasks[0], [running])).toBe("running");
    expect(taskNeedsYou(tasks[0], [running])).toBe(false);
    const partlyAnswered = { ...running, pending_questions: ["要合并吗？"] };
    expect(waitingPresentation(partlyAnswered)).toEqual({ status: "asked", text: "要合并吗？" });
    expect(waitingList(tasks, [partlyAnswered])).toHaveLength(1);
    const answered = { ...running, pending_questions: [] };
    expect(waitingList(tasks, [answered])).toEqual([]);
    expect(waitingPresentation(answered)).toEqual({ status: "running", text: "" });
  });

  it("keeps approval priority while also displaying the unanswered question", () => {
    const approval = { ...agent("a1", "needs_approval", 50), kind: "codex", waiting_for: "运行检查", pending_questions: ["选哪个分支？"] };
    const running = { ...agent("b1", "running", 1), kind: "codex", pending_questions: ["确认范围吗？"] };
    expect(waitingList(tasks, [running, approval]).map(w => w.agent.name)).toEqual(["a1", "b1"]);
    expect(waitingPresentation(approval)).toEqual({ status: "needs_approval", text: "运行检查\n选哪个分支？" });
  });

  it("ignores pending-question hints for exited or non-Codex conversations and supports older records", () => {
    const pending = { ...agent("a1", "running"), kind: "codex", pending_questions: ["确认吗？"] };
    for (const record of [{ ...pending, kind: "claude" }, { ...pending, exited: true }, { ...pending, status: "exited" as const }])
      expect(waitingList(tasks, [record])).toEqual([]);
    expect(waitingPresentation(agent("a1", "running"))).toEqual({ status: "running", text: "" });
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

describe("inDraggedOrder", () => {
  it("puts dragged conversations first in that order and keeps the rest in their given order", () => {
    const list = [agent("a", "running"), agent("b", "idle"), agent("c", "idle"), agent("d", "idle")];
    expect(inDraggedOrder(list, ["c", "gone", "a"]).map((x) => x.name)).toEqual(["c", "a", "b", "d"]);
    expect(inDraggedOrder(list, undefined).map((x) => x.name)).toEqual(["a", "b", "c", "d"]);
  });
});
