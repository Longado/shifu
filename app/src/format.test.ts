import { describe, expect, it } from "vitest";
import { ageText, agentLabel, needsYou, shortPath, sortAgents, statusLabel, statusLine, viewKey, type AgentView, type Mark, type Status } from "./format";

function agent(name: string, status: Status, since: number, mark?: Mark, title: string | null = null, last: string | null = null): AgentView {
  return {
    name,
    kind: "claude",
    cwd: "/w/" + name,
    dir_name: name,
    branch: null,
    title,
    status,
    status_since: since,
    last_message: last,
    exited: status === "exited",
    mark,
  };
}

describe("ageText", () => {
  it("rounds down to the largest whole unit", () => {
    expect(ageText(0)).toBe("刚刚");
    expect(ageText(59)).toBe("刚刚");
    expect(ageText(60)).toBe("1 分钟");
    expect(ageText(179)).toBe("2 分钟");
    expect(ageText(3599)).toBe("59 分钟");
    expect(ageText(3600)).toBe("1 小时");
    expect(ageText(86399)).toBe("23 小时");
    expect(ageText(86400 * 3 + 5)).toBe("3 天");
  });

  it("treats a clock that is slightly behind as just now", () => {
    expect(ageText(-5)).toBe("刚刚");
  });
});

describe("statusLabel / statusLine", () => {
  it("names every status in Chinese", () => {
    expect(statusLabel("needs_approval")).toBe("等你批准");
    expect(statusLabel("new")).toBe("做完了");
    expect(statusLabel("asked")).toBe("在问你");
    expect(statusLabel("parked")).toBe("停着");
    expect(statusLabel("running")).toBe("运行中");
    expect(statusLabel("idle")).toBe("空闲");
    expect(statusLabel("starting")).toBe("启动中");
    expect(statusLabel("exited")).toBe("已退出");
  });

  it("puts the wait time after the label", () => {
    expect(statusLine(agent("a", "needs_approval", 1000), 1000 + 180)).toBe("等你批准 3 分钟");
    expect(statusLine(agent("a", "running", 1000), 1010)).toBe("运行中 刚刚");
  });

  it("splits a reply into new and parked; a reply nobody marked counts as new", () => {
    expect(statusLine(agent("a", "needs_reply", 1000, "new"), 1000 + 180)).toBe("做完了 3 分钟");
    expect(statusLine(agent("a", "needs_reply", 1000, "parked"), 1000 + 86400 * 2)).toBe("停着 2 天");
    expect(viewKey(agent("a", "needs_reply", 1))).toBe("new");
  });

  it("a new reply whose last line asks something is a question; looked-at ones stay parked", () => {
    const reply = (last: string, mark: Mark = "new") => viewKey(agent("a", "needs_reply", 1, mark, null, last));
    expect(reply("要我接着把正式版也装上吗？")).toBe("asked");
    expect(reply("Which one should I keep?\n\n")).toBe("asked");
    expect(reply("先做 A 还是 B？**")).toBe("asked");
    expect(reply("要不要推？\n\n改动都在 HANDOFF 里。")).toBe("new");
    expect(reply("已经装好了。")).toBe("new");
    expect(reply("要我接着装吗？", "parked")).toBe("parked");
    expect(needsYou(agent("a", "needs_reply", 1, "new", null, "装吗？"))).toBe(true);
  });
});

describe("sortAgents", () => {
  it("orders by status group, then the longest wait first, then name", () => {
    const list = [
      agent("exited", "exited", 1),
      agent("idle", "idle", 5),
      agent("run-new", "running", 50),
      agent("reply-old", "needs_reply", 10, "new"),
      agent("parked-old", "needs_reply", 2, "parked"),
      agent("approve-new", "needs_approval", 90),
      agent("approve-old", "needs_approval", 20),
      agent("run-old", "running", 7),
      agent("b-starting", "starting", 5),
    ];
    expect(sortAgents(list).map((a) => a.name)).toEqual([
      "approve-old",
      "approve-new",
      "reply-old",
      "run-old",
      "run-new",
      "parked-old",
      "b-starting",
      "idle",
      "exited",
    ]);
  });

  it("returns a new array and leaves the input alone", () => {
    const list = [agent("b", "idle", 1), agent("a", "idle", 1)];
    const sorted = sortAgents(list);
    expect(sorted).not.toBe(list);
    expect(list.map((a) => a.name)).toEqual(["b", "a"]);
    expect(sorted.map((a) => a.name)).toEqual(["a", "b"]);
  });
});

describe("agentLabel / needsYou", () => {
  it("shows the topic, else the session name", () => {
    expect(agentLabel(agent("claude-W-3", "idle", 1, undefined, "修仙模拟器"))).toBe("修仙模拟器");
    expect(agentLabel(agent("claude-W-3", "idle", 1))).toBe("claude-W-3");
  });
  it("a name the user gave the tab wins over the topic", () => {
    expect(agentLabel({ ...agent("claude-W-3", "idle", 1, undefined, "修仙模拟器"), label: "前端" })).toBe("前端");
  });

  it("needs the user: approval or a new reply, not a parked one", () => {
    expect(needsYou(agent("a", "needs_approval", 1))).toBe(true);
    expect(needsYou(agent("a", "needs_reply", 1, "new"))).toBe(true);
    expect(needsYou(agent("a", "needs_reply", 1, "parked"))).toBe(false);
    expect(needsYou(agent("a", "running", 1))).toBe(false);
  });
});

describe("shortPath", () => {
  it("keeps the last three folders and marks what was cut", () => {
    expect(shortPath("/Users/meme/Desktop/Workspace/01_项目/shifu")).toBe("…/Workspace/01_项目/shifu");
  });

  it("leaves a short path alone", () => {
    expect(shortPath("/tmp/proj")).toBe("/tmp/proj");
    expect(shortPath("/a/b/c")).toBe("/a/b/c");
  });

  it("ignores a trailing slash", () => {
    expect(shortPath("/a/b/c/d/")).toBe("…/b/c/d");
  });
});
