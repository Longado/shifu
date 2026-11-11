import { describe, expect, it } from "vitest";
import { catchDroppedInput } from "./imeinput";

// Stand-ins for the terminal host and xterm.js's textarea; what matters is the order of events and the textarea text
// xterm.js later diffs. Run against the real xterm.js in WebKit: scripts/preview/ime-replay.cjs.
function setup() {
  const host = new EventTarget();
  const textarea = Object.assign(new EventTarget(), {
    value: "",
    selectionStart: 0,
    setSelectionRange(s: number) {
      this.selectionStart = s;
    },
  });
  const out: string[] = [];
  const typed = catchDroppedInput(host as HTMLElement, textarea as unknown as HTMLTextAreaElement, (d) => out.push(d));
  const keydown = (keyCode: number) => host.dispatchEvent(Object.assign(new Event("keydown"), { keyCode }));
  // The browser puts the text into the textarea, then fires input.
  const input = (data: string, isComposing = false) => {
    const at = textarea.selectionStart;
    textarea.value = textarea.value.slice(0, at) + data + textarea.value.slice(at);
    textarea.selectionStart = at + data.length;
    textarea.dispatchEvent(Object.assign(new Event("input"), { inputType: "insertText", isComposing, data }));
  };
  // xterm.js typing an ordinary key on its keypress.
  const keypress = (d: string) => {
    out.push(d);
    typed.sent();
  };
  return { out, textarea, keydown, input, keypress };
}

describe("catchDroppedInput", () => {
  it("types a character xterm.js drops (Shift+letter under Pinyin: no keypress, no 229)", () => {
    const t = setup();
    t.keydown(65);
    t.input("A");
    expect(t.out).toEqual(["A"]);
  });

  it("leaves an ordinary key to xterm.js", () => {
    const t = setup();
    t.keydown(53);
    t.keypress("5");
    t.input("5");
    expect(t.out).toEqual(["5"]);
  });

  // 11-10 用户 typed example51759 and got example55175599.
  it("types input-method keys once each and leaves nothing for xterm.js's textarea diff, even when they pile up", () => {
    const t = setup();
    t.textarea.value = "example";
    t.textarea.selectionStart = 7;
    for (const d of ["5", "1", "7", "5", "9"]) {
      t.keydown(229);
      t.input(d);
    }
    expect(t.out.join("")).toBe("51759");
    expect(t.textarea.value).toBe("example");
  });

  it("leaves composed text to xterm.js, also when WebKit's last input comes after compositionend", () => {
    const t = setup();
    t.keydown(229);
    t.input("ni", true);
    t.keydown(229); // space picks 你
    t.textarea.dispatchEvent(new Event("compositionend"));
    t.input("你");
    expect(t.out).toEqual([]);
  });
});
