import { describe, expect, it } from "vitest";
import { antigravityLines, type AntigravityUsage } from "./antigravity";

const now = Date.parse("2026-11-11T09:00:00Z") / 1000;
const usage: AntigravityUsage = {
  updated_at: now, error: null,
  groups: [{ name: "Gemini Models", buckets: [
    { name: "Weekly Limit Remaining", window: "weekly", used_percent: 25, reset_time: "2026-11-14T09:00:00Z" },
    { name: "Five Hour Limit Remaining", window: "5h", used_percent: 95, reset_time: "2026-11-11T10:00:00Z" },
  ] }, { name: "Claude and GPT models", buckets: [
    { name: "Weekly Limit Remaining", window: "weekly", used_percent: 0, reset_time: null },
  ] }],
};
describe("Antigravity quota presentation", () => {
  it("keeps the two pools separate and marks almost spent allowances", () => {
    const lines = antigravityLines(usage, now);
    expect(lines.map(l => l.label)).toEqual(["Antigravity Gemini", "Antigravity Claude/GPT"]);
    expect(lines[0].segments.map(s => [s.label,s.value,s.hot])).toEqual([["周","25%",false],["5h","95%",true]]);
    expect(lines[0].title).toContain("已用 25%");
    expect(lines[0].title).toContain("剩余 75%");
    expect(lines[0].title).toContain("重置");
  });
  it("never presents an expired or unknown allowance as fresh zero usage", () => {
    const lines = antigravityLines({ ...usage, groups: [{ name: "Gemini Models", buckets: [
      { ...usage.groups[0].buckets[0], used_percent: null },
      { ...usage.groups[0].buckets[1], reset_time: "2026-11-11T08:00:00Z" },
    ] }] }, now);
    expect(lines[0].segments.map(s => s.value)).toEqual(["暂无数据","待更新"]);
    expect(lines[0].segments.every(s => !s.hot && s.fill === undefined)).toBe(true);
  });
  it("labels last known values when refresh fails or the snapshot ages out", () => {
    expect(antigravityLines({ ...usage,error:"读取失败" },now)[0].label).toContain("旧数据");
    expect(antigravityLines({ ...usage,error:"读取失败" },now)[0].title).toContain("读取失败");
    expect(antigravityLines(usage,now+601)[0].label).toContain("旧数据");
    const empty = antigravityLines({ groups:[],updated_at:0,error:"请先登录 agy" },now);
    expect(empty[0].message).toBe("请先登录 agy");
    expect(empty[0].segments).toEqual([]);
  });
});
