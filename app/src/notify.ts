/**
 * Background notifications (APP-SPEC-B4). An agent that becomes "waiting for approval" or "new reply" gets one
 * macOS notification, and only when the window is not in front. `notified` holds `<name>|<status_since>` of
 * every such state already dealt with, so the same change never notifies twice.
 */
import { type AgentView, agentLabel, needsYou } from "./format";

export interface Notice {
  /** Agent name, handed back to the page when the notification is clicked. */
  agent: string;
  title: string;
  body: string;
  /** Set for a new reply of a Claude agent whose session is known: Jev's verdict is asked before it is sent
   *  (APP-SPEC-C1). `since` is the reply's `status_since`. Approvals, Codex and unknown sessions never wait. */
  jev?: { session: string; since: number };
}

export type Notified = ReadonlySet<string>;

const BODY_MAX = 80;

const keyOf = (a: Pick<AgentView, "name" | "status_since">) => `${a.name}|${a.status_since}`;

/** Approval states say so; a reply shows its last sentence, cut short; no text at all reads "答完了". */
export function bodyOf(agent: Pick<AgentView, "status" | "last_message">): string {
  if (agent.status === "needs_approval") return "等你批准";
  const text = (agent.last_message ?? "").replace(/\s+/g, " ").trim();
  if (!text) return "答完了";
  const chars = [...text];
  return chars.length <= BODY_MAX ? text : `${chars.slice(0, BODY_MAX).join("")}…`;
}

function noticeOf(a: AgentView): Notice {
  const notice: Notice = { agent: a.name, title: agentLabel(a), body: bodyOf(a) };
  const askJev = a.kind === "claude" && a.status === "needs_reply" && a.session_id;
  return askJev ? { ...notice, jev: { session: a.session_id as string, since: a.status_since } } : notice;
}

/** Every agent that needs the user right now (the start-up list: nothing is announced for those). */
export function currentlyNeeding(agents: readonly AgentView[]): Notified {
  return new Set(agents.filter(needsYou).map(keyOf));
}

/** What to send for this agent list. A change that arrives while the window is in front is recorded without a
 *  notice: the sidebar already shows it. `agents` must carry the new/parked marks. */
export function nextNotices(
  notified: Notified,
  agents: readonly AgentView[],
  windowFocused: boolean,
): { notices: Notice[]; notified: Notified } {
  const fresh = agents.filter((a) => needsYou(a) && !notified.has(keyOf(a)));
  if (fresh.length === 0) return { notices: [], notified };
  const next = new Set([...notified, ...fresh.map(keyOf)]);
  const notices = windowFocused ? [] : fresh.map(noticeOf);
  return { notices, notified: next };
}
