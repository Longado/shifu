import { listen } from "@tauri-apps/api/event";

type PtyData = { id: number; data: string };
type PtyExit = { id: number; code: number | null };
export interface PtyHandler {
  data(data: string): void;
  exit(code: number | null): void;
}

const handlers = new Map<number, PtyHandler>();
// Events can arrive before term_attach resolves with the id; hold them until the handler registers.
const early = new Map<number, Array<{ data: string } | { exit: number | null }>>();

function deliver(id: number, item: { data: string } | { exit: number | null }): void {
  const h = handlers.get(id);
  if (!h) {
    early.set(id, [...(early.get(id) ?? []), item]);
  } else if ("data" in item) {
    h.data(item.data);
  } else {
    h.exit(item.exit);
  }
}

export async function startPtyBus(): Promise<void> {
  await listen<PtyData>("pty-data", ({ payload }) => deliver(payload.id, { data: payload.data }));
  await listen<PtyExit>("pty-exit", ({ payload }) => deliver(payload.id, { exit: payload.code }));
}

export function register(id: number, h: PtyHandler): void {
  handlers.set(id, h);
  for (const item of early.get(id) ?? []) deliver(id, item);
  early.delete(id);
}

export function unregister(id: number): void {
  handlers.delete(id);
  early.delete(id);
}
