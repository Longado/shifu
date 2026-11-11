import { describe, expect, it } from "vitest";
import { formatBytes, formatCpu, summary } from "./memory";

describe("memory line", () => {
  it("shows GB from one binary gigabyte up, MB below", () => {
    expect(formatBytes(1.5 * 1024 ** 3)).toBe("1.5 GB");
    expect(formatBytes(480 * 1024 ** 2)).toBe("480 MB");
    expect(formatBytes(0)).toBe("0 MB");
    expect(formatBytes(1001 * 1024 ** 2)).toBe("1.0 GB");
  });

  it("counts the open agents and adds up their memory", () => {
    const r = {
      agents: [{ name: "a", bytes: 1024 ** 3, processes: 3, cpu: 0 }, { name: "b", bytes: 1024 ** 3 / 2, processes: 1, cpu: 0 }],
      shifu: { app: 1, web: 1, tmux: 1, cpu: 0 },
      strays: [],
      host_total: 1,
      host_used_percent: 25,
      pressure: 1,
    };
    expect(summary(r)).toBe("2 个对话 · 1.5 GB");
  });

  it("leaves idle CPU blank and rounds the busy ones", () => {
    expect(formatCpu(0.4)).toBe("");
    expect(formatCpu(12.6)).toBe("13%");
    expect(formatCpu(180)).toBe("180%");
  });
});
