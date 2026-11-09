/**
 * New replies versus parked ones (APP-SPEC-B2). `seen` maps an agent name to the `status_since` it had when the
 * user last had it in front of them. A reply is new while its `status_since` is newer than that.
 */
import type { AgentView, Mark } from "./format";

export type Seen = Record<string, number>;

/** First start after the upgrade: everything that exists now counts as seen, or twenty agents would all be new. */
export function baselineSeen(agents: readonly AgentView[]): Seen {
  return Object.fromEntries(agents.map((a) => [a.name, a.status_since]));
}

export function markOf(agent: Pick<AgentView, "name" | "status" | "status_since">, seen: Seen): Mark | undefined {
  if (agent.status !== "needs_reply") return undefined;
  const at = seen[agent.name];
  return at !== undefined && at >= agent.status_since ? "parked" : "new";
}

export function markAgents(agents: readonly AgentView[], seen: Seen): AgentView[] {
  return agents.map((a) => ({ ...a, mark: markOf(a, seen) }));
}

/** The agent in front of the user (active panel, window in the foreground) is seen at whatever state it is in
 *  now, so a reply that arrives while they watch is never new. Returns `seen` itself when nothing changes. */
export function recordViewed(seen: Seen, agents: readonly AgentView[], active: string | null, focused: boolean): Seen {
  if (!focused || active === null) return seen;
  const agent = agents.find((a) => a.name === active);
  if (!agent || (seen[active] ?? -1) >= agent.status_since) return seen;
  return { ...seen, [active]: agent.status_since };
}
