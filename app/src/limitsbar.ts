import { listen } from "@tauri-apps/api/event";
import { limitsGet } from "./api";
import { agentLabel } from "./format";
import { codebuddySegments, type LimitLine, type LimitSegment, type Limits, type Vendor, limitLines } from "./limits";
import { mascot } from "./mascots";
import { getAgents, reportError, subscribe } from "./store";
import { VENDOR_COLORS } from "./vendoricons";

const REFRESH_MS = 15000;
/** Kimi's mark is black, which would vanish on a dark footer; its bar takes the blue dot of the mark. */
const BAR_COLORS: Record<Vendor | "codebuddy", string> = {
  claude: VENDOR_COLORS.claude,
  codex: VENDOR_COLORS.codex,
  kimi: "#1d8cff",
  codebuddy: VENDOR_COLORS.codebuddy, // currentColor: the text colour, like its mark
};

/** The usage blocks at the bottom of the sidebar: Claude's, Codex's and Kimi's limits, then CodeBuddy: what Shifu's
 *  own CodeBuddy agents are doing, with its credits today and free quota under it (Kimi shows what it is doing too,
 *  until its first answer arrives). A window that has reset must stop
 *  showing its old number even if no new data arrives, so it redraws on a timer as well as on every update. */
export class LimitsBar {
  readonly element = document.createElement("div");
  private limits: Limits = { claude: null, codex: null, kimi: null, codebuddy: null };

  constructor() {
    this.element.className = "limits";
    this.element.hidden = true;
    void listen<Limits>("limits", ({ payload }) => this.set(payload)).catch(reportError);
    limitsGet().then((l) => this.set(l)).catch(reportError);
    window.setInterval(() => this.render(), REFRESH_MS);
    // Clawd moves more often as soon as a Claude agent starts running; the CodeBuddy and Kimi rows follow theirs.
    subscribe(() => this.render());
  }

  private set(limits: Limits): void {
    this.limits = limits;
    this.render();
  }

  private render(): void {
    const lines = limitLines(this.limits, Math.floor(Date.now() / 1000));
    const running = (kind: string) => getAgents().some((a) => a.kind === kind && a.status === "running");
    const blocks = lines.map((line) => vendorBlock(line, running(line.vendor)));
    const now = Math.floor(Date.now() / 1000);
    blocks.push(agentsBlock("codebuddy", "CodeBuddy", this.limits.codebuddy ? codebuddySegments(this.limits.codebuddy, now) : []));
    if (!this.limits.kimi) blocks.push(agentsBlock("kimi", "Kimi"));
    this.element.hidden = blocks.length === 0;
    this.element.replaceChildren(...blocks);
  }
}

function el(tag: string, className: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Whether Shifu's agents of this kind are working, and on what; usage rows under it when there are any. */
function agentsBlock(kind: Vendor | "codebuddy", label: string, segments: LimitSegment[] = []): HTMLElement {
  const working = getAgents()
    .filter((a) => a.kind === kind && a.status === "running")
    .map(agentLabel);
  const block = el("div", "limit-vendor");
  const head = el("div", "limit-head");
  const name = el("span", "limit-label");
  name.append(mascot(kind), label);
  const note = working.length === 0 ? "没在做事" : working.length === 1 ? `在做：${working[0]}` : `在做 ${working.length} 件`;
  head.append(name, el("span", "limit-note", note));
  block.title = working.join("\n") || `Shifu 里没有在跑的 ${label}`;
  block.append(head, ...usageRows(kind, segments));
  return block;
}

/** "Claude  23:30 重置", one bar per window ("5小时 ▬▬▬── 42%"), then "credits  29,855" if in use. */
function vendorBlock(line: LimitLine, busy: boolean): HTMLElement {
  const block = el("div", "limit-vendor");
  block.title = line.title;
  const head = el("div", "limit-head");
  const notes = line.segments.filter((s) => s.fill === undefined && !s.credits).map((s) => s.label);
  const name = el("span", "limit-label");
  name.append(mascot(line.vendor, busy), line.label);
  head.append(name, el("span", "limit-note", notes.join("  ")));
  block.append(head, ...usageRows(line.vendor, line.segments));
  return block;
}

/** A bar per window, then the credits rows. */
function usageRows(kind: Vendor | "codebuddy", segments: readonly LimitSegment[]): HTMLElement[] {
  const rows: HTMLElement[] = [];
  for (const seg of segments) {
    if (seg.fill === undefined) continue;
    const row = el("div", seg.hot ? "limit-row hot" : "limit-row");
    const track = el("span", "limit-track");
    const fill = el("span", "limit-fill");
    fill.style.width = `${seg.fill}%`;
    fill.style.background = BAR_COLORS[kind];
    track.append(fill);
    row.append(el("span", "limit-name", seg.label), track, el("span", "limit-value", seg.value));
    rows.push(row);
  }
  for (const seg of segments) {
    if (!seg.credits) continue;
    const row = el("div", "limit-row credits");
    row.append(el("span", "limit-name", seg.label), el("span", "limit-credit-gap"), el("span", "limit-value", seg.value));
    rows.push(row);
  }
  return rows;
}
