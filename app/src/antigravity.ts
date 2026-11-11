import { ACCENT_AT, resetText, type LimitSegment } from "./limits";

export interface AntigravityUsage {
  groups: { name: string; buckets: { name: string; window: string; used_percent: number | null; reset_time: string | null }[] }[];
  updated_at: number;
  error: string | null;
}
export interface AntigravityLine {
  key: string;
  label: string;
  segments: LimitSegment[];
  title: string;
  message?: string;
}
export function antigravityLines(usage: AntigravityUsage, now: number): AntigravityLine[] {
  if (!usage.groups.length) {
    const message = usage.error || "正在读取额度";
    return [{ key: "antigravity", label: "Antigravity", segments: [], title: message, message }];
  }
  const stale = !!usage.error || now - usage.updated_at > 600;
  return usage.groups.map((group) => {
    const family = group.name === "Gemini Models" ? "Gemini" : group.name === "Claude and GPT models" ? "Claude/GPT" : group.name;
    const title = ["Antigravity：显示已用额度", `更新于 ${new Date(usage.updated_at * 1000).toLocaleTimeString()}`];
    if (stale) title.push(usage.error || "额度尚未更新");
    const segments: LimitSegment[] = group.buckets.map((bucket) => {
      const label = bucket.window === "weekly" ? "周" : bucket.window === "5h" ? "5h" : bucket.name;
      const reset = bucket.reset_time ? Date.parse(bucket.reset_time) / 1000 : NaN;
      const expired = Number.isFinite(reset) && reset <= now;
      const known = bucket.used_percent !== null;
      let detail = `${label}：${known ? `已用 ${Math.round(bucket.used_percent!)}%，剩余 ${Math.round(100 - bucket.used_percent!)}%` : "暂无数据"}`;
      if (Number.isFinite(reset)) detail += `；${new Date(reset * 1000).toLocaleDateString()} ${resetText(reset, now)} 重置`;
      if (expired) detail += "；等待刷新";
      title.push(detail);
      if (expired || !known) return { label, value: expired ? "待更新" : "暂无数据" };
      return { label, value: `${Math.round(bucket.used_percent!)}%`, fill: bucket.used_percent!, hot: !stale && bucket.used_percent! >= ACCENT_AT };
    });
    return { key: `antigravity:${group.name}`, label: `Antigravity ${family}${stale ? "（旧数据）" : ""}`, segments, title: title.join("\n") };
  });
}
