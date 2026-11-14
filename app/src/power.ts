import { listen } from "@tauri-apps/api/event";
import { powerGet, powerSet, powerTimer, powerLid, powerLidSetup, type PowerMode, type PowerStatus } from "./api";
import { showMenu, type MenuItem } from "./menu";
import { reportError } from "./store";

const MODES: { mode: PowerMode; label: string }[] = [
  { mode: "off", label: "正常睡眠" },
  { mode: "system", label: "保持运行" },
  { mode: "display", label: "屏幕常亮" },
];
const TIMERS = [null, 30, 60, 120, 240, 480];

export class PowerButton {
  readonly element = document.createElement("button");
  private status: PowerStatus | null = null;
  private busy = false;
  private readonly remaining = document.createElement("span");

  constructor() {
    const button = this.element;
    button.className = "icon-btn power-button";
    button.setAttribute("aria-haspopup", "menu");
    button.innerHTML = '<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6.5h9v5a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3zM12 7h1.5a2 2 0 0 1 0 4H12M2 16h12"/><path class="cup-steam" d="M5 4V2M8 4V1.5M11 4V2"/></svg>';
    this.remaining.className = "power-remaining";
    this.remaining.setAttribute("aria-hidden", "true");
    button.append(this.remaining);
    button.disabled = true;
    button.title = "正在读取 Steamcup 状态";
    button.setAttribute("aria-label", button.title);
    void listen<PowerStatus>("power-changed", ({ payload }) => {
      this.status = payload;
      this.render();
    }).catch(reportError);
    void powerGet().then((status) => {
      this.status = status;
      this.render();
      button.disabled = false;
    }).catch((e) => {
      button.title = "Steamcup 状态读取失败";
      button.setAttribute("aria-label", button.title);
      reportError(e);
    });
    button.addEventListener("click", () => this.showModes());
    // Display only: native timers and recovery work while the window is hidden.
    window.setInterval(() => { if (this.status?.until) this.render(); }, 1000);
  }

  private showModes(): void {
    if (!this.status || this.busy) return;
    const status = this.status;
    const rect = this.element.getBoundingClientRect();
    const items: MenuItem[] = MODES.map(({ mode, label }) => ({
      label: `${status.mode === mode ? "✓ " : ""}${label}`,
      run: () => this.change(() => powerSet(mode)),
    }));
    items.push({ label: status.until ? `定时 ${this.remaining.textContent}` : "定时", run: () => this.showTimers() });
    const enabling = !status.lid.enabled || !status.lid.ready;
    const needsSetup = enabling && !status.lid.ready;
    items.push({
      label: `${status.lid.enabled && status.lid.ready ? "✓ " : ""}合盖运行${needsSetup ? "…" : ""}`,
      hint: status.lid.error || (status.lid.paused === "hot" ? "过热暂停，冷却后恢复" : status.lid.paused === "low_battery" ? "电量低，接电后恢复" : undefined),
      confirmLabel: needsSetup ? "授权并开启合盖运行" : undefined,
      confirmHint: needsSetup ? "添加电源权限和恢复任务，需管理员密码" : undefined,
      run: () => this.change(async () => {
        if (needsSetup) await powerLidSetup();
        return powerLid(enabling);
      }),
    });
    showMenu(rect.right, rect.top, items);
  }

  private showTimers(): void {
    const rect = this.element.getBoundingClientRect();
    showMenu(rect.right, rect.top, TIMERS.map((minutes) => ({
      label: `${this.status?.mode !== "off" && this.status?.minutes === minutes ? "✓ " : ""}${minutes === null ? "持续保持" : minutes < 60 ? `${minutes} 分钟` : `${minutes / 60} 小时`}`,
      run: () => this.change(() => powerTimer(minutes)),
    })));
  }

  private async change(run: () => Promise<PowerStatus>): Promise<void> {
    this.busy = true;
    this.element.disabled = true;
    try {
      this.status = await run();
      this.render();
    } catch (e) {
      // A setting may have changed before an error: read back the backend's state.
      try { this.status = await powerGet(); this.render(); } catch { /* Keep the last known state and action error. */ }
      this.element.title = `Steamcup 更改失败：${String(e)}`;
      this.element.setAttribute("aria-label", this.element.title);
      reportError(e);
    } finally {
      this.busy = false;
      this.element.disabled = false;
    }
  }

  private render(): void {
    if (!this.status) return;
    const s = this.status;
    const label = MODES.find((m) => m.mode === s.mode)!.label;
    const minutes = s.until ? Math.max(0, Math.ceil((s.until - Date.now()) / 60_000)) : 0;
    this.remaining.hidden = !s.until;
    this.remaining.textContent = s.until ? `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, "0")}` : "";
    const lid = s.lid.active ? "，合盖继续运行" : s.lid.paused === "hot" ? "，合盖运行因过热暂停" : s.lid.paused === "low_battery" ? "，合盖运行因低电量暂停" : "";
    this.element.title = `Steamcup：${label}${lid}${s.until ? `，剩余 ${this.remaining.textContent}` : ""}${s.lid.error ? `，${s.lid.error}` : ""}`;
    this.element.setAttribute("aria-label", this.element.title);
    this.element.setAttribute("aria-pressed", String(s.mode !== "off" || s.lid.active));
    this.element.classList.toggle("power-lid-active", s.lid.active);
  }
}
