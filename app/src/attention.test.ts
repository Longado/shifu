import { describe, expect, it } from "vitest";
import { baselineSeen, markAgents, markOf, recordViewed } from "./attention";
import type { AgentView, Status } from "./format";

function agent(name: string, status: Status, since: number): AgentView {
  return { name, kind: "claude", cwd: "/w", dir_name: "w", branch: null, title: null, status, status_since: since, last_message: null, exited: false };
}

describe("baselineSeen", () => {
  it("records every existing agent at its current time, so none of them is new after an upgrade", () => {
    const agents = [agent("a", "needs_reply", 100), agent("b", "running", 50), agent("c", "needs_approval", 7)];
    const seen = baselineSeen(agents);
    expect(seen).toEqual({ a: 100, b: 50, c: 7 });
    expect(markAgents(agents, seen).map((a) => a.mark)).toEqual(["parked", undefined, undefined]);
  });
});

describe("markOf", () => {
  it("a reply is new when never seen or when it came after the seen time, parked otherwise", () => {
    expect(markOf(agent("a", "needs_reply", 100), {})).toBe("new");
    expect(markOf(agent("a", "needs_reply", 100), { a: 99 })).toBe("new");
    expect(markOf(agent("a", "needs_reply", 100), { a: 100 })).toBe("parked");
    expect(markOf(agent("a", "needs_reply", 100), { a: 500 })).toBe("parked");
  });
  it("only replies are marked", () => {
    expect(markOf(agent("a", "needs_approval", 100), {})).toBeUndefined();
    expect(markOf(agent("a", "running", 100), {})).toBeUndefined();
  });
});

describe("recordViewed", () => {
  const agents = [agent("a", "needs_reply", 100), agent("b", "needs_reply", 200)];
  it("records the active agent when the window is in front", () => {
    expect(recordViewed({}, agents, "a", true)).toEqual({ a: 100 });
    expect(recordViewed({ b: 1 }, agents, "a", true)).toEqual({ b: 1, a: 100 });
  });
  it("records nothing when the window is behind, nothing is active, or the agent is unknown", () => {
    const seen = {};
    expect(recordViewed(seen, agents, "a", false)).toBe(seen);
    expect(recordViewed(seen, agents, null, true)).toBe(seen);
    expect(recordViewed(seen, agents, "zzz", true)).toBe(seen);
  });
  it("returns the same object when it is already recorded, so nothing gets written", () => {
    const seen = { a: 100 };
    expect(recordViewed(seen, agents, "a", true)).toBe(seen);
  });
  it("a reply that arrives while the user watches is seen at once; one that arrives later is new again", () => {
    let seen = recordViewed({ a: 50 }, [agent("a", "needs_reply", 100)], "a", true);
    expect(markOf(agent("a", "needs_reply", 100), seen)).toBe("parked");
    seen = recordViewed(seen, [agent("a", "running", 150)], "a", false);
    expect(markOf(agent("a", "needs_reply", 300), seen)).toBe("new");
  });
});
