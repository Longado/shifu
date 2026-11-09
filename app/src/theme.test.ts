import { describe, expect, it } from "vitest";
import { PRESETS, cssVars, isDark, mix } from "./theme";

const channels = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));

describe("theme", () => {
  it("the 夜灰 preset reproduces the original hand-picked palette", () => {
    // The colors style.css hard-coded before themes existed. They lean slightly bluer than a straight mix, so the
    // derived ones may differ by a few levels in the blue channel.
    const original: Record<string, string> = {
      "--bg-side": "#18191d",
      "--bg-hover": "#23252b",
      "--bg-active": "#2a2c33",
      "--bg-pop": "#25272d",
      "--bg-pressed": "#31343b",
      "--line": "#2f3137",
      "--line-strong": "#3a3d44",
      "--line-focus": "#5a5e68",
      "--sash": "#4a4d56",
      "--text-2": "#8b8f98",
      "--text-3": "#5f636c",
      "--gray": "#6b6f78",
      "--blue-dim": "#3f6199",
    };
    const vars = cssVars(PRESETS[0].theme);
    for (const [k, want] of Object.entries(original)) {
      const got = channels(vars[k]);
      channels(want).forEach((c, i) => expect(Math.abs(got[i] - c), `${k} ${vars[k]} vs ${want}`).toBeLessThanOrEqual(8));
    }
  });

  it("mixes and classifies", () => {
    expect(mix("#000000", "#ffffff", 0.5)).toBe("#808080");
    expect(isDark("#1e1f24")).toBe(true);
    expect(isDark("#f6f5f2")).toBe(false);
    expect(cssVars(PRESETS[3].theme)["color-scheme"]).toBe("light");
  });
});
