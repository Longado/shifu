/** Clickable things in terminal text: web links, and file paths that the backend then checks exist. */

export interface FoundLink {
  kind: "url" | "path";
  /** The URL, or the path as written (may be relative or start with `~`). */
  target: string;
  /** Line number written after the path (`a.ts:42`). */
  line?: number;
  /** String indices of the clickable text, end exclusive; includes any `:line:col`. */
  start: number;
  end: number;
}

// Token boundaries locate bare paths; quoted/Markdown destinations and balanced filename brackets are read below.
// Box-drawing lines are TUI borders, which can also glue rows together at the screen edge.
const TOKEN = /[^\s'"`<>()[\]{}，。；：！？、（）【】「」『』《》⎿\u2500-\u257f]+/g;
const TRAILING = /[.,;:!?]+$/;
const LINE_SUFFIX = /(?::(\d+)(?::\d+)?|#L(\d+)(?:C\d+)?(?:-L?\d+(?:C\d+)?)?)$/;
// A relative path counts only if it names a file with an extension, so "and/or", "3/4" and dates stay text.
const HAS_EXTENSION = /\/[^/]*\.[A-Za-z][\w]*$/;

function decoded(path: string): string {
  try {
    return decodeURIComponent(path);
  } catch {
    return path; // a stray "%" that is not an escape
  }
}

/** Try the literal filename first: a real file may itself contain "%20". Decode URI escapes just once. */
export function pathCandidates(path: string): string[] {
  const plain = decoded(path);
  return plain === path ? [path] : [path, plain];
}

/** Also used for OSC 8 links, whose visible label need not contain the path. Only local files and HTTP(S) open. */
export function parseLinkTarget(written: string, allowFilename = false): Omit<FoundLink, "start" | "end"> | null {
  if (/^https?:\/\/\S/.test(written)) return { kind: "url", target: written };
  const suffix = written.match(LINE_SUFFIX);
  let path = suffix ? written.slice(0, -suffix[0].length) : written;
  if (path.startsWith("file://")) {
    try {
      const url = new URL(path);
      if (url.hostname && url.hostname !== "localhost") return null;
      path = decoded(url.pathname);
    } catch {
      return null;
    }
  } else if (/^[a-z][\w+.-]*:/i.test(path)) {
    return null;
  }
  const rooted = path === "~" || /^(~|\.{1,2})?\/[^/]/.test(path);
  const relative = !path.startsWith("/") && (HAS_EXTENSION.test(path) || (allowFilename && /^[^/]+\.[A-Za-z][\w]*$/.test(path)));
  if (!rooted && !relative) return null;
  return { kind: "path", target: path, ...(suffix ? { line: Number(suffix[1] ?? suffix[2]) } : {}) };
}

export function findLinks(text: string): FoundLink[] {
  const links: FoundLink[] = [];
  const add = (written: string, start: number, allowFilename = false) => {
    const link = parseLinkTarget(written, allowFilename);
    if (link) links.push({ ...link, start, end: start + written.length });
  };
  // Quoted/inline-code/angle destinations preserve spaces and punctuation inside the filename.
  for (const m of text.matchAll(/`([^`\r\n]+)`|"([^"\r\n]+)"|'([^'\r\n]+)'|<([^<>\r\n]+)>/g))
    add(m[1] ?? m[2] ?? m[3] ?? m[4], m.index + 1, true);
  // Markdown destinations can have balanced parentheses in the filename, even without angle brackets.
  for (const m of text.matchAll(/\]\(/g)) {
    const start = m.index + 2;
    if (text[start] === "<") continue; // handled above
    let depth = 0;
    let end = start;
    for (; end < text.length && !/[\r\n]/.test(text[end]); end++) {
      if (text[end] === "(") depth++;
      if (text[end] === ")" && depth-- === 0) break;
    }
    if (text[end] === ")") add(text.slice(start, end), start, true);
  }
  for (const m of text.matchAll(TOKEN)) {
    const start = m.index;
    if (links.some((l) => start >= l.start && start < l.end)) continue;
    // The initial token establishes a path boundary (including Claude's Update(/path)). Extend it over balanced
    // filename brackets, stopping at an unmatched closing bracket belonging to surrounding prose/Markdown.
    if (!m[0].includes("/") && m[0] !== "~") continue;
    let end = start;
    const stack: string[] = [];
    for (; end < text.length; end++) {
      const c = text[end];
      if (/[\s'"`<>，。；：！？、（）【】「」『』《》⎿\u2500-\u257f]/.test(c)) break;
      if (c === "(" || c === "[" || c === "{") stack.push(c);
      if (c === ")" || c === "]" || c === "}") {
        const open = c === ")" ? "(" : c === "]" ? "[" : "{";
        if (stack.at(-1) !== open) break;
        stack.pop();
      }
    }
    add(text.slice(start, end).replace(TRAILING, ""), start);
  }
  return links.sort((a, b) => a.start - b.start);
}

/** One terminal cell: its text ("" when empty) and width (2 for 汉字, 0 for the cell a wide character spills into). */
export interface Cell {
  chars: string;
  width: number;
}

/** Where a character of glued text sits on screen, 1-based. */
export interface Placed {
  x: number;
  y: number;
  w: number;
}

const filled = (c: Cell | undefined) => !!c && (c.chars !== "" || c.width === 0);

/** Text runs into the right edge: the last cell is used (maybe by the second half of a 汉字), or only the last column
 *  is free, which is what is left when a 汉字 did not fit and went to the next row. */
export function runsToEdge(row: readonly Cell[]): boolean {
  return filled(row[row.length - 1]) || filled(row[row.length - 2]);
}

/** A row that carries on the one above: xterm wrapped it, the row above runs into the edge (a TUI wrapping its
 *  own text, as Claude and Codex do inside tmux), or the row above ends in a path that the TUI broke early, at a
 *  `/` or `-` or at a space, the way Codex and Claude wrap words. */
type RowAt = (i: number) => { cells: Cell[]; wrapped: boolean } | undefined;

const rowText = (cells: readonly Cell[]) => cells.filter((c) => c.width !== 0).map((c) => c.chars || " ").join("").trimEnd();

/** How a row joins the one above: null when it does not; "hard" when the text ran into the edge; "soft" when a TUI
 *  broke a path early. A soft break may have eaten a space ("报告 终版" drawn as "报告" / "终版"), so the link
 *  choices try it both ways. */
function joint(row: { cells: Cell[]; wrapped: boolean }, above: { cells: Cell[]; wrapped: boolean }): "hard" | "soft" | null {
  if (row.wrapped || runsToEdge(above.cells)) return "hard";
  const text = rowText(above.cells);
  // Only where a path was running: one of the row's last words holds a "/", and the next row is an indented
  // continuation. A row that ends a bracket or a sentence ended the path too.
  if (!text.split(/\s+/).slice(-3).some((w) => w.includes("/")) || /[)\]>」』）。，；]$/.test(text)) return null;
  return /^ +\S/.test(rowText(row.cells)) ? "soft" : null;
}

/**
 * Rows around `at` (0-based) read as one text, at most 32 rows each way (URI-encoded Chinese paths can be very long).
 * A carried-on row's hanging indent (Claude and Codex start continuation rows with spaces) is left out, so a long
 * path reads whole. `breaks` are the indices
 * where a carried-on row's text starts, for when the glue was wrong (see `linkChoices`).
 */
export function glueRows(rowAt: RowAt, at: number): { text: string; cells: Placed[]; breaks: number[]; soft: number[] } {
  const join = (i: number) => {
    const row = rowAt(i);
    const above = rowAt(i - 1);
    return row && above ? joint(row, above) : null;
  };
  let first = at;
  while (first > 0 && at - first < 32 && join(first) !== null) first--;
  let last = at;
  while (last - at < 32 && join(last + 1) !== null) last++;

  let text = "";
  const cells: Placed[] = [];
  const breaks: number[] = [];
  const soft: number[] = [];
  for (let y = first; y <= last; y++) {
    const row = rowAt(y)?.cells ?? [];
    let x = 0;
    if (y > first) {
      while (x < row.length && x < 8 && row[x].chars === " ") x++;
      breaks.push(text.length);
      if (join(y) === "soft") soft.push(text.length);
    }
    // A row the next one carries on ends where its text does (the free column a 汉字 left is not a space).
    let end = row.length;
    if (y < last) while (end > x && (row[end - 1].chars === "" || row[end - 1].chars === " ") && row[end - 1].width !== 0) end--;
    for (; x < end; x++) {
      const cell = row[x];
      if (cell.width === 0) continue;
      const chars = cell.chars || " ";
      for (let i = 0; i < chars.length; i++) cells.push({ x: x + 1, y: y + 1, w: cell.width });
      text += chars;
    }
  }
  return { text, cells, breaks, soft };
}

/**
 * Each link in glued text, with fallbacks: a glue can join a path to unrelated text on the row below or above, so a
 * path that crosses a break is also offered cut at each break, longest first; across a `soft` break it is also
 * offered with the space the TUI may have dropped there. The first one that exists wins.
 */
export function linkChoices(text: string, breaks: readonly number[], soft: readonly number[] = []): FoundLink[][] {
  return findLinks(text).map((link) => {
    if (link.kind === "url") return [link];
    const cuts = [link.start, ...breaks.filter((b) => b > link.start && b < link.end), link.end];
    const parts: FoundLink[] = [];
    for (let i = 0; i < cuts.length; i++)
      for (let j = cuts.length - 1; j > i; j--) {
        if (i === 0 && j === cuts.length - 1) continue;
        for (const l of findLinks(text.slice(cuts[i], cuts[j])))
          if (l.kind === "path") parts.push({ ...l, start: l.start + cuts[i], end: l.end + cuts[i] });
      }
    parts.sort((a, b) => b.end - b.start - (a.end - a.start));
    const whole = [...longer(text, link), link].flatMap((l) => {
      const inside = breaks.filter((b) => b > l.start && b < l.end);
      return [
        l,
        ...inside.filter((b) => soft.includes(b)).map((b) => ({ ...l, target: `${l.target.slice(0, b - l.start)} ${l.target.slice(b - l.start)}` })),
        // Cut where a row ends, in case the next row was not more of the name ("…版.pdf" then "Pages: 3").
        ...inside.map((b) => ({ ...l, target: text.slice(l.start, b).replace(TRAILING, ""), end: b })),
      ];
    });
    return [...whole, ...parts].flatMap((l) => pathCandidates(l.target).map((target) => ({ ...l, target })));
  });
}

const PATH_CHAR = `[^\\s'"\`<>()[\\]{}，。；：！？、（）【】「」『』《》⎿\\u2500-\\u257f]`;
// What can carry a filename on past where a bare path token stops: a space and a word, or a 「（…）」/「【…】」 group.
const MORE = new RegExp(`^(?: ${PATH_CHAR}+|（[^\\s（）]*）${PATH_CHAR}*|【[^\\s【】]*】${PATH_CHAR}*)`);

/** A bare path stops at a space or a Chinese bracket, but filenames often have them ("① 海外应用案例表（终版）.docx"):
 *  the path with up to six more pieces, longest first. Only a path that exists is linked, so prose after it falls
 *  away. */
function longer(text: string, link: FoundLink): FoundLink[] {
  if (link.line !== undefined) return [];
  const found: FoundLink[] = [];
  let end = link.end;
  for (let i = 0; i < 6; i++) {
    const m = text.slice(end).match(MORE);
    if (!m) break;
    end += m[0].length;
    const target = text.slice(link.start, end).replace(TRAILING, "");
    found.unshift({ kind: "path", target, start: link.start, end: link.start + target.length });
  }
  return found;
}

/** A path as typed at a shell prompt (what Terminal.app inserts for a dropped file): risky characters get a backslash. */
export function shellEscape(path: string): string {
  return path.replace(/[^\w\-.,/:@%+=\u0080-\uffff]/g, "\\$&");
}

/** Local paths from a drop's `text/uri-list` (one URI per line, `#` lines are comments). */
export function fileUris(uriList: string): string[] {
  return uriList
    .split(/\r?\n/)
    .filter((u) => u.startsWith("file://"))
    .map((u) => decodeURIComponent(new URL(u).pathname));
}
