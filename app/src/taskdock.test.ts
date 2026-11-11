import { describe, expect, it } from "vitest";
import { type DockPort, type LayoutStore, TaskDock } from "./taskdock";

const wait = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Behaves like dockview where it matters here: layout-change events arrive in a microtask after the change. */
class FakeDock implements DockPort {
  panels: string[] = [];
  onChange: () => void = () => {};
  private touch() {
    queueMicrotask(() => this.onChange());
  }
  toJSON = () => JSON.stringify({ panels: Object.fromEntries(this.panels.map((n) => [n, {}])) });
  clear() {
    this.panels = [];
    this.touch();
  }
  fromJSON(json: string) {
    this.panels = Object.keys(JSON.parse(json).panels);
    this.touch();
  }
  openAgents(names: readonly string[]) {
    this.panels = [...new Set([...this.panels, ...names])];
    this.touch();
  }
  closeAgents(names: readonly string[]) {
    this.panels = this.panels.filter((n) => !names.includes(n));
    this.touch();
  }
  settled = () => wait(0);
  /** What the user does: open or close tabs. */
  edit(panels: string[]) {
    this.panels = panels;
    this.touch();
  }
}

class FakeStore implements LayoutStore {
  files = new Map<string, string>();
  writes: { id: string; json: string }[] = [];
  async load(id: string) {
    await wait(2);
    return this.files.get(id) ?? null;
  }
  async save(id: string, json: string) {
    await wait(3);
    this.files.set(id, json);
    this.writes.push({ id, json });
  }
}

function setup() {
  const dock = new FakeDock();
  const store = new FakeStore();
  const errors: unknown[] = [];
  const td = new TaskDock(dock, store, (e) => errors.push(e), 4);
  dock.onChange = () => td.changed();
  return { dock, store, td, errors };
}

const panelsOf = (store: FakeStore, id: string) => Object.keys(JSON.parse(store.files.get(id)!).panels);

describe("TaskDock", () => {
  it("keeps both tasks' layouts through several switches, even with edits right before a switch", async () => {
    const { dock, store, td, errors } = setup();
    await td.show("A", ["a1"]);
    dock.edit(["a1", "a2"]);
    await td.show("B", ["b1"]);
    dock.edit(["b1", "b2", "b3"]);
    await td.show("A", ["a1"]); // edit above is still inside the save delay
    expect(dock.panels).toEqual(["a1", "a2"]);
    dock.edit(["a2", "a3"]);
    await td.show("B", ["b1"]);
    expect(dock.panels).toEqual(["b1", "b2", "b3"]);
    await td.show("A", ["a1"]);
    await td.show("B", ["b1"]);
    await wait(60); // let any stale debounced save fire

    expect(panelsOf(store, "A")).toEqual(["a2", "a3"]);
    expect(panelsOf(store, "B")).toEqual(["b1", "b2", "b3"]);
    // Clearing the dock for a switch must never reach a file as an empty layout.
    expect(store.writes.filter((w) => Object.keys(JSON.parse(w.json).panels).length === 0)).toEqual([]);
    expect(errors).toEqual([]);
  });

  it("saves a quiet edit after the delay, for the task that was shown", async () => {
    const { dock, store, td } = setup();
    await td.show("A", ["a1"]);
    dock.edit(["a1", "a9"]);
    await wait(60);
    expect(panelsOf(store, "A")).toEqual(["a1", "a9"]);
  });

  it("survives switches asked for without waiting for the previous one", async () => {
    const { dock, store, td } = setup();
    await td.show("A", ["a1"]);
    dock.edit(["a1", "a2"]);
    const all = [td.show("B", ["b1"]), td.show("A", ["a1"]), td.show("B", ["b1"]), td.show("A", ["a1"])];
    await Promise.all(all);
    await wait(60);
    expect(td.shown).toBe("A");
    expect(dock.panels).toEqual(["a1", "a2"]);
    expect(panelsOf(store, "A")).toEqual(["a1", "a2"]);
    expect(panelsOf(store, "B")).toEqual(["b1"]);
  });

  it("does not write the layout of a task that was detached (deleted)", async () => {
    const { dock, store, td } = setup();
    await td.show("A", ["a1"]);
    dock.edit(["a1", "a2"]);
    await td.detach();
    await wait(60);
    expect(store.files.has("A")).toBe(false);
    expect(td.shown).toBeNull();
    expect(dock.panels).toEqual([]);
  });

  it("remembers a move across restart, including another task's old copy, without banning later shared views", async () => {
    const { dock, store, td, errors } = setup();
    await td.show("A", ["a", "file:notes"]);
    await td.show("B", ["b"]);
    await td.show("C", ["c"]);
    dock.edit(["c", "a"]); // an older layout also shows a
    await td.show("A", ["a"]);
    await td.moveAgent("a", "B", ["A", "B", "C"]);
    expect(dock.panels).toEqual(["file:notes"]);
    expect(td.shown).toBe("A");

    const fresh = new FakeDock();
    const restarted = new TaskDock(fresh, store, (e) => errors.push(e), 4);
    fresh.onChange = () => restarted.changed();
    await restarted.show("C", ["c"]);
    expect(fresh.panels).toEqual(["c"]);
    await restarted.show("B", ["b", "a"]);
    expect(fresh.panels).toEqual(["b", "a"]);
    await restarted.show("A", []);
    expect(fresh.panels).toEqual(["file:notes"]);
    await restarted.show("C", ["c"]);
    fresh.edit(["c", "a"]); // explicitly share it again after the move
    await restarted.show("A", []);
    await restarted.show("C", ["c"]);
    expect(fresh.panels).toEqual(["c", "a"]);
    expect(errors).toEqual([]);
  });

  it("keeps only the last destination when an agent moves twice before either destination is opened", async () => {
    const { dock, store, td } = setup();
    await td.show("B", ["b"]);
    await td.show("C", ["c"]);
    await td.show("A", ["a"]);
    await td.moveAgent("a", "B", ["A", "B", "C"]);
    await td.moveAgent("a", "C", ["A", "B", "C"]);
    const fresh = new TaskDock(dock, store, () => {}, 4);
    dock.onChange = () => fresh.changed();
    await fresh.show("B", ["b"]);
    expect(dock.panels).toEqual(["b"]);
    await fresh.show("C", ["c", "a"]);
    expect(dock.panels).toEqual(["c", "a"]);
  });

  it("opens the task's agents as tabs when its saved layout cannot be read", async () => {
    const { dock, store, td, errors } = setup();
    store.files.set("A", "{not json");
    await td.show("A", ["a1", "a2"]);
    expect(dock.panels).toEqual(["a1", "a2"]);
    expect(errors).toHaveLength(1);
  });
});
