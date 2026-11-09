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

// A path or URL never spans these: whitespace, quotes, brackets, Chinese punctuation, and box-drawing lines (TUI
// borders, which also glue rows together when a box reaches the right edge).
const TOKEN = /[^\s'"`<>()[\]{}，。；：！？、（）【】「」『』《》⎿\u2500-\u257f]+/g;
const TRAILING = /[.,;:!?]+$/;
const LINE_SUFFIX = /:(\d+)(?::\d+)?$/;
// A relative path counts only if it names a file with an extension, so "and/or", "3/4" and dates stay text.
const HAS_EXTENSION = /\/[^/]*\.[A-Za-z][\w]*$/;

function decodeFileUrl(url: string): string {
  try {
    return decodeURIComponent(url.slice(7));
  } catch {
    return url.slice(7); // a stray "%" that is not an escape
  }
}

export function findLinks(text: string): FoundLink[] {
  const links: FoundLink[] = [];
  for (const m of text.matchAll(TOKEN)) {
    const token = m[0].replace(TRAILING, "");
    const start = m.index;
    const end = start + token.length;
    if (/^https?:\/\/\S/.test(token)) {
      links.push({ kind: "url", target: token, start, end });
      continue;
    }
    const lineMatch = token.match(LINE_SUFFIX);
    const written = lineMatch ? token.slice(0, -lineMatch[0].length) : token;
    // A Markdown file link written as a URL (`file:///Users/…/1.png`, `%20` for spaces) names the same file.
    const path = written.startsWith("file:///") ? decodeFileUrl(written) : written;
    const rooted = /^(~|\.{1,2})?\/[^/]/.test(path);
    if (!rooted && !(path.includes("/") && !path.startsWith("/") && HAS_EXTENSION.test(path))) continue;
    links.push({ kind: "path", target: path, ...(lineMatch ? { line: Number(lineMatch[1]) } : {}), start, end });
  }
  return links;
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

/** A row that carries on the one above: xterm wrapped it, or the row above runs into the edge (a TUI wrapping its
 *  own text, as Claude and Codex do inside tmux). */
type RowAt = (i: number) => { cells: Cell[]; wrapped: boolean } | undefined;

/**
 * Rows around `at` (0-based) read as one text, at most four rows each way. A carried-on row's hanging indent (Claude
 * and Codex start continuation rows with spaces) is left out, so a long path reads whole. `breaks` are the indices
 * where a carried-on row's text starts, for when the glue was wrong (see `linkChoices`).
 */
export function glueRows(rowAt: RowAt, at: number): { text: string; cells: Placed[]; breaks: number[] } {
  const carries = (i: number) => {
    const row = rowAt(i);
    const above = rowAt(i - 1);
    return !!row && !!above && (row.wrapped || runsToEdge(above.cells));
  };
  let first = at;
  while (first > 0 && at - first < 4 && carries(first)) first--;
  let last = at;
  while (last - at < 4 && carries(last + 1)) last++;

  let text = "";
  const cells: Placed[] = [];
  const breaks: number[] = [];
  for (let y = first; y <= last; y++) {
    const row = rowAt(y)?.cells ?? [];
    let x = 0;
    if (y > first) {
      while (x < row.length && x < 8 && row[x].chars === " ") x++;
      breaks.push(text.length);
    }
    // A row the next one carries on ends where its text does (the free column a 汉字 left is not a space).
    let end = row.length;
    if (y < last) while (end > x && row[end - 1].chars === "" && row[end - 1].width !== 0) end--;
    for (; x < end; x++) {
      const cell = row[x];
      if (cell.width === 0) continue;
      const chars = cell.chars || " ";
      for (let i = 0; i < chars.length; i++) cells.push({ x: x + 1, y: y + 1, w: cell.width });
      text += chars;
    }
  }
  return { text, cells, breaks };
}

/**
 * Each link in glued text, with fallbacks: a glue can join a path to unrelated text on the row below or above, so a
 * path that crosses a break is also offered cut at each break, longest first. The first one that exists wins.
 */
export function linkChoices(text: string, breaks: readonly number[]): FoundLink[][] {
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
    return [link, ...parts];
  });
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
