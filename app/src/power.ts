import { powerGet, powerSet, type PowerMode } from "./api";
import { showMenu } from "./menu";
import { reportError } from "./store";

const MODES: { mode: PowerMode; label: string; hint: string }[] = [
  { mode: "off", label: "正常睡眠", hint: "取消 Shifu 的保持唤醒，其他应用不受影响" },
  { mode: "system", label: "保持运行，允许息屏", hint: "合盖仍按系统设置睡眠；退出 Shifu 后恢复" },
  { mode: "display", label: "保持屏幕开启", hint: "电脑和屏幕保持唤醒；退出 Shifu 后恢复" },
];

export class PowerButton {
  readonly element = document.createElement("button");
  private mode: PowerMode = "off";

  constructor() {
    const button = this.element;
    button.className = "icon-btn power-button";
    button.setAttribute("aria-haspopup", "menu");
    button.innerHTML = '<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6.5h9v5a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3zM12 7h1.5a2 2 0 0 1 0 4H12M2 16h12"/><path class="cup-steam" d="M5 4V2M8 4V1.5M11 4V2"/></svg>';
    button.disabled = true;
    button.title = "正在读取保持唤醒状态";
    button.setAttribute("aria-label", button.title);
    void powerGet().then((mode) => {
      this.mode = mode;
      this.render();
      button.disabled = false;
    }).catch((e) => {
      button.title = "保持唤醒状态读取失败";
      button.setAttribute("aria-label", button.title);
      reportError(e);
    });
    button.addEventListener("click", () => {
      const rect = button.getBoundingClientRect();
      showMenu(rect.right, rect.top, MODES.map(({ mode, label, hint }) => ({
        label: `${this.mode === mode ? "✓ " : ""}${label}`,
        hint,
        run: async () => {
          button.disabled = true;
          try {
            this.mode = await powerSet(mode);
            this.render();
          } catch (e) {
            button.title = `更改失败：${String(e)}`;
            button.setAttribute("aria-label", button.title);
            reportError(e);
          }
          finally { button.disabled = false; }
        },
      })));
    });
  }

  private render(): void {
    const label = MODES.find((m) => m.mode === this.mode)!.label;
    this.element.title = `保持唤醒：${label}`;
    this.element.setAttribute("aria-label", this.element.title);
    this.element.setAttribute("aria-pressed", String(this.mode !== "off"));
  }
}
