export type Status = "starting" | "idle" | "running" | "needs_approval" | "needs_reply" | "exited";

/** A reply in "等你回复" is either new (not looked at yet) or parked (looked at, the agent is just waiting). */
export type Mark = "new" | "parked";

/** What a row shows: the status, with "needs_reply" split by its mark, and a new reply that ends on a question
 *  split off as "asked". */
export type ViewKey = Exclude<Status, "needs_reply"> | Mark | "asked" | "unready";

export const STARTUP_CHECK_SECS = 60;

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
  /** One line on what it waits for: the question asked, the command to approve, or the reply's closing line.
   *  Absent in records an older `shifu` wrote. */
  waiting_for?: string | null;
  /** Claude's session id from the hooks; null before the first one. */
  session_id?: string | null;
  exited: boolean;
  progress?: {
    run_id: string;
    batch: string | null;
    completed: number | null;
    target: number;
    last_progress_at: number;
    deadline: string;
    phase: "observing" | "met" | "stalled" | "invalid" | "stopped";
    error: string | null;
  } | null;
  progress_error?: string | null;
  mark?: Mark;
}

const LABELS: Record<ViewKey, string> = {
  starting: "启动中",
  unready: "启动待检查",
  idle: "空闲",
  running: "运行中",
  needs_approval: "等你批准",
  new: "做完了",
  asked: "待回复",
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
  unready: 4,
  idle: 4,
  exited: 5,
};

/** An unmarked reply counts as new: it is shown rather than hidden. */
/** The last non-empty line ends on a question mark, after trailing Markdown emphasis or code marks. */
function endsOnQuestion(text: string | null): boolean {
  const last = (text ?? "").trimEnd().split("\n").pop() ?? "";
  return /[?？]$/.test(last.replace(/[*_`~\s)）」』"”]+$/, ""));
}

type Waits = Pick<AgentView, "status" | "mark" | "last_message" | "waiting_for">;

/** `last_message` keeps only the start of a long reply, so the closing line is asked first when it is known. */
const asks = (agent: Pick<AgentView, "last_message" | "waiting_for">): boolean =>
  endsOnQuestion(agent.waiting_for || agent.last_message);

export function viewKey(agent: Waits): ViewKey {
  if (agent.status !== "needs_reply") return agent.status;
  if (agent.mark === "parked") return "parked";
  return asks(agent) ? "asked" : "new";
}

/** `shifu hook` keeps this many characters of a reply in `last_message` (LAST_MESSAGE_CHARS in src/hook.rs). */
const SAVED_REPLY_CHARS = 300;

/** A line that points at something instead of saying something: a Codex directive, a lone link, URL or path, or
 *  the label above such a list. Mirrors `points_elsewhere` in src/hook.rs, for records written before it. */
const pointsElsewhere = (line: string): boolean =>
  /^:+[A-Za-z][^{]*\{.*\}$/.test(line) || /^\[[^\]]*\]\([^)]*\)$/.test(line) || /^(https?:\/\/|\/|~\/)\S*$/.test(line) || /[:：]$/.test(line);

/** What a waiting agent wants, in one line; empty when it does not wait or nothing says. */
export function waitingFor(agent: Pick<AgentView, "status" | "last_message" | "waiting_for">): string {
  if (agent.status !== "needs_approval" && agent.status !== "needs_reply") return "";
  if (agent.status === "needs_approval") return agent.waiting_for ?? "";
  if (agent.waiting_for && !pointsElsewhere(agent.waiting_for)) return agent.waiting_for;
  // A record from an older `shifu`: the saved start of a reply ends where the reply does only when it was short.
  const reply = agent.last_message ?? "";
  if ([...reply].length >= SAVED_REPLY_CHARS) return "";
  const lines = reply.split("\n").map((l) => l.replace(/\*\*|__|`/g, "").replace(/^[\s>#*+-]+/, "").trim()).filter((l) => l && !pointsElsewhere(l));
  return lines[lines.length - 1] ?? "";
}

/** Reading a follow-up is not answering it. Keep its icon until the reported status changes.
 *  A missing readiness callback is a reason to inspect the terminal, not proof of idle or an approval. */
export function displayKey(agent: AgentView, nowSecs = Math.floor(Date.now() / 1000)): ViewKey {
  if (agent.status === "needs_reply" && asks(agent)) return "asked";
  return agent.status === "starting" && nowSecs - agent.status_since >= STARTUP_CHECK_SECS
    ? "unready" : viewKey(agent);
}

export const statusLabel = (key: ViewKey): string => LABELS[key];

export const statusHint = (key: ViewKey): string =>
  key === "unready" ? "尚未收到就绪状态，打开终端查看提示。"
    : key === "asked" ? "打开会话回复追问。" : "";

export const viewRank = (agent: Waits): number => RANK[viewKey(agent)];

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
  const key = displayKey(agent, nowSecs);
  const age = ageText(nowSecs - agent.status_since);
  return key === "asked" ? age : `${statusLabel(key)} ${age}`;
}

export function sortAgents(agents: readonly AgentView[]): AgentView[] {
  return [...agents].sort(
    (a, b) => viewRank(a) - viewRank(b) || a.status_since - b.status_since || a.name.localeCompare(b.name),
  );
}

/** Needs the user now: waiting for an approval, or a reply not looked at yet. Parked replies do not. */
export function needsYou(agent: Waits): boolean {
  const key = viewKey(agent);
  return key === "needs_approval" || key === "new" || key === "asked";
}

/** The last three folders of a path, for places too narrow for the whole thing. */
export function shortPath(path: string, keep = 3): string {
  const parts = path.split("/").filter(Boolean);
  return parts.length <= keep ? path.replace(/\/$/, "") : `…/${parts.slice(-keep).join("/")}`;
}
