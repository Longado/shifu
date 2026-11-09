/**
 * With Apple's Pinyin input method on, Shift+letter or Shift+digit is committed straight away: a keydown that is not
 * the IME's 229, no keypress, then an `input` event with the text. xterm.js only types on keypress and skips an
 * `input` that follows a keydown, so those characters never arrived.
 * This sends the text of such an `input` when nothing was typed since the key went down.
 */
export function catchDroppedInput(host: HTMLElement, textarea: HTMLTextAreaElement, send: (data: string) => void): { sent: () => void } {
  let typed = true;
  // Capture on the host runs before xterm.js's own keydown handling on the textarea.
  host.addEventListener("keydown", () => (typed = false), true);
  textarea.addEventListener("input", (e) => {
    const ev = e as InputEvent;
    if (ev.inputType !== "insertText" || ev.isComposing || !ev.data) return;
    const data = ev.data;
    // After xterm.js's own deferred handling of an IME keydown (it compares the textarea a tick later).
    setTimeout(() => {
      if (!typed) send(data);
    }, 0);
  });
  return { sent: () => (typed = true) };
}
