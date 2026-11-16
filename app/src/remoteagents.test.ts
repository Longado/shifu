import { describe, it, expect } from "vitest";
import { remoteAgents, remoteTaskGroups, type RemoteAgent } from "./remoteagents";

const agent = (name: string, task_id?: string, task_title?: string): RemoteAgent => ({
  name, title: name, kind: "codex", status: "needs_reply", cwd: "/work", task: null, task_id, task_title,
});

describe("remote task membership", () => {
  it("groups by real task identity, including automatic tasks, without merging duplicate names", () => {
    const a = agent("a", "one", "同名任务"), b = agent("b", "one", "同名任务"), c = agent("c", "two", "同名任务");
    expect(remoteTaskGroups([a, b, c], "")).toEqual({tasks: [
      {id: "one", title: "同名任务", agents: [a, b]}, {id: "two", title: "同名任务", agents: [c]},
    ], ungrouped: []});
  });
  it("searches task names and individual agents while preserving their parent", () => {
    const a = agent("first", "one", "工作任务"), b = agent("second", "one", "工作任务");
    expect(remoteTaskGroups([a, b], "工作").tasks[0].agents).toEqual([a, b]);
    expect(remoteTaskGroups([a, b], "second").tasks[0]).toEqual({id: "one", title: "工作任务", agents: [b]});
    expect(remoteTaskGroups([a, b], "missing")).toEqual({tasks: [], ungrouped: []});
  });
  it("accepts old hosts without inventing shared task membership", () => {
    const a = {...agent("a"), task: "同名任务"}, b = {...agent("b"), task: "同名任务"};
    expect(remoteTaskGroups(remoteAgents([a, b]), "")).toEqual({tasks: [], ungrouped: [a, b]});
    for (const bad of [{...a, task_id: 12}, {...a, task_title: {}}, {...a, task: false}]) expect(() => remoteAgents([bad])).toThrow();
  });
});
