import { describe, expect, it } from "vitest";
import { type Cell, fileUris, findLinks, glueRows, linkChoices, opensOutsideShifu, parseLinkTarget, pathCandidates, shellEscape } from "./links";

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
  const { text, cells, breaks, soft } = glueRows(rowAt, at);
  return linkChoices(text, breaks, soft)
    .map((alts) => alts.find((l) => l.kind === "path" && exists(l.target) && cells[l.start].y <= at + 1 && cells[l.end - 1].y >= at + 1))
    .filter(Boolean)
    .map((l) => l!.target);
}

describe("bare paths with a space in the filename", () => {
  const file = "/Users/example/研究/2026-11-20-领导反馈/① 海外应用案例表.docx";

  it("tries the path with the words after a space too, longest first, so the one that exists wins", () => {
    const rows = screen("  访达可直接打开：", "", "  " + file, "", "  Worked for 6m 25s");
    expect(clicked(rows, 2, (p) => p === file)).toEqual([file]);
    const [choices] = linkChoices(`见 ${file} 已更新。`, []);
    expect(choices.map((l) => l.target).slice(0, 3)).toEqual([`${file} 已更新`, file, "/Users/example/研究/2026-11-20-领导反馈/①"]);
  });

  it("still finds a path that ends at the space", () => {
    expect(clicked(screen("  打开 /w/a.md 看看"), 0, (p) => p === "/w/a.md")).toEqual(["/w/a.md"]);
  });
});

describe("paths a TUI broke before the edge", () => {
  // As Codex 0.170 drew a Markdown link and a bare path in a 108-column tmux pane (rows end short of the edge).
  const dir = "/Users/example/.cache/session-01a1f805-f93f-71c1-8220-0e55d5eab90b/scratchpad";
  const file = `${dir}/中文目录/报告 终版.md`;

  it("joins a row broken after a - or /, and one broken at a space", () => {
    const rows = screen(
      "• 1. 报告 终版.md (/Users/example/.cache/session-01a1f805-f93f-71c1-8220-",
      "     0e55d5eab90b/scratchpad/中文目录/报告 终版.md)",
      "  2. /Users/example/.cache/session-01a1f805-f93f-71c1-8220-0e55d5eab90b/scratchpad/中文目录/报告",
      "     终版.md",
      "",
      "› Ask Codex to do anything",
    );
    const exists = (p: string) => p === file || p === dir;
    for (const at of [0, 1, 2, 3]) expect(clicked(rows, at, exists), `row ${at}`).toEqual([file]);
  });

  it("reads Chinese brackets in a filename, also where the TUI broke inside them", () => {
    const named = "/Users/example/研究/领导反馈/① 海外应用案例表（场景一段话版）.docx";
    const exists = (p: string) => p === named;
    expect(clicked(screen(`  已生成：${named}，请看`), 0, exists)).toEqual([named]);
    const rows = screen("  /Users/example/研究/领导反馈/① 海外应用案例表（场景一段", "  话版）.docx", "", "› Ask Codex");
    expect(clicked(rows, 0, exists)).toEqual([named]);
    expect(clicked(rows, 1, exists)).toEqual([named]);
    expect(clicked(screen("  见 /w/a.md（已更新）"), 0, (p) => p === "/w/a.md")).toEqual(["/w/a.md"]);
    const pdf = "/Users/example/pdf/① 海外应用案例表（场景一段话版）.pdf";
    expect(clicked(screen(`  PDF 已生成：${pdf}`, "    Pages: 3"), 0, (p) => p === pdf)).toEqual([pdf]);
  });

  it("keeps a path whole when the indented row below is unrelated", () => {
    const rows = screen("  报告在 /Users/example/out/", "     下一步：等你确认");
    expect(clicked(rows, 0, (p) => p === "/Users/example/out/" || p === "/Users/example/out")).toEqual(["/Users/example/out/"]);
  });
});

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

describe("opensOutsideShifu", () => {
  it("sends Office documents and archives to their own app, keeps what the file tab can show", () => {
    for (const p of ["/a/报告.docx", "/a/B.XLSX", "/a/c.pptx", "/a/d.doc", "/a/e.key", "/a/f.pages", "/a/g.zip"])
      expect(opensOutsideShifu(p)).toBe(true);
    for (const p of ["/a/r.pdf", "/a/r.md", "/a/r.png", "/a/r.mp4", "/a/r.ts", "/a/Makefile", "/a/v1.2/notes"])
      expect(opensOutsideShifu(p)).toBe(false);
  });
});
