/**
 * Jev's verdict on a new reply (APP-SPEC-C1). Jev judges in the background whether Claude's finished turn waits for
 * the user and writes one line per turn to its `waiting-log.jsonl`. Shifu reads that log (never writes it) and holds
 * back the notification only when Jev says the turn was a report.
 */

export type Verdict = "report" | "notify";

/** Both sides only write whole seconds, and the two Stop hooks fire in the same second in either order. */
const SLACK_SECONDS = 2;

/** The turn's verdict: "report" (a record exists, it was judged, and Jev says nobody is waiting), "notify" (judged as
 *  waiting, or skipped by Jev's privacy screen), or null when no record for this turn is in the log yet. */
export function verdictOf(log: string, session: string, since: number): Verdict | null {
  for (const raw of log.split("\n")) {
    let line: { event?: string; session?: string; date?: string; skipped?: string; notified?: boolean };
    try {
      line = JSON.parse(raw);
    } catch {
      continue; // an empty or half-written line
    }
    if (line.event !== "stop" || line.session !== session || typeof line.date !== "string") continue;
    const at = Date.parse(line.date) / 1000;
    if (!(at >= since - SLACK_SECONDS)) continue;
    return line.skipped === undefined && line.notified === false ? "report" : "notify";
  }
  return null;
}

export interface Wait {
  readTail: () => Promise<string | null>;
  sleep: (ms: number) => Promise<void>;
  timeoutMs?: number;
  intervalMs?: number;
}

/** Waits up to 3 seconds for Jev's record of this turn. No log file, or no record in time: "notify", as before. */
export async function waitForVerdict(session: string, since: number, io: Wait): Promise<Verdict> {
  const { timeoutMs = 3000, intervalMs = 250 } = io;
  for (let waited = 0; ; waited += intervalMs) {
    const log = await io.readTail();
    if (log === null) return "notify";
    const verdict = verdictOf(log, session, since);
    if (verdict) return verdict;
    if (waited >= timeoutMs) return "notify";
    await io.sleep(intervalMs);
  }
}
