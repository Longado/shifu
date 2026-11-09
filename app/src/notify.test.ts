import { describe, expect, it } from "vitest";
import type { AgentView, Mark, Status } from "./format";
import { bodyOf, currentlyNeeding, nextNotices } from "./notify";

function agent(name: string, status: Status, since: number, extra: Partial<AgentView> = {}): AgentView {
  return { name, kind: "claude", cwd: "/w", dir_name: "w", branch: null, title: null, status, status_since: since, last_message: null, exited: false, ...extra };
}
const withMark = (a: AgentView, mark: Mark): AgentView => ({ ...a, mark });

describe("nextNotices", () => {
  it("announces a change once; the same state in later lists, and in the start-up list, stays quiet", () => {
    const start = [agent("old", "needs_reply", 1, { mark: "new" })];
    let notified = currentlyNeeding(start);
    expect(nextNotices(notified, start, false).notices).toEqual([]);

    const later = [...start, agent("a", "needs_approval", 5, { title: "话题 A" })];
    const first = nextNotices(notified, later, false);
    expect(first.notices).toEqual([{ agent: "a", title: "话题 A", body: "等你批准" }]);
    notified = first.notified;
    expect(nextNotices(notified, later, false).notices).toEqual([]);

    // the same agent moving on to a new state is a new change
    const again = [...start, agent("a", "needs_reply", 9, { last_message: "好了", mark: "new" })];
    expect(nextNotices(notified, again, false).notices.map((n) => [n.title, n.body])).toEqual([["a", "好了"]]);
  });

  it("window in front: nothing is sent, and the change is not announced later either", () => {
    const list = [agent("a", "needs_approval", 5)];
    const shown = nextNotices(new Set(), list, true);
    expect(shown.notices).toEqual([]);
    expect(nextNotices(shown.notified, list, false).notices).toEqual([]);
  });

  it("only a new Claude reply with a known session asks Jev first", () => {
    const list = [
      agent("reply", "needs_reply", 7, { session_id: "s-reply", mark: "new" }),
      agent("approve", "needs_approval", 8, { session_id: "s-approve" }),
      agent("nosession", "needs_reply", 9, { mark: "new" }),
      agent("codex", "needs_reply", 10, { kind: "codex", session_id: "s-codex", mark: "new" }),
    ];
    const jev = Object.fromEntries(nextNotices(new Set(), list, false).notices.map((n) => [n.agent, n.jev]));
    expect(jev).toEqual({ reply: { session: "s-reply", since: 7 }, approve: undefined, nosession: undefined, codex: undefined });
  });

  it("parked, running and exited agents are not announced", () => {
    const list = [withMark(agent("p", "needs_reply", 3), "parked"), agent("r", "running", 4), agent("x", "exited", 5)];
    expect(nextNotices(new Set(), list, false).notices).toEqual([]);
  });
});

describe("bodyOf", () => {
  it("shows the last reply cut to 80 characters, or 答完了 when there is none", () => {
    expect(bodyOf(agent("a", "needs_reply", 1, { last_message: "  第一行\n第二行  " }))).toBe("第一行 第二行");
    expect(bodyOf(agent("a", "needs_reply", 1, { last_message: "字".repeat(100) }))).toBe(`${"字".repeat(80)}…`);
    expect(bodyOf(agent("a", "needs_reply", 1, { last_message: "  " }))).toBe("答完了");
    expect(bodyOf(agent("a", "needs_reply", 1))).toBe("答完了");
  });
});
