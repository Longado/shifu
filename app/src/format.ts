export type Status = "starting" | "idle" | "running" | "needs_approval" | "needs_reply" | "exited";

/** A reply in "等你回复" is either new (not looked at yet) or parked (looked at, the agent is just waiting). */
export type Mark = "new" | "parked";

/** What a row shows: the status, with "needs_reply" split by its mark, and a new reply that ends on a question
 *  split off as "asked". */
export type ViewKey = Exclude<Status, "needs_reply"> | Mark | "asked";

/** Mirrors `AgentView` in app/src-tauri/src/agents.rs; `mark` is put on by the store (see attention.ts). */
export interface AgentView {
  name: string;
  kind: string;
  cwd: string;
  dir_name: string;
  branch: string | null;
  /** Conversation topic from the terminal title, already cleaned; null when there is none. */
  title: string | null;
  /** The name the user gave it (right-click on its tab); shown in place of the topic. */
  label?: string | null;
  status: Status;
  status_since: number;
  last_message: string | null;
  /** Claude's session id from the hooks; null before the first one. */
  session_id?: string | null;
  exited: boolean;
  mark?: Mark;
}

const LABELS: Record<ViewKey, string> = {
  starting: "启动中",
  idle: "空闲",
  running: "运行中",
  needs_approval: "等你批准",
  new: "做完了",
  asked: "在问你",
  parked: "停着",
  exited: "已退出",
};

/** Lower number sits higher in the sidebar. */
const RANK: Record<ViewKey, number> = {
  needs_approval: 0,
  asked: 1,
  new: 1,
  running: 2,
  parked: 3,
  starting: 4,
  idle: 4,
  exited: 5,
};

/** An unmarked reply counts as new: it is shown rather than hidden. */
/** The last non-empty line ends on a question mark, after trailing Markdown emphasis or code marks. */
function endsOnQuestion(text: string | null): boolean {
  const last = (text ?? "").trimEnd().split("\n").pop() ?? "";
  return /[?？]$/.test(last.replace(/[*_`~\s)）」』"”]+$/, ""));
}

export function viewKey(agent: Pick<AgentView, "status" | "mark" | "last_message">): ViewKey {
  if (agent.status !== "needs_reply") return agent.status;
  if (agent.mark === "parked") return "parked";
  return endsOnQuestion(agent.last_message) ? "asked" : "new";
}

export const statusLabel = (key: ViewKey): string => LABELS[key];

export const viewRank = (agent: Pick<AgentView, "status" | "mark" | "last_message">): number => RANK[viewKey(agent)];

/** The name to show for an agent: the user's own name for it, else its topic, else the session name. */
export const agentLabel = (agent: Pick<AgentView, "label" | "title" | "name">): string =>
  agent.label || agent.title || agent.name;

export function ageText(secs: number): string {
  if (secs < 60) return "刚刚";
  if (secs < 3600) return `${Math.floor(secs / 60)} 分钟`;
  if (secs < 86400) return `${Math.floor(secs / 3600)} 小时`;
  return `${Math.floor(secs / 86400)} 天`;
}

export function statusLine(agent: AgentView, nowSecs: number): string {
  return `${statusLabel(viewKey(agent))} ${ageText(nowSecs - agent.status_since)}`;
}

export function sortAgents(agents: readonly AgentView[]): AgentView[] {
  return [...agents].sort(
    (a, b) => viewRank(a) - viewRank(b) || a.status_since - b.status_since || a.name.localeCompare(b.name),
  );
}

/** Needs the user now: waiting for an approval, or a reply not looked at yet. Parked replies do not. */
export function needsYou(agent: Pick<AgentView, "status" | "mark" | "last_message">): boolean {
  const key = viewKey(agent);
  return key === "needs_approval" || key === "new" || key === "asked";
}

/** The last three folders of a path, for places too narrow for the whole thing. */
export function shortPath(path: string, keep = 3): string {
  const parts = path.split("/").filter(Boolean);
  return parts.length <= keep ? path.replace(/\/$/, "") : `…/${parts.slice(-keep).join("/")}`;
}
