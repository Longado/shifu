import { markAgents, recordViewed, type Seen } from "./attention";
import { type AgentView, needsYou } from "./format";
import type { Task, TaskFile } from "./tasks";

type Listener = () => void;

let rawAgents: readonly AgentView[] = [];
/** The agents with their new/parked marks; every reader gets these. */
let agents: readonly AgentView[] = [];
let tasks: readonly Task[] = [];
let currentId: string | null = null;
let seen: Seen = {};
let activeAgent: string | null = null;
let windowFocused = true;
let lastAttention = -1;
const listeners = new Set<Listener>();
const seenListeners = new Set<(seen: Seen) => void>();
const attentionListeners = new Set<(count: number) => void>();

const emit = () => listeners.forEach((l) => l());

export const getAgents = () => agents;
export const getTasks = () => tasks;
export const getCurrentId = () => currentId;
export const getCurrentTask = () => tasks.find((t) => t.id === currentId);
export const findAgent = (name: string) => agents.find((a) => a.name === name);

/** Marks the agents, first recording the one in front of the user as seen, then tells whoever listens. */
function refresh(): void {
  const next = recordViewed(seen, rawAgents, activeAgent, windowFocused);
  if (next !== seen) {
    seen = next;
    seenListeners.forEach((l) => l(seen));
  }
  agents = markAgents(rawAgents, seen);
  const attention = agents.filter(needsYou).length;
  if (attention !== lastAttention) {
    lastAttention = attention;
    attentionListeners.forEach((l) => l(attention));
  }
  emit();
}

/** Must come before the first `setAgents`: what the user had seen when the app last ran. */
export function initSeen(initial: Seen): void {
  seen = initial;
}

export function setAgents(next: readonly AgentView[]): void {
  rawAgents = next;
  refresh();
}

/** The panel in front (null when there is none). */
export function setActiveAgent(name: string | null): void {
  activeAgent = name;
  refresh();
}

export function setWindowFocused(focused: boolean): void {
  windowFocused = focused;
  refresh();
}

/** The task list from the backend. The current task is only changed by `setCurrent`, so a reply that was
 *  overtaken by a click cannot flip the sidebar back. */
export function setTaskFile(file: TaskFile): void {
  tasks = file.tasks;
  if (currentId !== null && !tasks.some((t) => t.id === currentId)) currentId = null;
  emit();
}

export function setCurrent(id: string | null): void {
  currentId = id;
  emit();
}

export function subscribe(l: Listener): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** Called with the whole map whenever a reply was seen; the owner stores it. */
export const onSeenChange = (l: (seen: Seen) => void) => seenListeners.add(l);
/** Called when the number of agents that need the user changes (and once at the first list). */
export const onAttentionChange = (l: (count: number) => void) => attentionListeners.add(l);

/** Errors from backend commands show in the sidebar footer. */
const errorListeners = new Set<(message: string) => void>();
export const onError = (l: (message: string) => void) => errorListeners.add(l);
export function reportError(e: unknown): void {
  const message = e instanceof Error ? e.message : String(e);
  for (const l of errorListeners) l(message);
}
