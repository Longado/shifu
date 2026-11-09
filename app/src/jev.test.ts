import { describe, expect, it } from "vitest";
import { verdictOf, waitForVerdict } from "./jev";

const SINCE = Date.parse("2026-11-07T04:25:53Z") / 1000;
const stop = (extra: Record<string, unknown>, date = "2026-11-07T04:25:53Z", session = "s1") =>
  JSON.stringify({ date, event: "stop", session, entrypoint: "cli", host: "com.stablyai.orca", ...extra });

describe("verdictOf", () => {
  it("a judged turn Jev does not count as waiting is a report", () => {
    expect(verdictOf(stop({ notified: false, probability: 0.2 }), "s1", SINCE)).toBe("report");
  });

  it("judged as waiting, or skipped by the privacy screen, still notifies", () => {
    expect(verdictOf(stop({ notified: true, probability: 0.94 }), "s1", SINCE)).toBe("notify");
    expect(verdictOf(stop({ notified: false, skipped: "sensitive" }), "s1", SINCE)).toBe("notify");
    expect(verdictOf(stop({ notified: false, skipped: "failed" }), "s1", SINCE)).toBe("notify");
  });

  it("no record of this session and turn is no verdict yet", () => {
    expect(verdictOf("", "s1", SINCE)).toBeNull();
    expect(verdictOf(stop({ notified: false }, "2026-11-07T04:25:53Z", "other"), "s1", SINCE)).toBeNull();
    // a reply line (the user answered) is not a stop
    expect(verdictOf(JSON.stringify({ date: "2026-11-07T04:25:53Z", event: "reply", session: "s1", stopNotified: false }), "s1", SINCE)).toBeNull();
    // the previous turn of the same session: more than 2 seconds before this status
    expect(verdictOf(stop({ notified: false }, "2026-11-07T04:20:00Z"), "s1", SINCE)).toBeNull();
  });

  it("the record may be a second or two older than status_since (both are whole seconds)", () => {
    expect(verdictOf(stop({ notified: false }, "2026-11-07T04:25:52Z"), "s1", SINCE)).toBe("report");
    expect(verdictOf(stop({ notified: false }, "2026-11-07T04:25:51Z"), "s1", SINCE)).toBe("report");
    expect(verdictOf(stop({ notified: false }, "2026-11-07T04:25:50Z"), "s1", SINCE)).toBeNull();
  });

  it("skips half-written and unrelated lines", () => {
    const log = ['{"date":"2026-11-07T04:2', "", stop({ notified: false }, "2026-11-07T04:25:53Z", "x"), stop({ notified: false })].join("\n");
    expect(verdictOf(log, "s1", SINCE)).toBe("report");
  });
});

function io(reads: (string | null)[], timeoutMs = 3000) {
  const slept: number[] = [];
  let i = 0;
  return {
    slept,
    wait: {
      readTail: async () => reads[Math.min(i++, reads.length - 1)],
      sleep: async (ms: number) => void slept.push(ms),
      timeoutMs,
      intervalMs: 250,
    },
  };
}

describe("waitForVerdict", () => {
  it("returns as soon as the record is there", async () => {
    const { wait, slept } = io(["", "", stop({ notified: false })]);
    expect(await waitForVerdict("s1", SINCE, wait)).toBe("report");
    expect(slept).toHaveLength(2);
  });

  it("a record that says waiting does not wait any longer", async () => {
    const { wait, slept } = io([stop({ notified: true })]);
    expect(await waitForVerdict("s1", SINCE, wait)).toBe("notify");
    expect(slept).toHaveLength(0);
  });

  it("no log file: notify at once, without waiting", async () => {
    const { wait, slept } = io([null]);
    expect(await waitForVerdict("s1", SINCE, wait)).toBe("notify");
    expect(slept).toHaveLength(0);
  });

  it("no record within 3 seconds: notify", async () => {
    const { wait, slept } = io([""]);
    expect(await waitForVerdict("s1", SINCE, wait)).toBe("notify");
    expect(slept.reduce((a, b) => a + b, 0)).toBe(3000);
  });
});
