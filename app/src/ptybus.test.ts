import { beforeAll, expect, it, vi } from "vitest";
import { beginPtyAttach, register, startPtyBus, unregister } from "./ptybus";

type Payload = { id: number; data?: string; code?: number | null };
const listeners = vi.hoisted(() => new Map<string, (e: { payload: Payload }) => void>());
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, fn: (e: { payload: Payload }) => void) => listeners.set(name, fn),
}));
beforeAll(startPtyBus);
const data = (id: number, text: string) => listeners.get("pty-data")!({ payload: { id, data: text } });
const exit = (id: number, code: number | null = 0) => listeners.get("pty-exit")!({ payload: { id, code } });
const recorder = () => {
  const events: (string | number | null)[] = [];
  return { events, data: (s: string) => events.push(s), exit: (code: number | null) => events.push(code) };
};

it("delivers early output and exit in order when simultaneous attach replies come back out of order", () => {
  const first = beginPtyAttach();
  const second = beginPtyAttach();
  const a = recorder(), b = recorder();
  data(1, "早");
  data(2, "hello");
  register(2, b);
  second();
  data(1, "到");
  exit(1, 7);
  register(1, a);
  first();
  data(2, " again");
  exit(2, null);
  expect(a.events).toEqual(["早", "到", 7]);
  expect(b.events).toEqual(["hello", " again", null]);
});

it("discards detached output while another attach still needs early events", () => {
  const a = recorder(), b = recorder();
  register(3, a);
  data(3, "live");
  unregister(3);
  const finish = beginPtyAttach();
  data(3, "flushing");
  exit(3);
  data(4, "early");
  register(4, b);
  finish();
  // Probing the old id detects any orphan queue still held by the bus.
  const probe = recorder();
  register(3, probe);
  expect(probe.events).toEqual([]);
  expect(a.events).toEqual(["live"]);
  expect(b.events).toEqual(["early"]);
  exit(3);
  exit(4);
});

it("releases events for a failed attach and ignores output arriving while no id is expected", () => {
  const failed = beginPtyAttach();
  data(5, "no owner");
  failed();
  data(6, "between attachments");
  const finish = beginPtyAttach();
  const a = recorder(), b = recorder();
  register(5, a);
  register(6, b);
  finish();
  expect(a.events).toEqual([]);
  expect(b.events).toEqual([]);
  exit(5);
  exit(6);
});

it("disposes clients that ended before their panel received the attach reply", () => {
  const finish = beginPtyAttach();
  data(7, "before teardown");
  exit(7);
  unregister(7);
  data(8, "pending");
  unregister(8);
  data(8, "flushing");
  exit(8);
  const a = recorder(), b = recorder();
  register(7, a);
  register(8, b);
  finish();
  expect(a.events).toEqual([]);
  expect(b.events).toEqual([]);
  exit(7);
  exit(8);
});
