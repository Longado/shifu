import { describe, expect, it } from "vitest";
import { PRESETS, contrast, cssVars, getTheme, isDark, mix, terminalTheme } from "./theme";

const channels = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));

// Presets are looked up by name: inserting one must not silently re-point an index-based assertion.
const preset = (name: string) => {
  const p = PRESETS.find((x) => x.name === name);
  if (!p) throw new Error(`no preset ${name}`);
  return p.theme;
};

describe("theme", () => {
  it("the 夜灰 preset reproduces the original hand-picked palette", () => {
    // The colors style.css hard-coded before themes existed. They lean slightly bluer than a straight mix, so the
    // derived ones may differ by a few levels in the blue channel.
    // --text-2 and --text-3 are deliberately absent: the hand-picked values sat at 4.95:1 and 2.62:1,
    // and both now carry a contrast floor. Everything else still reproduces the original palette.
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
      "--gray": "#6b6f78",
      "--blue-dim": "#3f6199",
    };
    const vars = cssVars(preset("夜灰"));
    for (const [k, want] of Object.entries(original)) {
      const got = channels(vars[k]);
      channels(want).forEach((c, i) => expect(Math.abs(got[i] - c), `${k} ${vars[k]} vs ${want}`).toBeLessThanOrEqual(8));
    }
  });

  it("mixes and classifies", () => {
    expect(mix("#000000", "#ffffff", 0.5)).toBe("#808080");
    expect(isDark("#1e1f24")).toBe(true);
    expect(isDark("#f6f5f2")).toBe(false);
    expect(cssVars(preset("浅色"))["color-scheme"]).toBe("light");
  });

  it("墨黑 is the landing page's three colors and is the default", () => {
    const ink = preset("墨黑");
    expect(ink).toEqual({ bg: "#0a0b0c", text: "#eeeeef", accent: "#5b9cf5" });
    const vars = cssVars(ink);
    expect(vars["color-scheme"]).toBe("dark");
    expect(contrast(vars["--text"], vars["--bg"])).toBeGreaterThanOrEqual(7);
    expect(contrast(vars["--blue"], vars["--bg"])).toBeGreaterThanOrEqual(4.5);
    // No DOM and no stored theme in this environment, so load() falls back to the default:
    // getTheme() is the default here. resetTheme() cannot run — it paints document.documentElement.
    expect(getTheme()).toEqual(ink);
  });

  it("every preset keeps secondary and tertiary text legible on every UI surface, in the right order", () => {
    for (const p of PRESETS) {
      const v = cssVars(p.theme);
      for (const key of ["--bg", "--bg-side", "--bg-hover", "--bg-active", "--bg-pop", "--bg-pressed"]) {
        const c2 = contrast(v["--text-2"], v[key]);
        const c3 = contrast(v["--text-3"], v[key]);
        expect(c2, `${p.name} ${key} text-2`).toBeGreaterThanOrEqual(6);
        expect(c3, `${p.name} ${key} text-3`).toBeGreaterThanOrEqual(4.5);
        // Tertiary must stay quieter than secondary, or the hierarchy inverts.
        expect(c2, `${p.name} ${key} order`).toBeGreaterThan(c3);
        expect(contrast(v["--text"], v[key]), `${p.name} ${key} text`).toBeGreaterThan(c2);
      }
    }
  });

  it("a mix that passes on the main background must also pass on raised surfaces", () => {
    // White on black at 0.426 passes 6:1 on black, but not on the pressed-row background.
    const stark = { bg: "#000000", text: "#ffffff", accent: "#5b9cf5" };
    const starkVars = cssVars(stark);
    const old = mix(stark.text, stark.bg, 0.426);
    expect(contrast(old, stark.bg)).toBeGreaterThanOrEqual(6);
    expect(contrast(old, starkVars["--bg-pressed"])).toBeLessThan(6);
    expect(contrast(starkVars["--text-2"], starkVars["--bg-pressed"])).toBeGreaterThanOrEqual(6);
    // 夜灰 at 0.66 was 2.62:1, so it must be pulled back toward the text color.
    const night = preset("夜灰");
    const v = cssVars(night);
    expect(v["--text-3"]).not.toBe(mix(night.text, night.bg, 0.66));
    expect(channels(v["--text-3"])[0]).toBeGreaterThan(channels(mix(night.text, night.bg, 0.66))[0]);
  });

  it("a custom low-contrast pick still gets the most legible result available", () => {
    // Nothing can reach 4.5:1 between these two; the floor must not overshoot past the text color.
    const flat = { bg: "#777777", text: "#999999", accent: "#5b9cf5" };
    const v = cssVars(flat);
    expect(contrast(v["--text-3"], flat.bg)).toBeLessThanOrEqual(contrast(flat.text, flat.bg) + 0.01);
  });

  it("every dark preset keeps the 16 ANSI colors, so black does not lose colored output", () => {
    const ansiKeys = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
      "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite"];
    for (const p of PRESETS.filter((x) => isDark(x.theme.bg))) {
      const t = terminalTheme(p.theme);
      for (const k of ansiKeys) expect(t[k as keyof typeof t], `${p.name} ${k}`).toBeTruthy();
      expect(t.background).toBe(p.theme.bg);
      expect(t.foreground).toBe(p.theme.text);
    }
  });

  it("Mocha keeps its own cursor and selection; other dark themes keep theirs", () => {
    const mocha = terminalTheme(preset("Mocha"));
    expect(mocha.cursor).toBe("#f5e0dc");
    expect(mocha.selectionBackground).toBe("#585b70");
    const ink = preset("墨黑");
    const t = terminalTheme(ink);
    expect(t.cursor).toBe(ink.text);
    expect(t.selectionBackground).toBe(mix(ink.bg, ink.accent, 0.35));
  });

  it("the light preset still repairs xterm's white and yellow and borrows no dark ANSI", () => {
    const t = terminalTheme(preset("浅色"));
    expect(t.white).toBe("#5f636c");
    expect(t.brightYellow).toBe("#b07d00");
    expect(t.red).toBeUndefined();
  });
});
