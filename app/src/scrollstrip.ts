/**
 * The scroll bar in the right gutter of a terminal.
 *
 * Claude and Codex draw full screen and scroll their own conversation; they tell the terminal neither how long it
 * is nor where the view sits. So the bar cannot show a true position. It shows how far the view was scrolled back
 * from the latest output, counted in the wheel notches that went through this terminal: the thumb rests at the
 * bottom while following, lifts as the view scrolls back, and stops at the top of the track however long the
 * conversation is. Dragging it sends the same wheel input a mouse does, so every program that scrolls with the
 * wheel scrolls with the bar.
 */

/** One wheel notch per this many pixels of thumb travel. */
export const PX_PER_NOTCH = 3;
const THUMB_PX = 40;
/** Room kept at the foot of the track for the back-to-latest arrow. */
const FOOT_PX = 14;

/** The SGR wheel report xterm.js sends for a turn over this cell (the form tmux asks for). */
export const wheelReport = (up: boolean, col: number, row: number): string => `\x1b[<${up ? 64 : 65};${col};${row}M`;

/** +1 for a wheel-up report, -1 for wheel-down, 0 for anything else. */
export function wheelNotches(data: string): number {
  const m = /^\x1b\[<(\d+);\d+;\d+M$/.exec(data);
  const button = m ? Number(m[1]) : -1;
  return button === 64 ? 1 : button === 65 ? -1 : 0;
}

export const scrolledBack = (now: number, notches: number, max: number): number => Math.min(max, Math.max(0, now + notches));

/** Where the thumb's top edge sits in a track with `range` pixels of travel. */
export const thumbTop = (back: number, range: number): number => Math.max(0, range - back * PX_PER_NOTCH);

/** Whole notches in a drag of `px` (up is positive), and the pixels left over. */
export function dragNotches(px: number): { notches: number; rest: number } {
  const notches = Math.trunc(px / PX_PER_NOTCH);
  return { notches, rest: px - notches * PX_PER_NOTCH };
}

export interface ScrollActions {
  /** Send `count` wheel notches to the program. */
  wheel(up: boolean, count: number): void;
  /** Go back to the latest output. */
  latest(back: number): void;
  /** Notches in one click on the track. */
  page(): number;
}

export class ScrollStrip {
  readonly element = document.createElement("div");
  private readonly thumb = document.createElement("div");
  private readonly latest = document.createElement("button");
  private back = 0;

  constructor(private readonly actions: ScrollActions) {
    this.element.className = "term-scroll";
    this.element.setAttribute("role", "scrollbar");
    this.element.setAttribute("aria-orientation", "vertical");
    this.element.setAttribute("aria-label", "滚动对话");
    this.thumb.className = "term-scroll-thumb";
    this.thumb.title = "拖动翻看之前的内容";
    this.latest.className = "term-scroll-latest";
    this.latest.type = "button";
    this.latest.title = "回到最新";
    this.latest.setAttribute("aria-label", "回到最新");
    this.element.append(this.thumb, this.latest);
    // Keep the keyboard in the terminal: nothing here takes focus.
    this.element.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      if (e.target === this.element) this.pageTowards(e.clientY);
    });
    this.thumb.addEventListener("pointerdown", (e) => this.drag(e));
    this.latest.addEventListener("click", () => {
      this.actions.latest(this.back);
      this.reset();
    });
    this.render();
  }

  /** A wheel report went to the program, from the wheel or from this bar. */
  noteWheel(notches: number): void {
    if (!notches) return;
    this.back = scrolledBack(this.back, notches, this.max());
    this.render();
  }

  /** The program went back to its latest output on its own (a message was sent, scroll mode was left). */
  reset(): void {
    if (this.back === 0) return;
    this.back = 0;
    this.render();
  }

  /** The terminal changed size. */
  layout(): void {
    this.back = Math.min(this.back, this.max());
    this.render();
  }

  private range(): number {
    return Math.max(0, this.element.clientHeight - THUMB_PX - FOOT_PX);
  }

  private max(): number {
    return Math.floor(this.range() / PX_PER_NOTCH);
  }

  private render(): void {
    this.thumb.style.transform = `translateY(${thumbTop(this.back, this.range())}px)`;
    this.element.classList.toggle("scrolled", this.back > 0);
    this.element.setAttribute("aria-valuenow", String(this.back));
    this.latest.hidden = this.back === 0;
  }

  private pageTowards(clientY: number): void {
    const thumb = this.thumb.getBoundingClientRect();
    this.actions.wheel(clientY < thumb.top, this.actions.page());
  }

  private drag(start: PointerEvent): void {
    start.preventDefault();
    start.stopPropagation();
    this.thumb.setPointerCapture(start.pointerId);
    this.element.classList.add("dragging");
    let last = start.clientY;
    let rest = 0;
    const move = (e: PointerEvent) => {
      const step = dragNotches(rest + (last - e.clientY));
      last = e.clientY;
      rest = step.rest;
      if (step.notches) this.actions.wheel(step.notches > 0, Math.abs(step.notches));
    };
    const end = () => {
      this.element.classList.remove("dragging");
      this.thumb.removeEventListener("pointermove", move);
      this.thumb.removeEventListener("pointerup", end);
      this.thumb.removeEventListener("pointercancel", end);
    };
    this.thumb.addEventListener("pointermove", move);
    this.thumb.addEventListener("pointerup", end);
    this.thumb.addEventListener("pointercancel", end);
  }
}
