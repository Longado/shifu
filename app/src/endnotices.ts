import { reportError } from "./store";

/** Each requested end has its own five-second grace period; hovering never extends it. */
export class EndNotices {
  readonly element = document.createElement("div");
  private readonly pending = new Set<string>();

  constructor(private readonly end: (name: string) => Promise<void>, private readonly cancelled: () => void) {
    this.element.className = "end-notices";
    this.element.hidden = true;
  }

  schedule(name: string, label: string): void {
    if (this.pending.has(name)) return;
    const notice = document.createElement("div");
    notice.className = "undo-close";
    const message = document.createElement("span");
    message.setAttribute("role", "status");
    const undo = document.createElement("button");
    undo.textContent = "撤销";
    undo.title = `取消结束 ${label}`;
    notice.append(message, undo);
    this.element.append(notice);
    this.element.hidden = false;

    const deadline = Date.now() + 5000;
    const paint = () => {
      message.textContent = `${Math.max(1, Math.ceil((deadline - Date.now()) / 1000))} 秒后结束 ${label}`;
    };
    paint();
    const clock = window.setInterval(paint, 250);
    const remove = () => {
      window.clearTimeout(timer);
      window.clearInterval(clock);
      this.pending.delete(name);
      notice.remove();
      this.element.hidden = this.pending.size === 0;
    };
    const timer = window.setTimeout(() => {
      remove();
      void this.end(name).catch(reportError);
    }, 5000);
    this.pending.add(name);
    undo.addEventListener("click", () => {
      remove();
      this.cancelled();
    });
  }
}
