import { describe, expect, it } from "vitest";
import { type Cell, fileUris, findLinks, glueRows, linkChoices, parseLinkTarget, pathCandidates, shellEscape } from "./links";

const texts = (line: string) => findLinks(line).map((l) => [l.kind, l.target, l.line ?? null]);

describe("findLinks", () => {
  it("finds absolute, home and relative paths, with Chinese segments", () => {
    expect(texts("看 /Users/example/Desktop/Workspace/01_项目/demo/HANDOFF.md 这里")).toEqual([
      ["path", "/Users/example/Desktop/Workspace/01_项目/demo/HANDOFF.md", null],
    ]);
    expect(texts("~/Desktop/a.txt and ./b/c.ts and ../d.md")).toEqual([
      ["path", "~/Desktop/a.txt", null],
      ["path", "./b/c.ts", null],
      ["path", "../d.md", null],
    ]);
    expect(texts("edited app/src/terminal.ts")).toEqual([["path", "app/src/terminal.ts", null]]);
  });

  it("takes paths out of Claude's tool lines, backticks and Chinese punctuation", () => {
    expect(texts("⏺ Update(/w/app/src/main.ts)")).toEqual([["path", "/w/app/src/main.ts", null]]);
    expect(texts("──────/w/a.md")).toEqual([["path", "/w/a.md", null]]);
    expect(texts("改了 `src/links.ts`，还有「/w/x.md」。")).toEqual([
      ["path", "src/links.ts", null],
      ["path", "/w/x.md", null],
    ]);
  });

  it("splits off a trailing line and column and sentence punctuation", () => {
    expect(texts("see src/a.ts:42:7, then /w/b.md:3.")).toEqual([
      ["path", "src/a.ts", 42],
      ["path", "/w/b.md", 3],
    ]);
  });

  it("finds web links and does not also read them as paths", () => {
    expect(texts("open https://github.com/Longado/shifu/pull/3). done")).toEqual([
      ["url", "https://github.com/Longado/shifu/pull/3", null],
    ]);
  });

  it("ignores lone slashes, fractions and dates", () => {
    expect(texts("a / b, 3/4, 2026/11/07, and/or")).toEqual([]);
  });

  it("reports where each link sits in the string", () => {
    const [link] = findLinks("x `/w/a.md:9` y");
    expect(link.start).toBe(3);
    expect(link.end).toBe(12); // covers "/w/a.md:9"
  });

  it("keeps spaces and brackets in quoted and Markdown destinations", () => {
    for (const text of [
      '`/w/申请 材料/报名表 (最终版).docx`',
      '"/w/申请 材料/报名表 (最终版).docx"',
      '[Word](</w/申请 材料/报名表 (最终版).docx>)',
      '[Word](/w/申请 材料/报名表 (最终版).docx)',
    ]) {
      expect(texts(text)).toEqual([["path", "/w/申请 材料/报名表 (最终版).docx", null]]);
      const [link] = findLinks(text);
      expect(text.slice(link.start, link.end)).toBe("/w/申请 材料/报名表 (最终版).docx");
    }
    expect(texts("看 /w/report(final).pdf，和 docs/report[2].pdf。" )).toEqual([
      ["path", "/w/report(final).pdf", null], ["path", "docs/report[2].pdf", null],
    ]);
    expect(texts("Update(/w/report(final).pdf)")).toEqual([["path", "/w/report(final).pdf", null]]);
    expect(texts("[README](README.md)")).toEqual([["path", "README.md", null]]);
  });

  it("resolves encoded absolute destinations without changing the highlighted range", () => {
    const written = "/w/%E7%94%B3%E8%AF%B7%20%E6%9D%90%E6%96%99/report%20%28final%29.pdf";
    const text = `[PDF](<${written}>)`;
    const [choices] = linkChoices(text, []);
    expect(choices.map((l) => l.target)).toEqual([written, "/w/申请 材料/report (final).pdf"]);
    expect(choices.every((l) => text.slice(l.start, l.end) === written)).toBe(true);
    expect(pathCandidates("/w/100%.pdf")).toEqual(["/w/100%.pdf"]);
    expect(pathCandidates("/w/a%2520b.pdf")).toEqual(["/w/a%2520b.pdf", "/w/a%20b.pdf"]);
  });

  it("recognizes file URLs and code line references while rejecting other protocols", () => {
    expect(parseLinkTarget("file://localhost/w/hello%20world.md#L12")).toEqual({kind:"path",target:"/w/hello world.md",line:12});
    expect(texts("[source](src/main.ts#L42-L48)")).toEqual([["path", "src/main.ts", 42]]);
    for (const target of ["javascript:alert(1)", "data:text/html,test", "file://other-host/w/a.md", "vscode://file/w/a.md"])
      expect(parseLinkTarget(target, true)).toBeNull();
  });
});

/** A 108-column screen as tmux draws it (no row is marked wrapped); 汉字 take two cells. */
function screen(...lines: string[]) {
  const rows = lines.map((line) => {
    const cells: Cell[] = [];
    for (const ch of line) {
      const wide = /[\u3000-\u9fff\uff00-\uffef]/.test(ch);
      cells.push({ chars: ch, width: wide ? 2 : 1 });
      if (wide) cells.push({ chars: "", width: 0 });
    }
    if (cells.length > 108) throw new Error(`row wider than the screen: ${line}`);
    while (cells.length < 108) cells.push({ chars: "", width: 1 });
    return { cells, wrapped: false };
  });
  return (i: number) => rows[i];
}

/** The path a click on row `at` finds, given which candidates exist. */
function clicked(rowAt: ReturnType<typeof screen>, at: number, exists: (p: string) => boolean) {
  const { text, cells, breaks } = glueRows(rowAt, at);
  return linkChoices(text, breaks)
    .map((alts) => alts.find((l) => l.kind === "path" && exists(l.target) && cells[l.start].y <= at + 1 && cells[l.end - 1].y >= at + 1))
    .filter(Boolean)
    .map((l) => l!.target);
}

describe("paths that a TUI wrapped", () => {
  const generated = "/Users/example/.codex/generated_images/01a1bf11-47cd-7ba1-9814-79059b921075/exec-9b79b791-4779-4ed5-bccb-73aee8342d7c.png";

  it("reads a long path across Codex's hanging indent, from either row", () => {
    const head = "  └ " + generated.slice(0, 104);
    const rows = screen("• Generated image", head, "    " + generated.slice(104), "", "› Ask Codex");
    expect(clicked(rows, 1, () => true)).toEqual([generated]);
    expect(clicked(rows, 2, (p) => p === generated)).toEqual([generated]);
  });

  it("joins a row cut one short where a 汉字 did not fit", () => {
    const rows = screen(
      "  09/1.png)、图 2 (01_项目/sample-app/docs/design-2026-11-09/2.png)、图 3 (01_xxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
      "  项目/sample-app/docs/design-2026-11-09/3.png)。",
    );
    const want = "01_xxxxxxxxxxxxxxxxxxxxxxxxxxxxx项目/sample-app/docs/design-2026-11-09/3.png";
    expect(clicked(rows, 1, (p) => p === want)).toEqual([want]);
  });

  it("keeps a path that only happens to end at the edge when the next row is something else", () => {
    const path = "/w/" + "a".repeat(99) + ".png";
    const rows = screen("  " + path, "  然后再看第二张");
    expect(clicked(rows, 0, (p) => p === path)).toEqual([path]);
  });

  it("reads Markdown file links", () => {
    expect(texts("[1.png](file:///Users/example/%E6%88%AA%20%E5%9B%BE.png)")).toEqual([["path", "/Users/example/截 图.png", null]]);
  });

  it("finds encoded Chinese paths from the first, middle and final row of a long reply", () => {
    const path = "/w/" + "申请材料/".repeat(22) + "报名表.pdf";
    const encoded = encodeURI(path);
    const chunks = encoded.match(/.{1,104}/g)!;
    expect(chunks.length).toBeGreaterThan(4);
    const rows = screen(...chunks.map((s) => "    " + s));
    for (const at of [0, Math.floor(chunks.length / 2), chunks.length - 1])
      expect(clicked(rows, at, (p) => p === path)).toContain(path);
  });
});

describe("dropped files", () => {
  it("escapes a path the way a shell prompt expects", () => {
    expect(shellEscape("/Users/example/Desktop/截屏 2026-11-07 (2).png")).toBe("/Users/example/Desktop/截屏\\ 2026-11-07\\ \\(2\\).png");
    expect(shellEscape("/w/it's&me.md")).toBe("/w/it\\'s\\&me.md");
  });

  it("reads file paths out of a uri list", () => {
    expect(fileUris("# c\r\nfile:///Users/example/a%20b.png\r\nhttps://x.com\nfile:///w/%E6%88%AA.png")).toEqual([
      "/Users/example/a b.png",
      "/w/截.png",
    ]);
  });
});
