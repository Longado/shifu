/**
 * Text an input method commits without a keypress, which xterm.js would drop or type twice.
 *
 * With Apple's Pinyin input method on, Shift+letter or Shift+digit is committed straight away: a keydown that is not
 * the IME's 229, no keypress, then an `input` event with the text. xterm.js only types on keypress and skips an
 * `input` that follows a keydown, so those characters never arrived (11-09: "shift 加任何都没反应").
 *
 * Digits and punctuation under the input method come as a 229 keydown and an `input`; xterm.js types them a tick
 * later by diffing the textarea against its value at keydown. When keys pile up (a busy page runs keydowns ahead of
 * timers), the first diff already holds the later keys and each later diff types its own key again (11-10:
 * example51759 came out as example55175599).
 *
 * Recorded in Shifu (11-10), Apple Pinyin punctuation comes the other way round: `input` first, then the 229
 * keydown. xterm.js types that `input` itself (no key down yet), and the keydown after it starts a diff that a quickly
 * following character lands in, so that character went out twice; the old timer check here, reset by that late
 * keydown, sent the punctuation itself twice too (，，。。).
 *
 * So such an `input` is typed here at once if nobody has typed it, and either way its text is taken back out of the
 * textarea, which leaves xterm.js's diff empty. Decided at the event, not on a timer. Composed text (Chinese) is left
 * to xterm.js, which reads it from the textarea.
 */
export function catchDroppedInput(host: HTMLElement, textarea: HTMLTextAreaElement, send: (data: string) => void): { sent: () => void } {
  // Whether this key has been typed already (keypress, or this catcher).
  let handled = true;
  // A committed composition xterm.js has yet to type (it waits a tick): text typed here meanwhile waits behind it,
  // and so does anything typed here after that, until the wait is over.
  let compositionPending = false;
  let waiting: string[] | null = null;
  // Things typed so far, and how many there were when the current `input` began.
  let sends = 0;
  let sendsAtInput = 0;
  // Capture on the host runs before xterm.js's own keydown and input handling on the textarea.
  host.addEventListener("keydown", () => (handled = false), true);
  host.addEventListener("input", () => (sendsAtInput = sends), true);
  // xterm.js types a committed composition itself; WebKit may send its last `input` after compositionend.
  textarea.addEventListener("compositionend", () => {
    handled = true;
    compositionPending = true;
  });
  // Registered after xterm.js's own input listener, so anything it types for this event has already gone through.
  textarea.addEventListener("input", (e) => {
    const ev = e as InputEvent;
    if (ev.inputType !== "insertText" || ev.isComposing || !ev.data) return;
    // xterm.js typed it itself, just now (no key down at the time: Pinyin punctuation, the emoji picker).
    const typedByXterm = sends > sendsAtInput;
    // Typed on keypress, or the text of a composition xterm.js is about to read out of the textarea: leave it.
    if (handled && !typedByXterm) return;
    const end = textarea.selectionStart;
    const start = end - ev.data.length;
    if (start >= 0 && textarea.value.slice(start, end) === ev.data) {
      textarea.value = textarea.value.slice(0, start) + textarea.value.slice(end);
      textarea.setSelectionRange(start, start);
    }
    if (typedByXterm) return;
    handled = true;
    if (!compositionPending && !waiting) return send(ev.data);
    if (!waiting) {
      waiting = [];
      // Set now, so it runs after the timer xterm.js set at compositionend.
      setTimeout(() => {
        const queued = waiting ?? [];
        waiting = null;
        for (const d of queued) send(d);
      }, 0);
    }
    waiting.push(ev.data);
  });
  return {
    sent: () => {
      sends++;
      handled = true;
      compositionPending = false;
    },
  };
}
