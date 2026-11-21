import { listen } from "@tauri-apps/api/event";

type PtyData = { id: number; data: string };
type PtyExit = { id: number; code: number | null };
export interface PtyHandler {
  data(data: string): void;
  exit(code: number | null): void;
}

type PtyEvent = { data: string } | { exit: number | null };
// A detached client can still emit output before the native pump's final exit event.
const handlers = new Map<number, PtyHandler | null>();
const early = new Map<number, PtyEvent[]>();
let pendingAttaches = 0;

/** The native pump can emit before term_attach returns its id. Release unused events when all requests settle. */
export function beginPtyAttach(): () => void {
  pendingAttaches++;
  return () => { if (--pendingAttaches === 0) early.clear(); };
}

function deliver(id: number, item: PtyEvent): void {
  const h = handlers.get(id);
  if (h === undefined) {
    if (!pendingAttaches) return;
    const queued = early.get(id);
    if (queued) queued.push(item);
    else early.set(id, [item]);
  } else if ("data" in item) {
    h?.data(item.data);
  } else {
    handlers.delete(id);
    h?.exit(item.exit);
  }
}

export async function startPtyBus(): Promise<void> {
  await listen<PtyData>("pty-data", ({ payload }) => deliver(payload.id, { data: payload.data }));
  await listen<PtyExit>("pty-exit", ({ payload }) => deliver(payload.id, { exit: payload.code }));
}

export function register(id: number, h: PtyHandler): void {
  handlers.set(id, h);
  const queued = early.get(id);
  early.delete(id);
  if (queued) for (const item of queued) deliver(id, item);
}

/** Called when a panel disposes its client. An exit already buffered before the attach reply is final. */
export function unregister(id: number): void {
  const last = early.get(id)?.at(-1);
  if (last && "exit" in last) handlers.delete(id);
  else handlers.set(id, null);
  early.delete(id);
}
