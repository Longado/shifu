/** What xterm.js sent: a wheel turn, another mouse report (press, drag, release; SGR form, which tmux asks for), or
 *  null for a key. Wheel buttons have bit 64 set. */
export function mouseReport(data: string): "wheel" | "other" | null {
  const m = /^\x1b\[<(\d+);\d+;\d+[Mm]$/.exec(data);
  if (!m) return null;
  return Number(m[1]) & 64 ? "wheel" : "other";
}
