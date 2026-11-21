import type { AgentView } from "./format";

export interface WaitingSeenEntry {
  session: string | null;
  keys: string[];
}

export type WaitingSeen = Record<string, WaitingSeenEntry>;
export type WaitingAgent = AgentView & { waiting_state_seen?: boolean };
export const WAITING_SEEN_KEY = "shifu.waiting-seen";

// Persist an identifier for a state, without retaining its question or response text.
function textKey(text: string): string {
  let hash = 2166136261;
  for (let i = 0; i < text.length; i++) hash = Math.imul(hash ^ text.charCodeAt(i), 16777619);
  return (hash >>> 0).toString(36);
}

export function waitingKeys(agent: AgentView): { state: string | null; questions: string[] } {
  const state = agent.status === "needs_reply" || agent.status === "needs_approval"
    ? `${agent.status}:${agent.status_since}:${textKey(`${agent.waiting_for ?? ""}\n${agent.last_message ?? ""}`)}`
    : null;
  const questions = agent.kind === "codex" && !agent.exited && agent.status !== "exited"
    ? (agent.pending_questions ?? []).map((text, index) => `question:${agent.pending_question_ids?.[index] ?? textKey(text)}`)
    : [];
  return { state, questions };
}

export function recordWaitingOpened(seen: WaitingSeen, agent: AgentView): WaitingSeen {
  const { state, questions } = waitingKeys(agent);
  const keys = [...(state ? [state] : []), ...questions];
  if (keys.length === 0) return seen;
  const previous = seen[agent.name];
  const session = agent.session_id ?? null;
  const known = previous?.session === session ? previous.keys : [];
  if (keys.length === known.length && keys.every((key) => known.includes(key))) return seen;
  return { ...seen, [agent.name]: { session, keys } };
}

/** A waiting row can contain only the newly arrived questions from the same conversation. */
export function unseenWaitingAgent(agent: AgentView, seen: WaitingSeen): WaitingAgent | null {
  const entry = seen[agent.name];
  if (!entry || entry.session !== (agent.session_id ?? null)) return agent;
  const keys = waitingKeys(agent);
  const remaining = keys.questions.map((key, index) => ({ key, index })).filter(({ key }) => !entry.keys.includes(key));
  const stateUnseen = keys.state !== null && !entry.keys.includes(keys.state);
  if (!stateUnseen && remaining.length === 0) return null;
  if (remaining.length === keys.questions.length && stateUnseen) return agent;
  return {
    ...agent,
    waiting_state_seen: keys.state !== null && !stateUnseen,
    pending_questions: remaining.map(({ index }) => agent.pending_questions![index]),
    pending_question_ids: remaining.map(({ index }) => agent.pending_question_ids?.[index] ?? keys.questions[index]),
  };
}

/** Local preferences can be absent or come from an interrupted or older write. */
export function loadWaitingSeen(storage: Pick<Storage, "getItem">): WaitingSeen {
  try {
    const parsed: unknown = JSON.parse(storage.getItem(WAITING_SEEN_KEY) ?? "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const entries = Object.entries(parsed).filter(([, entry]) => {
      if (!entry || typeof entry !== "object") return false;
      const value = entry as Partial<WaitingSeenEntry>;
      return (value.session === null || typeof value.session === "string")
        && Array.isArray(value.keys) && value.keys.every((key) => typeof key === "string");
    });
    return Object.fromEntries(entries) as WaitingSeen;
  } catch {
    return {};
  }
}
