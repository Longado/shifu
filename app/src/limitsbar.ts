import { listen } from "@tauri-apps/api/event";
import { limitsGet, limitsRefresh } from "./api";
import { codebuddySegments, resetText, type LimitLine, type LimitSegment, type Limits, type Vendor, limitLines } from "./limits";
import { mascot } from "./mascots";
import { vendorIcon } from "./vendoricons";
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

  constructor(onExpand: () => void) {
    this.element.className = "limits";
    this.element.hidden = true;
    void listen<Limits>("limits", ({ payload }) => this.set(payload)).catch(reportError);
    limitsGet().then((l) => this.set(l)).catch(reportError);
    this.refresh();
    window.addEventListener("focus", () => this.refresh());
    document.addEventListener("visibilitychange", () => {
      if (!document.hidden) this.refresh();
    });
    window.setInterval(() => {
      if (!document.hidden && document.hasFocus()) this.refresh();
    }, 60_000);
    window.setInterval(() => this.render(), REFRESH_MS);
    window.setInterval(() => {
      if (this.unfolded || this.pointed) return;
      this.render(true);
    }, TURN_MS);
    this.element.addEventListener("pointerenter", () => (this.pointed = true));
    this.element.addEventListener("pointerleave", () => (this.pointed = false));
    this.element.addEventListener("click", () => {
      this.unfolded = !this.unfolded;
      if (this.unfolded) {
        onExpand();
        this.refresh();
      }
      this.render();
    });
    document.addEventListener("pointerdown", (event) => {
      if (event.target instanceof Node && !this.element.contains(event.target)) this.close();
    });
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") this.close();
    });
    // Clawd moves more often as soon as a Claude agent starts running; the CodeBuddy and Kimi rows follow theirs.
    subscribe(() => this.render());
  }

  close(): void {
    if (!this.unfolded) return;
    this.unfolded = false;
    this.render();
  }

  private refresh(): void {
    // The backend wakes its existing readers and coalesces requests to one per minute.
    void limitsRefresh().catch(reportError);
  }

  private set(limits: Limits): void {
    this.limits = limits;
    this.render();
  }

  /** `turned`: the rotating line moves on to the next vendor, which slides in. */
  private render(turned = false): void {
    const now = Math.floor(Date.now() / 1000);
    const running = (kind: string) => getAgents().some((a) => a.kind === kind && a.status === "running");
    const blocks = limitLines(this.limits, now).map((line) => vendorBlock(line, running(line.vendor), this.unfolded,
      line.vendor === "kimi" && this.limits.kimi?.error?.startsWith("Kimi 登录已过期") ? "待登录" : undefined));
    const codebuddy = this.limits.codebuddy ? codebuddySegments(this.limits.codebuddy, now) : [];
    blocks.push({ ...agentsBlock("codebuddy", "CodeBuddy", codebuddy, this.unfolded), empty: codebuddy.length === 0 });
    if (!this.limits.kimi) blocks.push({ ...agentsBlock("kimi", "Kimi", [], this.unfolded), empty: true });
    for (const line of antigravityLines(this.limits.antigravity, now)) {
      const element = row(null, line.label, false, line.segments, line.message, this.unfolded);
      element.classList.add("limit-antigravity");
      element.title = `${line.label.replace("（旧数据）", "")}\n${line.title}`;
      if (this.unfolded) element.setAttribute("aria-label", element.title);
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
    const children = shown.map((b) => b.element);
    if (this.unfolded) {
      const head = el("div", "limit-head");
      head.append(el("span", "", "用量"), el("span", "", "周"), el("span", "", "5h"));
      children.unshift(head);
      const footer = el("div", "limit-footer");
      const updated = Math.max(this.limits.claude?.updated_at ?? 0, this.limits.codex?.updated_at ?? 0,
        this.limits.kimi?.updated_at ?? 0, this.limits.antigravity.updated_at);
      footer.append(el("span", "limit-updated", updated > 0 ? `最近更新 ${resetText(updated, now)}` : "暂无更新"));
      const refresh = el("button", "limit-refresh", "刷新");
      refresh.setAttribute("aria-label", "刷新用量");
      refresh.title = `${updated > 0 ? `最近更新 ${resetText(updated, now)}。` : ""}查询最新额度，每家最多每分钟一次`;
      refresh.addEventListener("click", (event) => { event.stopPropagation(); this.refresh(); });
      footer.append(refresh);
      children.push(footer);
    }
    this.element.replaceChildren(...children);
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

function vendorBlock(line: LimitLine, busy: boolean, table: boolean, status?: string): Block {
  // The week first, then the 5 hours if the plan has them : the data lists the short window first.
  const bars = line.segments.filter((s) => s.fill !== undefined).reverse();
  const hot = bars.some((b) => b.hot);
  const block = row(line.vendor, line.label, busy, bars, line.message, table, status);
  block.title = line.title;
  if (table) block.setAttribute("aria-label", line.title);
  if (hot) {
    // The reset first: it is the part that changes the plan; the credits balance under it.
    const notes = line.segments.filter((s) => s.fill === undefined).sort((a, b) => Number(!!a.credits) - Number(!!b.credits));
    for (const s of notes) block.append(el("div", "limit-more", s.credits ? `${s.label} ${s.value}` : s.label));
  }
  return { key: line.vendor, element: block, hot };
}

/** CodeBuddy has no plan limit: its free model's quota as the bar, or else the credits spent today. Kimi before its
 *  first answer has neither. */
function agentsBlock(kind: "codebuddy" | "kimi", label: string, segments: LimitSegment[], table: boolean): Block {
  const bar = segments.find((s) => s.fill !== undefined) ?? null;
  const spent = segments.find((s) => s.credits);
  const block = row(kind, label, getAgents().some((a) => a.kind === kind && a.status === "running"), bar ? [bar] : [], spent ? `${spent.label} ${spent.value}` : "暂无数据", table);
  block.title = segments.map((s) => `${s.label} ${s.value ?? ""}`).join("\n") || `还没有 ${label} 的用量`;
  if (table) block.setAttribute("aria-label", block.title);
  return { key: kind, element: block, hot: bar?.hot };
}

/** Short names on the one line: "周" for the week (Claude and Kimi say 7天, Codex 本周), "5h" for five hours. */
const SHORT: Record<string, string> = { "7天": "周", 本周: "周", "5小时": "5h" };

/** "[mark] Claude      周 13%  5h 35%" ; with no window, the text
 *  alone. A window at ACCENT_AT or more is in the accent colour. */
function row(kind: Vendor | "codebuddy" | null, label: string, busy: boolean, bars: LimitSegment[], text = "", table = false, status?: string): HTMLElement {
  const block = el("div", "limit-vendor");
  if (table) {
    block.tabIndex = 0;
    block.setAttribute("role", "group");
  }
  const line = el("div", "limit-row");
  const name = el("span", "limit-label");
  if (kind) name.append(mascot(kind, busy));
  else if (table) {
    name.append(vendorIcon("antigravity", "limit-brand"));
  }
  const stale = label.endsWith("（旧数据）");
  name.append(el("span", "limit-name", table ? label.replace("（旧数据）", "") : label));
  if (table && (status || stale)) name.append(el("span", "limit-state", status || "旧数据"));
  block.classList.toggle("limit-stale", stale);
  if (table) {
    line.append(name);
    const week = bars.find((bar) => ["周", "本周", "7天"].includes(bar.label));
    const five = bars.find((bar) => ["5h", "5小时"].includes(bar.label));
    if (bars.length && bars.every((bar) => bar === week || bar === five)) {
      for (const [className, bar] of [["limit-week", week], ["limit-five", five]] as const) {
        line.append(el("span", `${className}${bar?.hot ? " hot" : ""}${bar ? "" : " limit-empty"}`, bar?.value ?? "—"));
      }
    } else {
      line.append(el("span", "limit-other", bars.length ? bars.map((bar) => `${bar.label} ${bar.value ?? ""}`).join("  ") : kind === "codebuddy" ? text : "—"));
    }
    block.append(line);
    return block;
  }
  const values = el("span", "limit-text");
  if (bars.length === 0) values.textContent = text;
  bars.forEach((bar) => {
    values.append(el("span", bar.hot ? "limit-num hot" : "limit-num", `${SHORT[bar.label] ?? bar.label} ${bar.value}`));
  });
  line.append(name, values);
  block.append(line);
  return block;
}
