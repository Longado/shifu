import { listen } from "@tauri-apps/api/event";
import { limitsGet } from "./api";
import { codebuddySegments, type LimitLine, type LimitSegment, type Limits, type Vendor, limitLines } from "./limits";
import { mascot } from "./mascots";
import { antigravityLines } from "./antigravity";
import { getAgents, reportError, subscribe } from "./store";

const REFRESH_MS = 15000;
/** How long each vendor stays on the rotating line. Picked by hand. */
const TURN_MS = 4000;

/** The usage lines at the bottom of the sidebar: Claude's, Codex's and Kimi's limits, then CodeBuddy's free quota or
 *  credits today. What each agent is doing is in the sidebar above, not here. Folded , it is
 *  one line that turns through the vendors, holding still under the pointer; a vendor at ACCENT_AT or more stays out
 *  of the turn and shows all the time. A click unfolds every vendor, another folds it again. A window that has reset must stop
 *  showing its old number even if no new data arrives, so it redraws on a timer as well as on every update. */
export class LimitsBar {
  readonly element = document.createElement("div");
  private limits: Limits = { claude: null, codex: null, kimi: null, codebuddy: null, antigravity: { groups: [], updated_at: 0, error: null } };
  /** The vendor on the rotating line, by name: the next one is the one after it, even if a vendor came or went. */
  private current: string | null = null;
  private unfolded = false;
  private pointed = false;

  constructor() {
    this.element.className = "limits";
    this.element.hidden = true;
    void listen<Limits>("limits", ({ payload }) => this.set(payload)).catch(reportError);
    limitsGet().then((l) => this.set(l)).catch(reportError);
    window.setInterval(() => this.render(), REFRESH_MS);
    window.setInterval(() => {
      if (this.unfolded || this.pointed) return;
      this.render(true);
    }, TURN_MS);
    this.element.addEventListener("pointerenter", () => (this.pointed = true));
    this.element.addEventListener("pointerleave", () => (this.pointed = false));
    this.element.addEventListener("click", () => {
      this.unfolded = !this.unfolded;
      this.render();
    });
    // Clawd moves more often as soon as a Claude agent starts running; the CodeBuddy and Kimi rows follow theirs.
    subscribe(() => this.render());
  }

  private set(limits: Limits): void {
    this.limits = limits;
    this.render();
  }

  /** `turned`: the rotating line moves on to the next vendor, which slides in. */
  private render(turned = false): void {
    const now = Math.floor(Date.now() / 1000);
    const running = (kind: string) => getAgents().some((a) => a.kind === kind && a.status === "running");
    const blocks = limitLines(this.limits, now).map((line) => vendorBlock(line, running(line.vendor)));
    const codebuddy = this.limits.codebuddy ? codebuddySegments(this.limits.codebuddy, now) : [];
    blocks.push({ ...agentsBlock("codebuddy", "CodeBuddy", codebuddy), empty: codebuddy.length === 0 });
    if (!this.limits.kimi) blocks.push({ ...agentsBlock("kimi", "Kimi"), empty: true });
    for (const line of antigravityLines(this.limits.antigravity, now)) {
      const element = row(null, line.label, false, line.segments, line.message);
      element.classList.add("limit-antigravity");
      element.title = line.title;
      blocks.push({ key: line.key, element, hot: line.segments.some(s => s.hot) });
    }
    let shown = blocks;
    if (!this.unfolded) {
      const pinned = blocks.filter((b) => b.hot);
      const turning = blocks.filter((b) => !b.hot && !b.empty);
      const at = turning.findIndex((b) => b.key === this.current);
      const index = at < 0 ? 0 : turned ? (at + 1) % turning.length : at;
      const current = turning[index] ?? null;
      this.current = current?.key ?? null;
      if (current && turned && turning.length > 1) current.element.classList.add("turn-in");
      shown = current ? [...pinned, current] : pinned;
    }
    this.element.hidden = blocks.length === 0;
    this.element.classList.toggle("unfolded", this.unfolded);
    this.element.title = this.unfolded ? "点一下收起" : "点一下看全部";
    this.element.replaceChildren(...shown.map((b) => b.element));
  }
}

function el(tag: string, className: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** One line: the week first, then the 5 hours if the plan has them . Everything else is in the hover
 *  title. Only a window at ACCENT_AT or more adds when it comes back and the credits left, because that is when
 *  the next task should go elsewhere. */
interface Block {
  key: string;
  element: HTMLElement;
  /** Nearly used up: never turned away. */
  hot?: boolean;
  /** Nothing to say yet: left out of the turn. */
  empty?: boolean;
}

function vendorBlock(line: LimitLine, busy: boolean): Block {
  // The week first, then the 5 hours if the plan has them : the data lists the short window first.
  const bars = line.segments.filter((s) => s.fill !== undefined).reverse();
  const hot = bars.some((b) => b.hot);
  const block = row(line.vendor, line.label, busy, bars);
  block.title = line.title;
  if (hot) {
    // The reset first: it is the part that changes the plan; the credits balance under it.
    const notes = line.segments.filter((s) => s.fill === undefined).sort((a, b) => Number(!!a.credits) - Number(!!b.credits));
    for (const s of notes) block.append(el("div", "limit-more", s.credits ? `${s.label} ${s.value}` : s.label));
  }
  return { key: line.vendor, element: block, hot };
}

/** CodeBuddy has no plan limit: its free model's quota as the bar, or else the credits spent today. Kimi before its
 *  first answer has neither. */
function agentsBlock(kind: "codebuddy" | "kimi", label: string, segments: LimitSegment[] = []): Block {
  const bar = segments.find((s) => s.fill !== undefined) ?? null;
  const spent = segments.find((s) => s.credits);
  const block = row(kind, label, getAgents().some((a) => a.kind === kind && a.status === "running"), bar ? [bar] : [], spent ? `${spent.label} ${spent.value}` : "还没数据");
  block.title = segments.map((s) => `${s.label} ${s.value ?? ""}`).join("\n") || `还没有 ${label} 的用量`;
  return { key: kind, element: block, hot: bar?.hot };
}

/** Short names on the one line: "周" for the week (Claude and Kimi say 7天, Codex 本周), "5h" for five hours. */
const SHORT: Record<string, string> = { "7天": "周", 本周: "周", "5小时": "5h" };

/** "[mark] Claude      周 13%  5h 35%" ; with no window, the text
 *  alone. A window at ACCENT_AT or more is in the accent colour. */
function row(kind: Vendor | "codebuddy" | null, label: string, busy: boolean, bars: LimitSegment[], text = ""): HTMLElement {
  const block = el("div", "limit-vendor");
  const line = el("div", "limit-row");
  const name = el("span", "limit-label");
  if (kind) name.append(mascot(kind, busy));
  name.append(label);
  const values = el("span", "limit-text");
  if (bars.length === 0) values.textContent = text;
  bars.forEach((bar) => {
    values.append(el("span", bar.hot ? "limit-num hot" : "limit-num", `${SHORT[bar.label] ?? bar.label} ${bar.value}`));
  });
  line.append(name, values);
  block.append(line);
  return block;
}
