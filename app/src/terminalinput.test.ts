import { expect, it } from "vitest";
import { TerminalInput } from "./terminalinput";

const deferred = () => {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
};

it("keeps fast text, ShiftEnter, IME and paste in order while another pane stays independent", async () => {
  const a = new TerminalInput(), b = new TerminalInput();
  a.attach(1); b.attach(2);
  const wait = deferred(), entered = deferred();
  const sent: string[] = [];
  const first = a.enqueue(async () => { entered.resolve(); await wait.promise; sent.push("a"); });
  const rest = ["ShiftEnter", "中文", "pasted\ntext", "Enter"].map((key) => a.enqueue(async () => { sent.push(key); }));
  await entered.promise;
  await b.enqueue(async () => { sent.push("other pane"); });
  expect(sent).toEqual(["other pane"]);
  wait.resolve();
  await Promise.all([first, ...rest]);
  expect(sent).toEqual(["other pane", "a", "ShiftEnter", "中文", "pasted\ntext", "Enter"]);
});

it("drops queued keys and keys waiting for scroll exit when attachment is replaced", async () => {
  const queue = new TerminalInput(), wait = deferred(), entered = deferred();
  queue.attach(1);
  const sent: string[] = [];
  const first = queue.enqueue(async (current) => {
    entered.resolve(); await wait.promise;
    if (current()) sent.push("old scroll key");
  });
  const old = queue.enqueue(async () => { sent.push("old key"); });
  await entered.promise;
  queue.invalidate();
  queue.attach(2);
  const fresh = queue.enqueue(async () => { sent.push("new attachment key"); });
  wait.resolve();
  await Promise.all([first, old, fresh]);
  expect(sent).toEqual(["new attachment key"]);
});

it("keeps accepting input after an IPC rejection", async () => {
  const queue = new TerminalInput();
  queue.attach(1);
  await expect(queue.enqueue(async () => { throw new Error("closed"); })).rejects.toThrow("closed");
  let sent = false;
  await queue.enqueue(async () => { sent = true; });
  expect(sent).toBe(true);
});

it("holds pre-attach text and ShiftEnter in order, dropping them on disposal", async () => {
  const queue = new TerminalInput();
  const sent: string[] = [];
  const text = queue.enqueue(async () => { sent.push("text"); });
  const newline = queue.enqueue(async () => { sent.push("ShiftEnter"); });
  await Promise.resolve();
  expect(sent).toEqual([]);
  queue.attach(7);
  await Promise.all([text, newline]);
  expect(sent).toEqual(["text", "ShiftEnter"]);
  queue.invalidate();
  const abandoned = queue.enqueue(async () => { sent.push("disposed"); });
  queue.invalidate();
  await abandoned;
  expect(sent).toEqual(["text", "ShiftEnter"]);
});
