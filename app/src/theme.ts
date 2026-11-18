import type { ITheme } from "@xterm/xterm";

/** Three picked colors; every other interface color is mixed from them so any choice stays consistent. */
export interface Theme {
  bg: string;
  text: string;
  accent: string;
}

export const PRESETS: { name: string; theme: Theme }[] = [
  { name: "夜灰", theme: { bg: "#1e1f24", text: "#dcdee3", accent: "#5b9cf5" } },
  { name: "纯黑", theme: { bg: "#0f0f10", text: "#e4e4e6", accent: "#43a5a5" } },
  // The default: near-black with the blue accent the interface has always used. A blacker
  // background also raises contrast against the same light text.
  { name: "墨黑", theme: { bg: "#0a0b0c", text: "#eeeeef", accent: "#5b9cf5" } },
  { name: "暖棕", theme: { bg: "#24201c", text: "#e3dcd2", accent: "#c9785d" } },
  { name: "浅色", theme: { bg: "#f6f5f2", text: "#26272b", accent: "#2f6fd6" } },
  // Catppuccin Mocha, a Ghostty theme; the previous default.
  { name: "Mocha", theme: { bg: "#1e1e2e", text: "#cdd6f4", accent: "#89b4fa" } },
];

const MOCHA = PRESETS.find((p) => p.name === "Mocha")!.theme;
const DEFAULT = PRESETS.find((p) => p.name === "墨黑")!.theme;

/** Catppuccin Mocha's 16 terminal colors, so Claude's and Codex's colored output looks as it does in Ghostty. */
const DARK_ANSI: ITheme = {
  black: "#45475a",
  red: "#f38ba8",
  green: "#a6e3a1",
  yellow: "#f9e2af",
  blue: "#89b4fa",
  magenta: "#f5c2e7",
  cyan: "#94e2d5",
  white: "#bac2de",
  brightBlack: "#585b70",
  brightRed: "#f38ba8",
  brightGreen: "#a6e3a1",
  brightYellow: "#f9e2af",
  brightBlue: "#89b4fa",
  brightMagenta: "#f5c2e7",
  brightCyan: "#94e2d5",
  brightWhite: "#a6adc8",
  cursor: "#f5e0dc",
  selectionBackground: "#585b70",
};
const KEY = "shifu.theme";

function rgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

/** `a` moved toward `b` by `t` (0..1). */
export function mix(a: string, b: string, t: number): string {
  const [x, y] = [rgb(a), rgb(b)];
  return `#${x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, "0")).join("")}`;
}

export function isDark(hex: string): boolean {
  const [r, g, b] = rgb(hex);
  return 0.299 * r + 0.587 * g + 0.114 * b < 128;
}

function luminance(hex: string): number {
  const [r, g, b] = rgb(hex).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio between two colors (1..21). */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/**
 * Dim `fg` toward `bg` by `t`, keeping `min` contrast on every surface that uses it.
 * Any three colors can be picked, so a fixed ratio cannot promise readability on its own;
 * the ratio is kept when it is already legible and backed off when it is not.
 */
function dim(fg: string, bg: string, t: number, min: number, surfaces: readonly string[]): string {
  const readable = (color: string) => surfaces.every((surface) => contrast(color, surface) >= min);
  if (readable(mix(fg, bg, t))) return mix(fg, bg, t);
  let lo = 0;
  let hi = t;
  for (let i = 0; i < 24; i++) {
    const mid = (lo + hi) / 2;
    if (readable(mix(fg, bg, mid))) lo = mid;
    else hi = mid;
  }
  return mix(fg, bg, lo);
}

/** CSS variables for a theme. The ratios reproduce the original hand-picked dark palette from its three colors. */
export function cssVars(t: Theme): Record<string, string> {
  const dark = isDark(t.bg);
  const toText = (k: number) => mix(t.bg, t.text, k);
  // Muted text also appears in the sidebar, popovers and hovered/selected rows.
  const surfaces = {
    "--bg": t.bg,
    "--bg-side": mix(t.bg, "#000000", dark ? 0.2 : 0.035),
    "--bg-hover": toText(0.03),
    "--bg-active": toText(0.063),
    "--bg-pop": toText(0.037),
    "--bg-pressed": toText(0.1),
  };
  const backgrounds = Object.values(surfaces);
  return {
    ...surfaces,
    "--line": toText(0.09),
    "--line-strong": toText(0.147),
    "--line-focus": toText(0.316),
    "--sash": toText(0.23),
    "--text": t.text,
    // Secondary and tertiary are both text, so both get a contrast floor. The floors are spaced
    // (6 vs 4.5) to keep the hierarchy: on the light preset the old text-2 was 3.67:1, so fixing
    // text-3 alone would have made tertiary labels louder than secondary ones.
    "--text-2": dim(t.text, t.bg, 0.426, 6, backgrounds),
    // The old 0.66 mix landed at 2.0-2.8:1, under AA on every preset.
    "--text-3": dim(t.text, t.bg, 0.66, 4.5, backgrounds),
    "--gray": mix(t.text, t.bg, 0.6),
    "--blue": t.accent,
    "--blue-dim": mix(t.accent, t.bg, 0.46),
    "--shadow": dark ? "rgba(0, 0, 0, 0.35)" : "rgba(0, 0, 0, 0.12)",
    "color-scheme": dark ? "dark" : "light",
  };
}

export function terminalTheme(t: Theme): ITheme {
  const base: ITheme = {
    background: t.bg,
    foreground: t.text,
    cursor: t.text,
    cursorAccent: t.bg,
    selectionBackground: mix(t.bg, t.accent, 0.35),
  };
  if (isDark(t.bg)) {
    // Picking a black background used to drop the 16 ANSI colors and fall back to xterm's
    // VGA defaults, so Claude's and Codex's output changed with the theme. Mocha keeps its
    // own cursor and selection; other dark themes keep theirs and only borrow the ANSI set.
    const ansi: ITheme = { ...DARK_ANSI };
    if (!sameTheme(t, MOCHA)) {
      ansi.cursor = base.cursor;
      ansi.selectionBackground = base.selectionBackground;
    }
    return { ...base, ...ansi };
  }
  // xterm's stock white and yellow vanish on a light background.
  return { ...base, white: "#5f636c", brightWhite: "#26272b", yellow: "#9a6b00", brightYellow: "#b07d00" };
}

function valid(t: unknown): t is Theme {
  const hex = (v: unknown) => typeof v === "string" && /^#[0-9a-f]{6}$/i.test(v);
  return typeof t === "object" && t !== null && ["bg", "text", "accent"].every((k) => hex((t as never)[k]));
}

function load(): Theme {
  try {
    const t: unknown = JSON.parse(localStorage.getItem(KEY) ?? "null");
    return valid(t) ? t : DEFAULT;
  } catch {
    return DEFAULT;
  }
}

let current = load();
const listeners = new Set<(t: Theme) => void>();

export function getTheme(): Theme {
  return current;
}

function paint(t: Theme): void {
  const root = document.documentElement;
  const vars = cssVars(t);
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
  root.dataset.themeTone = vars["color-scheme"];
}

export function applyStoredTheme(): void {
  paint(current);
}

export function setTheme(t: Theme): void {
  current = { ...t };
  localStorage.setItem(KEY, JSON.stringify(current));
  paint(current);
  listeners.forEach((f) => f(current));
}

export function resetTheme(): void {
  setTheme(DEFAULT);
}

export function onThemeChange(f: (t: Theme) => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

export function sameTheme(a: Theme, b: Theme): boolean {
  return a.bg.toLowerCase() === b.bg.toLowerCase() && a.text.toLowerCase() === b.text.toLowerCase() &&
    a.accent.toLowerCase() === b.accent.toLowerCase();
}
