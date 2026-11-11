import { invoke } from "@tauri-apps/api/core";
import type { Hit } from "./search";
import type { Resources } from "./memory";
import type { AgentView } from "./format";
import type { Seen } from "./attention";
import type { Limits } from "./limits";
import type { Goals } from "./progress";
import type { AgentModels } from "./models";
import type { Changed, TaskFile } from "./tasks";

export const agentsList = () => invoke<AgentView[]>("agents_list");
export const agentNew = (kind: string, cwd: string) => invoke<string>("agent_new", { kind, cwd });
/** Ends the agent and takes it off its task. */
export const agentKill = (name: string) => invoke<TaskFile>("agent_kill", { name });
/** null before the first start of this version. */
/** Blank goes back to the conversation topic. */
export const agentRename = (name: string, label: string) => invoke<void>("agent_rename", { name, label });
export const seenLoad = () => invoke<Seen | null>("seen_load");
export const seenSave = (seen: Seen) => invoke<void>("seen_save", { seen });
export const setAttention = (count: number) => invoke<void>("set_attention", { count });
/** Shows a macOS notification; clicking it brings the window forward and answers with the `notify-jump` event. */
export const notifySend = (agent: string, title: string, body: string) =>
  invoke<void>("notify_send", { agent, title, body });
/** The end of Jev's waiting log (see jev.ts); null when the file does not exist. */
export const jevLogTail = () => invoke<string | null>("jev_log_tail");
export const limitsGet = () => invoke<Limits>("limits_get");
export const goalsGet = () => invoke<Goals>("goals_get");
export const resourcesGet = () => invoke<Resources>("resources_get");
export type PowerMode = "off" | "system" | "display";
export const powerGet = () => invoke<PowerMode>("power_get");
export const powerSet = (mode: PowerMode) => invoke<PowerMode>("power_set", { mode });
export const pmDir = () => invoke<string>("pm_dir");
export const pmOpen = (kind: string, pick: { model: string | null; effort: string | null }) =>
  invoke<Changed>("pm_open", { kind, ...pick });
export const searchConversations = (query: string) => invoke<Hit[]>("search_conversations", { query });
export const modelsList = () => invoke<AgentModels[]>("models_list");
export const tasksLoad = () => invoke<TaskFile>("tasks_load");
/** An empty `name` makes the backend call the task "任务 N". */
/** `model`/`effort` null = the CLI's own default. */
type PickArgs = { model: string | null; effort: string | null };
export const taskNew = (project: string, name: string, kind: string, pick: PickArgs) =>
  invoke<Changed>("task_new", { project, name, kind, ...pick });
export const taskAddAgent = (id: string, kind: string, pick: PickArgs) => invoke<Changed>("task_add_agent", { id, kind, ...pick });
export const taskRename = (id: string, name: string) => invoke<TaskFile>("task_rename", { id, name });
export const taskSetCurrent = (id: string) => invoke<TaskFile>("task_set_current", { id });
/** Drag an agent's tab onto another task: only its ownership changes, the agent keeps running. */
export const taskMoveAgent = (name: string, toId: string) => invoke<TaskFile>("task_move_agent", { name, toId });
export const taskDelete = (id: string) => invoke<TaskFile>("task_delete", { id });
export const appName = () => invoke<string>("app_name");
export const pathReveal = (path: string) => invoke<void>("path_reveal", { path });
export const pathsResolve = (paths: string[], cwd: string) =>
  invoke<({ path: string; is_dir: boolean } | null)[]>("paths_resolve", { paths, cwd });
export const pathOpen = (target: string) => invoke<void>("path_open", { target });
export const fileRead = (path: string) => invoke<ArrayBuffer>("file_read", { path });
/** Raw bytes in the body; the name in a header, percent-encoded because headers are ASCII. */
export const dropSave = (name: string, bytes: ArrayBuffer) =>
  invoke<string>("drop_save", bytes, { headers: { "x-name": encodeURIComponent(name) } });
export const agentResume = (name: string) => invoke<void>("agent_resume", { name });
export const agentReveal = (name: string) => invoke<void>("agent_reveal", { name });
export const recentDirs = () => invoke<string[]>("recent_dirs");
export const termAttach = (name: string, cols: number, rows: number) =>
  invoke<number>("term_attach", { name, cols, rows });
export const ptyWrite = (id: number, data: string) => invoke<void>("pty_write", { id, data });
export const ptyResize = (id: number, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows });
export const ptyKill = (id: number) => invoke<void>("pty_kill", { id });
export const layoutSave = (taskId: string, json: string) => invoke<void>("layout_save", { taskId, json });
export const layoutLoad = (taskId: string) => invoke<string | null>("layout_load", { taskId });
export const clipboardWrite = (text: string) => invoke<void>("clipboard_write", { text });
export const termShiftEnter = (name: string) => invoke<void>("term_shift_enter", { name });
export const termLeaveScroll = (name: string) => invoke<void>("term_leave_scroll", { name });

/** Drag payload type for a sidebar row; the value is the agent name. */
export const AGENT_MIME = "application/x-shifu-agent";
