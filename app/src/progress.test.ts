import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentView } from "./format";
import { goalRows, parseGoals, percent, ProgressPage, projectProgress, todoProjects, wbRows, wbShown, wbWhen, type WbSession } from "./progress";

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
  append(...children: (Element | string)[]): void { this.children.push(...children); }
  replaceChildren(...children: (Element | string)[]): void { this.children = children; }
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

  it("starts collapsed and keeps only opened cards expanded across data and agent refreshes", () => {
    vi.stubGlobal("document", { createElement: () => new Element() });
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

    root.find("goal-head")[1].listeners.get("click")!();
    expect(root.find("goal-head").map((h) => h.attributes.get("aria-expanded"))).toEqual(["false", "true"]);
    expect(root.find("goal-text").map((n) => n.textContent)).toEqual(["Next  12-01 截止"]);

    page.setGoals({ ...goals, todos: [{ ...goals.todos[0], title: "Updated" }] });
    page.setAgents([]);
    expect(root.find("goal-head").map((h) => h.attributes.get("aria-expanded"))).toEqual(["false", "true"]);
    expect(root.find("goal-text").map((n) => n.textContent)).toEqual(["Updated  12-01 截止"]);
    root.find("goal-head")[1].listeners.get("click")!();
    page.setGoals(goals);
    expect(root.find("goal-items")).toHaveLength(0);
  });
});
