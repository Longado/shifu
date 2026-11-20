import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentView } from "./format";
import { taskGoalAssign, taskGoalSave } from "./api";
import { GOAL_TEMPLATE, goalRows, parseGoals, parseTaskGoal, percent, ProgressPage, projectProgress, todoProjects, wbRows, wbShown, wbWhen, type WbSession } from "./progress";

vi.mock("./api", () => ({ taskGoalAssign: vi.fn(), taskGoalSave: vi.fn(), taskGoalTell: vi.fn() }));

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
    expect(parseGoals("- [ ] x", "/w/01_项目/notes").title).toBe("notes");
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

describe("todoProjects", () => {
  const r = (project: string, title: string, status: string, due: string | null = null) => ({ project, title, status, due });

  it("counts done over all, lists open work doing-first then by due date, and sorts projects by nearest due", () => {
    const p = todoProjects([
      r("Alpha", "a", "done"),
      r("Alpha", "b", "todo", "2026-12-01"),
      r("Alpha", "c", "blocked"),
      r("Alpha", "d", "doing"),
      r("Alpha", "e", "todo", "2026-11-20"),
      r("Beta", "f", "todo", "2026-11-15"),
      r("Gamma", "g", "todo"),
    ]);
    expect(p.map((x) => [x.project, x.done, x.total, x.nextDue])).toEqual([
      ["Beta", 0, 1, "2026-11-15"],
      ["Alpha", 1, 5, "2026-11-20"],
      ["Gamma", 0, 1, null],
    ]);
    expect(p[1].rows.map((x) => [x.text, x.tag ?? null])).toEqual([
      ["d", "正在做"],
      ["c", "卡住"],
      ["e  11-20 截止", null],
      ["b  12-01 截止", null],
    ]);
  });

  it("keeps undated projects alphabetical and completed tasks out of the open rows", () => {
    const projects = todoProjects([
      r("Gamma", "Finished", "done", "2026-11-01"),
      r("Gamma", "Open", "todo"),
      r("Alpha", "Work", "doing"),
    ]);
    expect(projects.map((p) => [p.project, p.nextDue, p.done, p.total])).toEqual([
      ["Alpha", null, 0, 1],
      ["Gamma", null, 1, 2],
    ]);
    expect(projects[1].rows).toEqual([{ done: false, text: "Open" }]);
  });
});

/** The page's DOM boundary; the tests exercise its real card and refresh logic. */
class Element {
  className = "";
  textContent = "";
  title = "";
  hidden = false;
  innerHTML = "";
  style: Record<string, string> = {};
  children: (Element | string)[] = [];
  attributes = new Map<string, string>();
  listeners = new Map<string, () => void>();
  classList = { contains: (name: string) => this.className.split(" ").includes(name) };
  append(...children: (Element | string)[]): void { this.children.push(...children); }
  replaceChildren(...children: (Element | string)[]): void {
    const dom = document as unknown as { activeElement: Element | null };
    if (this.contains(dom.activeElement)) dom.activeElement = null;
    this.children = children;
  }
  contains(node: unknown): boolean {
    return node === this || this.children.some((c) => c instanceof Element && c.contains(node));
  }
  querySelectorAll(selector: string): Element[] { return this.find(selector.slice(1)); }
  focus(_options?: FocusOptions): void { (document as unknown as { activeElement: Element | null }).activeElement = this; }
  setAttribute(name: string, value: string): void { this.attributes.set(name, value); }
  addEventListener(name: string, handler: () => void): void { this.listeners.set(name, handler); }
  find(className: string): Element[] {
    return [
      ...(this.className.split(" ").includes(className) ? [this] : []),
      ...this.children.flatMap((c) => c instanceof Element ? c.find(className) : []),
    ];
  }
}

describe("progress cards", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("keeps the expanded heading focused across toggles, data and agent refreshes without taking outside focus", () => {
    const dom = { activeElement: null as Element | null, createElement: () => new Element() };
    vi.stubGlobal("document", dom);
    const page = new ProgressPage(() => {});
    const root = page.element as unknown as Element;
    root.hidden = false;
    const goals = {
      files: [{ dir: "/w/alpha", text: "# Alpha\n- [ ] First" }],
      agents: {},
      todos: [{ project: "Beta", title: "Next", status: "doing", due: "2026-12-01" }],
    };
    page.setGoals(goals);
    expect(root.find("goal-head").map((h) => h.attributes.get("aria-expanded"))).toEqual(["false", "false"]);
    expect(root.find("goal-items")).toHaveLength(0);

    root.find("goal-head")[1].focus();
    root.find("goal-head")[1].listeners.get("click")!();
    expect(root.find("goal-head").map((h) => h.attributes.get("aria-expanded"))).toEqual(["false", "true"]);
    expect(root.find("goal-text").map((n) => n.textContent)).toEqual(["Next  12-01 截止"]);
    expect(dom.activeElement).toBe(root.find("goal-head")[1]);

    page.setGoals({ ...goals, todos: [{ ...goals.todos[0], title: "Updated" }] });
    page.setAgents([]);
    expect(root.find("goal-head").map((h) => h.attributes.get("aria-expanded"))).toEqual(["false", "true"]);
    expect(root.find("goal-text").map((n) => n.textContent)).toEqual(["Updated  12-01 截止"]);
    expect(dom.activeElement).toBe(root.find("goal-head")[1]);
    root.find("goal-head")[1].listeners.get("click")!();
    expect(dom.activeElement).toBe(root.find("goal-head")[1]);
    const outside = new Element();
    outside.focus();
    page.setGoals(goals);
    expect(root.find("goal-items")).toHaveLength(0);
    expect(dom.activeElement).toBe(outside);
  });

  it("does not move focus to another card when the focused card disappears", () => {
    const dom = { activeElement: null as Element | null, createElement: () => new Element() };
    vi.stubGlobal("document", dom);
    const page = new ProgressPage(() => {});
    const root = page.element as unknown as Element;
    const goals = { files: [], agents: {}, todos: [
      { project: "Alpha", title: "First", status: "todo", due: null },
      { project: "Beta", title: "Next", status: "todo", due: null },
    ] };
    page.setGoals(goals);
    root.find("goal-head")[0].focus();
    page.setGoals({ ...goals, todos: [goals.todos[1]] });
    expect(root.find("goal-head")).toHaveLength(1);
    expect(dom.activeElement).toBeNull();
  });
});

describe("handing a checklist line to a conversation", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("lists the task's conversations under the line, asks for that line by position and text, and shows who has it", async () => {
    vi.stubGlobal("document", { activeElement: null, createElement: () => new Element() });
    const text = "# 终点\n拿到 offer\n\n# 清单\n- [x] 改简历\n- [ ] 投十家\n- [ ] 约内推 @codex-a\n";
    const task = { id: "t-1", project: "/w", name: "求职", auto_name: false, created_at: 0, agents: ["codex-a", "claude-b", "kimi-c"] };
    const page = new ProgressPage(() => {}, () => [task]);
    const root = page.element as unknown as Element;
    page.setAgents([agent("codex-a", { kind: "codex", title: "筛岗位" }), agent("claude-b", { title: "改简历" }), agent("kimi-c", { kind: "kimi" })]);
    page.setTaskGoals({ "t-1": { path: "/h/goals/t-1.md", text } });
    root.find("goal-head")[0].listeners.get("click")!();

    // The finished line offers nothing; the open ones offer the button, the third already naming its conversation.
    expect(root.find("goal-text").map((n) => n.textContent)).toEqual(["改简历", "投十家", "约内推"]);
    expect(root.find("goal-give").map((n) => n.textContent)).toEqual(["交给…", "筛岗位"]);
    expect(root.find("goal-pick")).toHaveLength(0);

    root.find("goal-give")[0].listeners.get("click")!();
    // Only conversations that can be sent a queued message are offered.
    const chips = root.find("goal-pick")[0].find("goal-person");
    expect(chips.map((c) => c.find("goal-person-name")[0].textContent)).toEqual(["改简历", "筛岗位"]);

    const saved = { path: "/h/goals/t-1.md", text: text.replace("投十家", "投十家 @claude-b") };
    vi.mocked(taskGoalAssign).mockResolvedValue(saved);
    chips[0].listeners.get("click")!();
    await Promise.resolve();
    await Promise.resolve();
    expect(taskGoalAssign).toHaveBeenCalledWith("t-1", 1, "投十家", "claude-b");
    expect(root.find("goal-pick")).toHaveLength(0);
    expect(root.find("goal-give").map((n) => n.textContent)).toEqual(["改简历", "筛岗位"]);
    expect(root.find("goal-told")[0].textContent).toBe("已把「投十家」交给 改简历，它空下来就会收到");
  });
});

describe("saving a task's file while its conversations edit it", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("keeps the typing, shows the file as it is now, and saves on top of that the second time", async () => {
    vi.stubGlobal("document", { activeElement: null, createElement: () => new Element() });
    vi.stubGlobal("queueMicrotask", () => {});
    const opened = "# 清单\n- [ ] 投十家\n";
    const ticked = "# 清单\n- [x] 投十家\n";
    const task = { id: "t-1", project: "/w", name: "求职", auto_name: false, created_at: 0, agents: [] };
    const page = new ProgressPage(() => {}, () => [task]);
    const root = page.element as unknown as Element;
    page.setTaskGoals({ "t-1": { path: "/h/goals/t-1.md", text: opened } });
    root.find("goal-head")[0].listeners.get("click")!();
    root.find("goal-action").find((b) => b.textContent === "编辑")!.listeners.get("click")!();

    const area = root.find("goal-editor-text")[0] as unknown as { value: string };
    const mine = "# 终点\n拿到 offer\n\n" + opened;
    area.value = mine;
    const submit = () => (root.find("goal-editor")[0].listeners.get("submit") as unknown as (e: { preventDefault(): void }) => void)({ preventDefault() {} });

    vi.mocked(taskGoalSave).mockResolvedValue({ saved: false, goal: { path: "/h/goals/t-1.md", text: ticked } });
    submit();
    await Promise.resolve();
    await Promise.resolve();
    expect(taskGoalSave).toHaveBeenLastCalledWith("t-1", opened, mine);
    // Still editing, typing intact, and the current file is shown beside it.
    const shown = root.find("goal-editor-text") as unknown as { value: string; readOnly?: boolean }[];
    expect(shown.map((a) => [a.value, a.readOnly === true])).toEqual([[mine, false], [ticked, true]]);
    expect(root.find("goal-conflict")[0].hidden).toBe(false);

    const merged = mine.replace("- [ ]", "- [x]");
    area.value = merged;
    vi.mocked(taskGoalSave).mockResolvedValue({ saved: true, goal: { path: "/h/goals/t-1.md", text: merged } });
    submit();
    await Promise.resolve();
    await Promise.resolve();
    expect(taskGoalSave).toHaveBeenLastCalledWith("t-1", ticked, merged);
    expect(root.find("goal-editor")).toHaveLength(0);
  });
});

describe("parseTaskGoal", () => {
  it("takes a trailing @name off a checklist line as the conversation it was given to", () => {
    const got = parseTaskGoal("# 清单\n- [ ] 投十家 @codex-Workspace-8\n- [x] 发给 a@b.com 的信\n- [ ] @只有名字\n");
    expect(got.items).toEqual([
      { done: false, text: "投十家", owner: "codex-Workspace-8" },
      { done: true, text: "发给 a@b.com 的信" },
      { done: false, text: "@只有名字" },
    ]);
  });

  it("reads 终点 and 重点 under their headings and the checklist anywhere", () => {
    const got = parseTaskGoal("# 终点\n拿到 3 个上海 offer\n选定一个\n\n# 重点\n今天投完帆软\n\n# 清单\n- [x] 改简历\n- [ ] 投十家\n");
    expect(got.end).toBe("拿到 3 个上海 offer 选定一个");
    expect(got.focus).toBe("今天投完帆软");
    expect(got.items).toEqual([{ done: true, text: "改简历" }, { done: false, text: "投十家" }]);
  });

  it("an empty template has no end, focus, items or notes", () => {
    expect(parseTaskGoal(GOAL_TEMPLATE)).toEqual({ end: "", focus: "", items: [], notes: [] });
    expect(GOAL_TEMPLATE).toContain("# 记录");
  });

  it("reads what the task's conversations left for each other under 记录, one note per line", () => {
    const got = parseTaskGoal("# 终点\n上线\n\n# 清单\n- [ ] 部署\n\n# 记录\n- 11-19 接口限流是每分钟 60 次，批量要分页\n* 11-20 **定了**：用 `staging` 库测\n\n11-21 没有列表符号的一行\n- [x] 勾选行算清单，不算记录\n");
    expect(got.notes).toEqual(["11-19 接口限流是每分钟 60 次，批量要分页", "11-20 定了：用 staging 库测", "11-21 没有列表符号的一行"]);
    expect(got.items).toEqual([{ done: false, text: "部署" }, { done: true, text: "勾选行算清单，不算记录" }]);
    expect(got.end).toBe("上线");
  });
});
