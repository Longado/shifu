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
  { name: "暖棕", theme: { bg: "#24201c", text: "#e3dcd2", accent: "#c9785d" } },
  { name: "浅色", theme: { bg: "#f6f5f2", text: "#26272b", accent: "#2f6fd6" } },
  // Catppuccin Mocha, the default.
  { name: "Mocha", theme: { bg: "#1e1e2e", text: "#cdd6f4", accent: "#89b4fa" } },
];

const MOCHA = PRESETS[4].theme;
const DEFAULT = MOCHA;

/** Catppuccin Mocha's 16 terminal colors, so Claude's and Codex's colored output looks as it does in Ghostty. */
const MOCHA_ANSI: ITheme = {
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

/** CSS variables for a theme. The ratios reproduce the original hand-picked dark palette from its three colors. */
export function cssVars(t: Theme): Record<string, string> {
  const dark = isDark(t.bg);
  const toText = (k: number) => mix(t.bg, t.text, k);
  return {
    "--bg": t.bg,
    "--bg-side": mix(t.bg, "#000000", dark ? 0.2 : 0.035),
    "--bg-hover": toText(0.03),
    "--bg-active": toText(0.063),
    "--bg-pop": toText(0.037),
    "--bg-pressed": toText(0.1),
    "--line": toText(0.09),
    "--line-strong": toText(0.147),
    "--line-focus": toText(0.316),
    "--sash": toText(0.23),
    "--text": t.text,
    "--text-2": mix(t.text, t.bg, 0.426),
    "--text-3": mix(t.text, t.bg, 0.66),
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
  if (sameTheme(t, MOCHA)) return { ...base, ...MOCHA_ANSI };
  if (isDark(t.bg)) return base;
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
  const style = document.documentElement.style;
  for (const [k, v] of Object.entries(cssVars(t))) style.setProperty(k, v);
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
