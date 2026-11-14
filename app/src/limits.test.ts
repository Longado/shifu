import { describe, expect, it } from "vitest";
import { ACCENT_AT, codebuddySegments, creditsText, isHot, isReset, limitLine, limitLines, resetText, untilText, windowName, type CodeBuddyUse, type VendorLimits } from "./limits";

/** Local-time seconds, so the tests do not depend on the machine's time zone. */
const at = (y: number, mo: number, d: number, h: number, mi: number) => new Date(y, mo - 1, d, h, mi).getTime() / 1000;

describe("windowName", () => {
  it("names the known windows per vendor and falls back to minutes", () => {
    expect(windowName(300, "claude")).toBe("5小时");
    expect(windowName(300, "codex")).toBe("5小时");
    expect(windowName(10080, "codex")).toBe("本周");
    expect(windowName(10080, "kimi")).toBe("7天");
    expect(windowName(10080, "claude")).toBe("7天");
    expect(windowName(60, "codex")).toBe("60分钟");
  });
});

describe("resetText", () => {
  const now = at(2026, 11, 6, 9, 30); // a Friday
  it("is the clock time within today and weekday plus time otherwise", () => {
    expect(resetText(at(2026, 11, 6, 13, 11), now)).toBe("13:11");
    expect(resetText(at(2026, 11, 6, 23, 59), now)).toBe("23:59");
    expect(resetText(at(2026, 11, 9, 13, 11), now)).toBe("周一 13:11");
    expect(resetText(at(2026, 11, 7, 0, 5), now)).toBe("周六 00:05");
  });
});

describe("untilText", () => {
  it("counts down in days and hours, then hours and minutes, then minutes rounded up", () => {
    const now = 1_000_000;
    expect(untilText(now + 3 * 86400, now)).toBe("3天后重置");
    expect(untilText(now + 86400 + 5 * 3600 + 1800, now)).toBe("1天5小时后重置");
    expect(untilText(now + 3600, now)).toBe("1小时后重置");
    expect(untilText(now + 3600 + 25 * 60 + 10, now)).toBe("1小时25分后重置");
    expect(untilText(now + 59 * 60, now)).toBe("59分钟后重置");
    expect(untilText(now + 90, now)).toBe("2分钟后重置");
    expect(untilText(now + 1, now)).toBe("1分钟后重置");
  });
});

describe("reset and accent", () => {
  it("a window is reset when its time is now or earlier", () => {
    const w = { used_percent: 100, window_minutes: 10080, resets_at: 1000 };
    expect(isReset(w, 999)).toBe(false);
    expect(isReset(w, 1000)).toBe(true);
  });
  it("90 percent and above is hot", () => {
    expect(ACCENT_AT).toBe(90);
    expect(isHot({ used_percent: 89.9, window_minutes: 300, resets_at: 1 })).toBe(false);
    expect(isHot({ used_percent: 90, window_minutes: 300, resets_at: 1 })).toBe(true);
  });
});

describe("creditsText", () => {
  it("drops the decimals and groups thousands", () => {
    expect(creditsText("29855.1005840000")).toBe("29,855");
    expect(creditsText("0")).toBe("0");
    expect(creditsText("n/a")).toBeNull();
  });
});

describe("limitLine", () => {
  const now = at(2026, 11, 6, 22, 0);
  const codex: VendorLimits = {
    windows: [{ used_percent: 100, window_minutes: 10080, resets_at: at(2026, 11, 9, 13, 11) }],
    credits: "29855.1005840000",
    updated_at: at(2026, 11, 6, 21, 50),
  };

  it("real Codex data: one window at 100 percent, credits, reset on Monday", () => {
    const line = limitLine("codex", codex, now);
    expect(line.label).toBe("Codex");
    expect(line.segments).toEqual([
      { label: "本周", value: "100%", hot: true, fill: 100 },
      { label: "credits", value: "29,855", credits: true },
      { label: "2天15小时后重置" },
    ]);
    expect(line.title).toContain("本周：已用 100%，11月9日 周一 13:11 重置");
    expect(line.title).toContain("credits 余额 29,855");
    expect(line.title).toContain("数据记于 21:50");
  });

  it("credits only show when a live window is at 100", () => {
    const line = limitLine("codex", { ...codex, windows: [{ ...codex.windows[0], used_percent: 99.6 }] }, now);
    expect(line.segments.map((s) => s.label)).toEqual(["本周", "2天15小时后重置"]);
    expect(line.segments[0]).toEqual({ label: "本周", value: "100%", hot: true, fill: 99.6 });
    const none = limitLine("codex", { ...codex, credits: null }, now);
    expect(none.segments.map((s) => s.label)).toEqual(["本周", "2天15小时后重置"]);
  });

  it("a past reset time waits for a new reading, with no old percentage or inferred recovery", () => {
    const line = limitLine("codex", codex, at(2026, 11, 9, 14, 0));
    expect(line.segments).toEqual([{ label: "本周", value: "待更新", fill: 0 }]);
    expect(line.label).toContain("旧数据");
  });

  it("the nearest reset among the live windows is written; a reset window is skipped", () => {
    const claude: VendorLimits = {
      windows: [
        { used_percent: 42.4, window_minutes: 300, resets_at: at(2026, 11, 6, 23, 30) },
        { used_percent: 63, window_minutes: 10080, resets_at: at(2026, 11, 9, 9, 0) },
      ],
      credits: null,
      updated_at: at(2026, 11, 6, 21, 0),
    };
    expect(limitLine("claude", claude, now).segments).toEqual([
      { label: "5小时", value: "42%", hot: false, fill: 42.4 },
      { label: "7天", value: "63%", hot: false, fill: 63 },
      { label: "1小时30分后重置" },
    ]);
    const later = limitLine("claude", claude, at(2026, 11, 7, 0, 0));
    expect(later.segments).toEqual([
      { label: "5小时", value: "待更新", fill: 0 },
      { label: "7天", value: "63%", hot: false, fill: 63 },
      { label: "2天9小时后重置" },
    ]);
  });

  it("keeps a failed reading's percentage and timestamp with its reason", () => {
    const line = limitLine("codex", { ...codex, error: "Codex 查询超时" }, now);
    expect(line.label).toBe("Codex（旧数据）");
    expect(line.segments[0].value).toBe("100%");
    expect(line.note).toContain("数据截至");
    expect(line.note).toContain("Codex 查询超时");
    expect(limitLine("codex", { ...codex, updated_at: now - 601 }, now).label).toContain("旧数据");
    const empty = limitLine("claude", { windows: [], credits: null, updated_at: 0, error: "查询失败" }, now);
    expect(empty.message).toBe("暂无数据");
    expect(empty.note).toContain("查询失败");
  });
});

describe("limitLines", () => {
  it("a vendor without data has no line", () => {
    const data: VendorLimits = { windows: [{ used_percent: 1, window_minutes: 300, resets_at: 5000 }], credits: null, updated_at: 1 };
    const antigravity = { groups: [], updated_at: 0, error: null };
    expect(limitLines({ claude: null, codex: null, kimi: null, codebuddy: null, antigravity }, 0)).toEqual([]);
    expect(limitLines({ claude: null, codex: data, kimi: null, codebuddy: null, antigravity }, 0).map((l) => l.label)).toEqual(["Codex"]);
    expect(limitLines({ claude: data, codex: data, kimi: data, codebuddy: null, antigravity }, 0).map((l) => l.label)).toEqual(["Claude", "Codex", "Kimi"]);
  });
});

describe("codebuddySegments", () => {
  // Local noon of some day, and that day's midnight in ms.
  const noon = new Date(2026, 10, 9, 12, 0, 0);
  const now = Math.floor(noon.getTime() / 1000);
  const ms = (h: number) => new Date(2026, 10, 9, h, 0, 0).getTime();

  it("adds up only today's credits and shows the free quota until it resets", () => {
    const use: CodeBuddyUse = { spends: [[ms(0) - 60_000, 9], [ms(9), 15.06], [ms(11), 1.28]], free: { used_percent: 9, resets_at: now + 3600 } };
    expect(codebuddySegments(use, now)).toEqual([
      { label: "免费", value: "9%", hot: false, fill: 9 },
      { label: "今天", value: "16.3 积分", credits: true },
    ]);
  });

  it("has no rows when nothing was spent today and the free quota has reset", () => {
    expect(codebuddySegments({ spends: [[ms(0) - 1, 4]], free: { used_percent: 50, resets_at: now - 1 } }, now)).toEqual([]);
  });
});
