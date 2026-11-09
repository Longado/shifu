/**
 * tmux owns the mouse in a panel: a drag selects inside tmux, and on release tmux hands the text to the terminal as
 * OSC 52 (`52;<selection>;<base64>`). Returns the text, or null for a clipboard query or a payload that is not base64.
 */
export function decodeOsc52(data: string): string | null {
  const payload = data.slice(data.indexOf(";") + 1);
  if (payload === "" || payload === "?") return null;
  let binary: string;
  try {
    binary = atob(payload);
  } catch {
    return null;
  }
  return new TextDecoder().decode(Uint8Array.from(binary, (c) => c.charCodeAt(0)));
}
