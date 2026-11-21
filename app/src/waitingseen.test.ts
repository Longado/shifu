import { describe, expect, it } from "vitest";
import type { AgentView } from "./format";
import { waitingList, waitingPresentation, type Task } from "./tasks";
import { loadWaitingSeen, recordWaitingOpened, unseenWaitingAgent, WAITING_SEEN_KEY } from "./waitingseen";

const agent = (name: string, overrides: Partial<AgentView> = {}): AgentView => ({
  name, kind: "codex", cwd: "/w", dir_name: "w", branch: null, title: null,
  status: "running", status_since: 1, last_message: null, exited: false, session_id: "s1", ...overrides,
});
const task: Task = { id: "t", project: "/w", name: "T", auto_name: false, created_at: 1, agents: ["a", "b"] };

describe("opening a waiting conversation", () => {
  it("removes only the opened state and keeps its source question and other conversation", () => {
    const a = agent("a", { status: "needs_reply", mark: "parked", last_message: "继续吗？" });
    const b = agent("b", { status: "needs_approval", waiting_for: "允许执行吗？" });
    const seen = recordWaitingOpened({}, a);
    expect(waitingList([task], [a, b], seen).map((row) => row.agent.name)).toEqual(["b"]);
    expect(a.last_message).toBe("继续吗？");
    expect(a.status).toBe("needs_reply");
    expect(waitingList([task], [a, b], seen).map((row) => row.agent.name)).toEqual(["b"]);
    expect(waitingList([task], [{ ...a, status_since: 2 }, b], seen).map((row) => row.agent.name)).toEqual(["b", "a"]);
  });

  it("keeps opened async questions hidden through partial answers and shows a same-text new call", () => {
    const a = agent("a", { pending_questions: ["问题？", "另一问？"], pending_question_ids: ["call-a:0", "call-a:1"] });
    const seen = recordWaitingOpened({}, a);
    const partial = { ...a, pending_questions: ["另一问？"], pending_question_ids: ["call-a:1"] };
    expect(unseenWaitingAgent(partial, seen)).toBeNull();
    const fresh = { ...partial, pending_questions: ["另一问？", "问题？"], pending_question_ids: ["call-a:1", "call-b:0"] };
    const row = waitingList([task], [fresh], seen)[0];
    expect(row.agent.pending_questions).toEqual(["问题？"]);
    expect(row.agent.pending_question_ids).toEqual(["call-b:0"]);
  });

  it("persists identifiers across reload and separates a new session with the same name", () => {
    const a = agent("a", { pending_questions: ["private question text"], pending_question_ids: ["call-a:0"] });
    const stored = JSON.stringify(recordWaitingOpened({}, a));
    expect(stored).not.toContain("private question text");
    const recovered = loadWaitingSeen({ getItem: (key) => key === WAITING_SEEN_KEY ? stored : null });
    expect(unseenWaitingAgent(a, recovered)).toBeNull();
    expect(unseenWaitingAgent({ ...a, session_id: "s2" }, recovered)).not.toBeNull();
  });

  it("dismisses approvals and unread completions without changing attention marks", () => {
    for (const status of ["needs_approval", "needs_reply"] as const) {
      const a = agent("a", { status, mark: "new", last_message: "已完成。" });
      expect(waitingList([task], [a], recordWaitingOpened({}, a))).toEqual([]);
      expect(a.mark).toBe("new");
    }
  });

  it("ignores malformed stored preferences", () => {
    expect(loadWaitingSeen({ getItem: () => "{" })).toEqual({});
    expect(loadWaitingSeen({ getItem: () => JSON.stringify({ a: { session: 1, keys: [] }, b: { session: "s", keys: [2] } }) })).toEqual({});
  });
});

describe("waiting store openings", () => {
  it("dismisses only on explicit opening and shows fresh questions on the active panel", async () => {
    const store = await import("./store");
    const a = agent("a", { pending_questions: ["问题？"], pending_question_ids: ["call-a:0"] });
    const b = agent("b", { pending_questions: ["别的问题？"], pending_question_ids: ["call-b:0"] });
    const task: Task = { id: "t", project: "/w", name: "Task", auto_name: false, created_at: 1, agents: ["a", "b"] };
    store.initSeen({});
    store.initWaitingOpened({});
    store.setTaskFile({ tasks: [task], current: "t" });
    store.setAgents([a, b]);
    store.setActiveAgent("a");
    store.setWindowFocused(true);
    store.setAgents([a, b]);
    expect(store.getWaitingList().map((row) => row.agent.name)).toEqual(["a", "b"]);
    let persisted = {};
    store.onWaitingOpenedChange((next) => { persisted = next; });
    store.dismissWaitingFor("missing");
    expect(store.getWaitingList()).toHaveLength(2);
    store.dismissWaitingFor("a");
    expect(store.getWaitingList().map((row) => row.agent.name)).toEqual(["b"]);
    store.setCurrent(null);
    store.setCurrent("t");
    store.setAgents([a, b]);
    expect(store.getWaitingList().map((row) => row.agent.name)).toEqual(["b"]);
    store.initWaitingOpened(loadWaitingSeen({ getItem: () => JSON.stringify(persisted) }));
    store.setAgents([a, b]);
    expect(store.getWaitingList().map((row) => row.agent.name)).toEqual(["b"]);
    store.setAgents([{ ...a, pending_question_ids: ["new-call:0"] }, b]);
    expect(store.getWaitingList().map((row) => row.agent.name)).toEqual(["a", "b"]);
  });
});


describe("previously opened states", () => {
  it("shows a new question without reviving a previously opened approval", () => {
    const approved = agent("a", { status: "needs_approval", waiting_for: "旧批准" });
    const seen = recordWaitingOpened({}, approved);
    const newQuestion = { ...approved, pending_questions: ["新问题？", "另一问？"], pending_question_ids: ["call-new:0", "call-new:1"] };
    const task: Task = { id: "t", project: "/w", name: "Task", auto_name: false, created_at: 1, agents: ["a", "b"] };
    const otherApproval = agent("b", { status: "needs_approval", status_since: 20 });
    const rows = waitingList([task], [newQuestion, otherApproval], seen);
    expect(rows.map((row) => row.agent.name)).toEqual(["b", "a"]);
    expect(rows[1].agent.status).toBe("needs_approval");
    expect(waitingPresentation(rows[1].agent)).toEqual({ status: "asked", text: "新问题？\n另一问？" });
    const partial = { ...newQuestion, pending_questions: ["另一问？"], pending_question_ids: ["call-new:1"] };
    const opened = recordWaitingOpened(seen, newQuestion);
    expect(waitingList([task], [partial], opened)).toEqual([]);
    expect(waitingList([task], [{ ...partial, pending_questions: [], pending_question_ids: [] }], opened)).toEqual([]);
  });

  it("keeps only identities present at the latest explicit opening", () => {
    let seen = {};
    for (let status_since = 1; status_since <= 100; status_since++) {
      const current = agent("a", { status: "needs_reply", status_since, last_message: "完成" });
      seen = recordWaitingOpened(seen, current);
    }
    expect(Object.values(seen)[0]).toMatchObject({ keys: [expect.any(String)] });
    const before = agent("a", { pending_questions: ["第一问", "第二问"], pending_question_ids: ["q:0", "q:1"] });
    const opened = recordWaitingOpened({}, before);
    const partial = { ...before, pending_questions: ["第二问"], pending_question_ids: ["q:1"] };
    const refreshed = recordWaitingOpened(opened, partial);
    expect(refreshed.a.keys).toEqual(["question:q:1"]);
    expect(unseenWaitingAgent(partial, refreshed)).toBeNull();
  });
});
