/**
 * The little figures in the usage footer. None of them hops; Claude's alone now and then blinks, waves or taps its
 * feet, more often while a Claude agent is running.
 * Claude's is Clawd, the pixel creature Claude Code prints at start-up, drawn on the same grid as the AtomS3R desk
 * buddy (01_项目/claude-buddy-atoms3r/src/clawd.cpp). Codex, CodeBuddy and Kimi have no mascot of their own, so
 * their marks stand in, the same as in the "+" menu. They belong to Anthropic, OpenAI, Tencent and Moonshot.
 */
import { vendorIcon } from "./vendoricons";

export type MascotKind = "claude" | "codex" | "codebuddy" | "kimi";

/** Clawd on a grid of cells one wide and two tall (a terminal cell's quarter blocks); "#" is body. */
const ART = ["...############...", "...##.######.##...", ".################.", "...############..."];
const LEGS = [4, 6, 11, 13];
const EYES = [5, 12];
const ORANGE = "#d97757";
const NS = "http://www.w3.org/2000/svg";

type Arm = "out" | "up";
interface Pose {
  eyes: "open" | "shut";
  left: Arm;
  right: Arm;
  /** Leg pairs (1st and 3rd, 2nd and 4th): 2 standing, 1 lifted. */
  legsA: number;
  legsB: number;
}
const STAND: Pose = { eyes: "open", left: "out", right: "out", legsA: 2, legsB: 2 };

/** Body cells as [column, row]; rows are two units tall. */
function bodyCells(pose: Pose): [number, number][] {
  const cells: [number, number][] = [];
  ART.forEach((row, r) => [...row].forEach((ch, c) => ch === "#" && c > 1 && c < 16 && cells.push([c, r])));
  for (const [c, arm] of [[1, pose.left], [16, pose.right]] as const)
    cells.push(...(arm === "out" ? [[c, 2]] : [[c, 1], [c, 0]]) as [number, number][]);
  return cells;
}

function drawClawd(svg: SVGSVGElement, pose: Pose): void {
  const rect = (x: number, y: number, w: number, h: number, fill: string) => {
    const r = document.createElementNS(NS, "rect");
    for (const [k, v] of Object.entries({ x, y, width: w, height: h, fill })) r.setAttribute(k, String(v));
    return r;
  };
  const parts = bodyCells(pose).map(([c, r]) => rect(c, r * 2, 1, 2, ORANGE));
  LEGS.forEach((c, i) => {
    const h = i % 2 ? pose.legsB : pose.legsA;
    // A lifted leg is a shorter one, still joined to the body.
    parts.push(rect(c, 8, 1, h, ORANGE));
  });
  // Open eyes are the dark gaps of the banner; shut, the gap fills in and leaves a line at the bottom.
  for (const c of EYES) {
    if (pose.eyes === "open") parts.push(rect(c, 2, 1, 2, "#1b1b1f"));
    else parts.push(rect(c, 2, 1, 2, ORANGE), rect(c - 0.15, 3.4, 1.3, 0.6, "#1b1b1f"));
  }
  svg.replaceChildren(...parts);
}

/** A short burst of poses, each held for its milliseconds. */
type Act = [Pose, number][];
const ACTS: Record<string, Act> = {
  blink: [[{ ...STAND, eyes: "shut" }, 140]],
  doubleBlink: [[{ ...STAND, eyes: "shut" }, 120], [STAND, 160], [{ ...STAND, eyes: "shut" }, 120]],
  wave: [0, 1, 2].flatMap((): Act => [[{ ...STAND, right: "up" }, 260], [STAND, 200]]),
  cheer: [[{ ...STAND, left: "up", right: "up", eyes: "shut" }, 700]],
  tap: [0, 1, 2, 3].map((i): [Pose, number] => [{ ...STAND, legsA: i % 2 ? 2 : 1, legsB: i % 2 ? 1 : 2 }, 180]),
  work: [0, 1, 2, 3].map((i): [Pose, number] => [
    { ...STAND, legsA: i % 2 ? 2 : 1, legsB: i % 2 ? 1 : 2, left: i % 2 ? "out" : "up", right: i % 2 ? "up" : "out" },
    200,
  ]),
};
const IDLE_ACTS = ["blink", "blink", "doubleBlink", "wave", "tap", "cheer"];
const BUSY_ACTS = ["work", "tap", "blink", "work", "wave"];

const pick = <T>(xs: readonly T[]): T => xs[Math.floor(Math.random() * xs.length)];

/** One Clawd for the app, kept across redraws of the footer so its timing is not reset every half second. */
let clawdBox: HTMLElement | null = null;

function clawd(): HTMLElement {
  if (clawdBox) return clawdBox;
  const box = document.createElement("span");
  box.className = "mascot mascot-claude";
  box.setAttribute("aria-hidden", "true");
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 18 10");
  svg.setAttribute("shape-rendering", "crispEdges");
  box.append(svg);
  drawClawd(svg, STAND);
  const still = window.matchMedia("(prefers-reduced-motion: reduce)");
  const play = async () => {
    const busy = box.dataset.busy === "1";
    if (!still.matches && document.hasFocus()) {
      for (const [pose, ms] of ACTS[pick(busy ? BUSY_ACTS : IDLE_ACTS)]) {
        drawClawd(svg, pose);
        await new Promise((resolve) => setTimeout(resolve, ms));
      }
      drawClawd(svg, STAND);
    }
    // Now and then: every 3-7 s while Claude works, every 7-15 s otherwise.
    setTimeout(play, busy ? 3000 + Math.random() * 4000 : 7000 + Math.random() * 8000);
  };
  setTimeout(play, 2000);
  clawdBox = box;
  return box;
}

/** `busy`: that side is working; only Clawd shows it, by moving more often. */
export function mascot(kind: MascotKind, busy = false): HTMLElement {
  if (kind === "claude") {
    const c = clawd();
    c.dataset.busy = busy ? "1" : "0";
    return c;
  }
  const box = document.createElement("span");
  box.className = `mascot mascot-${kind}`;
  box.setAttribute("aria-hidden", "true");
  box.append(vendorIcon(kind, "mascot-art"));
  return box;
}
