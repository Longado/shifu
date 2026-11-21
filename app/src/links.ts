/** Clickable things in terminal text: web links, and file paths that the backend then checks exist. */

import type { CodexFileLink } from "./api";
import type { AgentView } from "./format";

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

export function findLinks(text: string, includeRendered = true): FoundLink[] {
  const links: FoundLink[] = [];
  const add = (written: string, start: number, allowFilename = false) => {
    const link = parseLinkTarget(written, allowFilename);
    if (link) links.push({ ...link, start, end: start + written.length });
  };
  // Quoted/inline-code/angle destinations preserve spaces and punctuation inside the filename.
  for (const m of text.matchAll(/`([^`\r\n]+)`|"([^"\r\n]+)"|'([^'\r\n]+)'|<([^<>\r\n]+)>/g))
    add(m[1] ?? m[2] ?? m[3] ?? m[4], m.index + 1, true);
  // Markdown destinations, including Codex's rendered "label (target)", keep spaces and filename brackets.
  for (const m of text.matchAll(/\]\(|[ \t]\(/g)) {
    const start = m.index + 2;
    if (text[start] === "<") continue; // handled above
    let depth = 0;
    let end = start;
    for (; end < text.length && !/[\r\n]/.test(text[end]); end++) {
      if (text[end] === "(") depth++;
      if (text[end] === ")" && depth-- === 0) break;
    }
    if (text[end] !== ")") continue;
    const written = text.slice(start, end);
    if (m[0] !== "](") {
      if (!includeRendered) continue;
      // In rendered text, a phrase before the first slash is prose around a path. Let the token reader below
      // find that path (or URL), and do not treat an HTTP URL plus its following prose as one URL.
      const slash = written.indexOf("/");
      if ((slash > 0 && /\s/.test(written.slice(0, slash))) || (/^https?:\/\//.test(written) && /\s/.test(written))) continue;
      const original = findLinks(written, false);
      const target = parseLinkTarget(written, true);
      // Several existing targets, or a filename followed by a sentence, keep their original token boundaries.
      if (original.length > 1 || (original.length === 1 && /\s/.test(written) && target?.kind === "path" && !HAS_EXTENSION.test(`/${target.target}`))) continue;
    }
    add(written, start, true);
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
  underlined?: boolean;
}

/** Where a character of glued text sits on screen, 1-based. */
export interface Placed {
  x: number;
  y: number;
  w: number;
  underlined?: boolean;
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

/** Codex draws its own side panes inside one terminal; a divider must continue on a neighboring row. */
export function linkColumns(rowAt: RowAt, at: number): { start: number; end: number }[] {
  const row = rowAt(at);
  if (!row) return [];
  const neighbors = [rowAt(at - 1), rowAt(at + 1)];
  const columns: { start: number; end: number }[] = [];
  let start = 0;
  for (let x = 1; x < row.cells.length - 1; x++) {
    const divider = row.cells[x].chars;
    if ((divider !== "│" && divider !== "┃") || !neighbors.some((neighbor) => neighbor?.cells[x]?.chars === divider)) continue;
    columns.push({ start, end: x });
    start = x + 1;
  }
  columns.push({ start, end: row.cells.length });
  return columns;
}

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
export function glueRows(sourceRowAt: RowAt, at: number, columns?: { start: number; end: number }): { text: string; cells: Placed[]; breaks: number[]; soft: number[] } {
  const rowAt: RowAt = columns ? (i) => {
    const row = sourceRowAt(i);
    if (!row) return undefined;
    // A panel ending or moving is a row boundary, never a continuation into its neighbor.
    if ((columns.start > 0 && !/[│┃]/.test(row.cells[columns.start - 1]?.chars ?? ""))
      || (columns.end < row.cells.length && !/[│┃]/.test(row.cells[columns.end]?.chars ?? ""))) return undefined;
    const cells = row.cells.slice(columns.start, columns.end);
    const fullWidth = columns.start === 0 && columns.end === row.cells.length;
    if (!fullWidth) {
      // TUI padding is drawn as spaces; it does not mean the text reached the pane edge.
      for (let x = cells.length - 1; x >= 0 && (cells[x].chars === " " || cells[x].chars === ""); x--)
        cells[x] = { ...cells[x], chars: "" };
    }
    return { cells, wrapped: fullWidth && row.wrapped };
  } : sourceRowAt;
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
      for (let i = 0; i < chars.length; i++) cells.push({ x: x + 1 + (columns?.start ?? 0), y: y + 1, w: cell.width, ...(cell.underlined ? { underlined: true } : {}) });
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
export function linkChoices(text: string, breaks: readonly number[], soft: readonly number[] = [], sourceMessage?: string | null, sourceLabels: readonly string[] = []): FoundLink[][] {
  const labels = [...sourceLabels, ...[...(sourceMessage ?? "").matchAll(/\[([^\]\r\n]+)\]\(/g)].map((m) => m[1])].sort((a, b) => b.length - a.length);
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
    return [...whole, ...parts].flatMap((l) => pathCandidates(l.target).map((target) => {
      // Restore target offsets first; its label is clickable only when that target passes the existence check.
      return { ...l, target, start: renderedLabelStart(text, l, breaks, labels) };
    }));
  });
}

/** Codex renders a Markdown link as "label (target)". Source labels make ordinary and inline titles unambiguous. */
function renderedLabelStart(text: string, link: FoundLink, breaks: readonly number[], labels: readonly string[] = []): number {
  if (text[link.start - 1] !== "(" || text[link.end] !== ")") return link.start;
  const rowStart = breaks.filter((b) => b < link.start).at(-1) ?? 0;
  const prefix = text.slice(rowStart, link.start - 1);
  const before = prefix.trimEnd();
  for (const label of labels) {
    const start = before.length - label.length;
    const joinsLatinWord = start > 0 && /[\p{Script=Latin}\p{Number}_]/u.test(before[start - 1]) && /^[\p{Script=Latin}\p{Number}_]/u.test(label);
    if (start >= 0 && before.slice(start) === label && !joinsLatinWord)
      return rowStart + start;
  }
  // Without the source, keep the preceding prose out and recognize only a standalone opening action label.
  const action = prefix.match(/^[ \t]*(?:(?:[•*-]|\d+[.)]) +)?((?:下载|打开|查看|预览)[^/()[\]{}<>，。；：！？、\r\n]*?) +$/);
  return action ? rowStart + prefix.indexOf(action[1]) : link.start;
}

/** Full underlined Codex titles use the source session's unambiguous file destination. */
export function underlinedFileLinks(text: string, cells: readonly Placed[], source: readonly CodexFileLink[]): FoundLink[][] {
  const links: FoundLink[][] = [];
  for (let start = 0; start < text.length; start++) {
    if (!cells[start]?.underlined) continue;
    let end = start + 1;
    while (end < text.length && cells[end]?.underlined && cells[end].y === cells[start].y) end++;
    const label = text.slice(start, end);
    const entry = source.find((item) => item.label === label);
    const target = entry && parseLinkTarget(entry.target, true);
    if (target?.kind === "path") {
      links.push(pathCandidates(target.target).map((path) => ({ ...target, target: path, start, end })));
    }
    start = end - 1;
  }
  return links;
}

type LinkLabelScope = Pick<AgentView, "name" | "kind" | "session_id" | "status_since" | "last_message">;

/** Inspect the entire glued block: the hovered row may hold the label while its target starts on a later row. */
export function needsCodexFileLinkLabels(text: string, choices: readonly FoundLink[][]): boolean {
  return findLinks(text).some((l) => l.kind === "path" && text[l.start - 1] === "(" && text[l.end] === ")"
    && !choices.some((alts) => alts.some((a) => a.start < l.start && a.end === l.end)));
}

/** One lookup for the current reply. Hovers share its result, including an empty result or a read failure. */
export class CodexFileLinkLabelCache {
  private key = "";
  private pending: Promise<readonly CodexFileLink[]> | null = null;

  get(agent: LinkLabelScope | undefined, hasRenderedFileLink: boolean, load: (name: string, sessionId: string) => Promise<CodexFileLink[]>): Promise<readonly CodexFileLink[]> {
    if (agent?.kind !== "codex" || !agent.session_id || !hasRenderedFileLink) return Promise.resolve([]);
    const sessionId = agent.session_id;
    const key = JSON.stringify([agent.name, agent.session_id, agent.status_since, agent.last_message]);
    if (this.pending && this.key === key) return this.pending;
    this.key = key;
    this.pending = Promise.resolve().then(() => load(agent.name, sessionId)).then(
      (labels) => this.key === key ? labels : [],
      () => [],
    );
    return this.pending;
  }
}

const PATH_CHAR = `[^\\s'"\`<>()[\\]{}，。；：！？、（）【】「」『』《》⎿\\u2500-\\u257f]`;
// A filename may continue with a word or a bracketed part, including spaces inside or before the brackets.
const MORE = new RegExp(`^(?: ${PATH_CHAR}+| ?\\([^()\\r\\n]*\\)${PATH_CHAR}*| ?（[^（）\\r\\n]*）${PATH_CHAR}*| ?【[^【】\\r\\n]*】${PATH_CHAR}*)`);

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

// Office, iWork and archives lead with their default app. DOCX also has a read-only Shifu preview.
const OUTSIDE = new Set(["doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "key", "pages", "numbers", "zip", "dmg"]);

/** Files whose path menu leads with the system default app. */
export function opensOutsideShifu(path: string): boolean {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return dot > 0 && OUTSIDE.has(name.slice(dot + 1).toLowerCase());
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
