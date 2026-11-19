import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentView } from "./format";
import { GOAL_TEMPLATE, goalRows, parseGoals, parseTaskGoal, percent, ProgressPage, projectProgress, todoProjects, wbRows, wbShown, wbWhen, type WbSession } from "./progress";

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

describe("parseTaskGoal", () => {
  it("reads 终点 and 重点 under their headings and the checklist anywhere", () => {
    const got = parseTaskGoal("# 终点\n拿到 3 个上海 offer\n选定一个\n\n# 重点\n今天投完帆软\n\n# 清单\n- [x] 改简历\n- [ ] 投十家\n");
    expect(got.end).toBe("拿到 3 个上海 offer 选定一个");
    expect(got.focus).toBe("今天投完帆软");
    expect(got.items).toEqual([{ done: true, text: "改简历" }, { done: false, text: "投十家" }]);
  });

  it("an empty template has no end, focus or items", () => {
    expect(parseTaskGoal(GOAL_TEMPLATE)).toEqual({ end: "", focus: "", items: [] });
  });
});
