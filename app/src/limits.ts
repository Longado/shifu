/** Plan limits for the sidebar footer (APP-SPEC-B3). The numbers come from `limits.rs`; this file decides what to say. */

/** Mirrors `Window` / `VendorLimits` / `Limits` in app/src-tauri/src/limits.rs. */
export interface LimitWindow {
  used_percent: number;
  window_minutes: number;
  /** Unix seconds. */
  resets_at: number;
}
export interface VendorLimits {
  windows: LimitWindow[];
  credits: string | null;
  /** Unix seconds: when the data was recorded. */
  updated_at: number;
}
/** Mirrors `CodeBuddyUse` in limits.rs: charged turns as [Unix ms, credits], and the free model's quota. */
export interface CodeBuddyUse {
  spends: [number, number][];
  free: { used_percent: number; resets_at: number } | null;
}
export interface Limits {
  claude: VendorLimits | null;
  codex: VendorLimits | null;
  kimi: VendorLimits | null;
  codebuddy: CodeBuddyUse | null;
}
export type Vendor = "claude" | "codex" | "kimi";

/** A percentage at or above this is shown in the accent colour. Picked by hand. */
export const ACCENT_AT = 90;

export interface LimitSegment {
  label: string;
  /** Shown after the label; drawn in the accent colour when `hot`. */
  value?: string;
  hot?: boolean;
  /** Set on a window's segment (0..100, 0 once reset): drawn as a bar. Notes like the reset time have none. */
  fill?: number;
  /** The credits balance once a window is used up: drawn as a row under the bars, with a note instead of a bar. */
  credits?: true;
}
export interface LimitLine {
  vendor: Vendor;
  label: string;
  segments: LimitSegment[];
  /** Everything: each window's use, full reset times, when the data was recorded. */
  title: string;
}

const VENDOR_NAMES: Record<Vendor, string> = { claude: "Claude", codex: "Codex", kimi: "Kimi" };
const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

/** Claude and Kimi call the long window "7天", Codex "本周"; all have a 5-hour one. */
export function windowName(minutes: number, vendor: Vendor): string {
  if (minutes === 300) return "5小时";
  if (minutes === 10080) return vendor === "codex" ? "本周" : "7天";
  return `${minutes}分钟`;
}

export const isReset = (w: LimitWindow, now: number) => w.resets_at <= now;
export const isHot = (w: LimitWindow) => w.used_percent >= ACCENT_AT;

const pad = (n: number) => String(n).padStart(2, "0");
const clock = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** "13:11" within today (local time), otherwise "周一 13:11". Both are Unix seconds. */
export function resetText(resetsAt: number, now: number): string {
  const at = new Date(resetsAt * 1000);
  const today = new Date(now * 1000);
  const sameDay = at.getFullYear() === today.getFullYear() && at.getMonth() === today.getMonth() && at.getDate() === today.getDate();
  return sameDay ? clock(at) : `${WEEKDAYS[at.getDay()]} ${clock(at)}`;
}

/** How long until a reset: "2天15小时后重置", "1小时25分后重置", "20分钟后重置". The exact time is in the
 *  hover title. */
export function untilText(resetsAt: number, now: number): string {
  const left = Math.max(0, resetsAt - now);
  const days = Math.floor(left / 86400);
  const hours = Math.floor((left % 86400) / 3600);
  const minutes = Math.floor((left % 3600) / 60);
  if (days > 0) return hours > 0 ? `${days}天${hours}小时后重置` : `${days}天后重置`;
  if (hours > 0) return minutes > 0 ? `${hours}小时${minutes}分后重置` : `${hours}小时后重置`;
  return `${Math.max(1, Math.ceil(left / 60))}分钟后重置`;
}

const fullTime = (secs: number) => {
  const d = new Date(secs * 1000);
  return `${d.getMonth() + 1}月${d.getDate()}日 ${WEEKDAYS[d.getDay()]} ${clock(d)}`;
};

/** Whole credits: "29855.1005840000" -> "29,855". Null when the balance is not a number. */
export function creditsText(balance: string): string | null {
  const n = Number(balance);
  return Number.isFinite(n) ? Math.round(n).toLocaleString("en-US") : null;
}

/** One vendor's line. A window whose reset time has passed shows "已重置" instead of its old percentage (the file
 *  still says 100% a day after the quota came back). */
export function limitLine(vendor: Vendor, data: VendorLimits, now: number): LimitLine {
  const segments: LimitSegment[] = [];
  const details: string[] = [];
  for (const w of data.windows) {
    const name = windowName(w.window_minutes, vendor);
    if (isReset(w, now)) {
      segments.push({ label: name, value: "已重置", fill: 0 });
      details.push(`${name}：已重置（${fullTime(w.resets_at)}）`);
    } else {
      segments.push({
        label: name,
        value: `${Math.round(w.used_percent)}%`,
        hot: isHot(w),
        fill: Math.max(0, Math.min(100, w.used_percent)),
      });
      details.push(`${name}：已用 ${Math.round(w.used_percent)}%，${fullTime(w.resets_at)} 重置`);
    }
  }
  const live = data.windows.filter((w) => !isReset(w, now));
  const credits = data.credits === null ? null : creditsText(data.credits);
  if (credits !== null && live.some((w) => w.used_percent >= 100)) {
    segments.push({ label: "credits", value: credits, credits: true });
    details.push(`credits 余额 ${credits}`);
  }
  if (live.length > 0) {
    const next = Math.min(...live.map((w) => w.resets_at));
    segments.push({ label: untilText(next, now) });
  }
  details.push(`数据记于 ${resetText(data.updated_at, now)}`);
  return { vendor, label: VENDOR_NAMES[vendor], segments, title: details.join("\n") };
}

/** CodeBuddy's rows: the free model's quota while it has not reset, then the credits spent since local midnight
 *  Nothing when there is neither. */
export function codebuddySegments(use: CodeBuddyUse, now: number): LimitSegment[] {
  const segments: LimitSegment[] = [];
  const free = use.free;
  if (free && free.resets_at > now) {
    segments.push({ label: "免费", value: `${Math.round(free.used_percent)}%`, hot: free.used_percent >= ACCENT_AT, fill: Math.max(0, Math.min(100, free.used_percent)) });
  }
  const midnight = new Date(now * 1000);
  midnight.setHours(0, 0, 0, 0);
  const spent = use.spends.filter(([at]) => at >= midnight.getTime()).reduce((sum, [, credit]) => sum + credit, 0);
  if (spent > 0) segments.push({ label: "今天", value: `${spent.toLocaleString("en-US", { maximumFractionDigits: 1 })} 积分`, credits: true });
  return segments;
}

/** The lines to show: a vendor without data has none. */
export function limitLines(limits: Limits, now: number): LimitLine[] {
  const lines: LimitLine[] = [];
  if (limits.claude) lines.push(limitLine("claude", limits.claude, now));
  if (limits.codex) lines.push(limitLine("codex", limits.codex, now));
  if (limits.kimi) lines.push(limitLine("kimi", limits.kimi, now));
  return lines;
}
