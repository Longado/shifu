import { describe, expect, it } from "vitest";
import { type Cell, CodexFileLinkLabelCache, fileUris, findLinks, glueRows, linkChoices, needsCodexFileLinkLabels, opensOutsideShifu, parseLinkTarget, pathCandidates, shellEscape } from "./links";

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

  it("keeps bracketed filename parts after spaces, including spaces within Chinese brackets", () => {
    for (const file of ["/w/报名表 (最终版).docx", "/w/图 1 (最终版).png", "/w/中文（版本 一）.docx", "/w/中文 【版本 一】.docx"]) {
      const text = `  见 ${file}，请用默认程序打开。`;
      const exists = (p: string) => p === file;
      expect(clicked(screen(text), 0, exists)).toEqual([file]);
      const link = linkChoices(text, []).flat().find((l) => exists(l.target))!;
      expect(text.slice(link.start, link.end)).toBe(file);
    }
  });

  it("still picks the existing file before trailing prose or an unmatched enclosing bracket", () => {
    const file = "/w/报名表 (最终版).docx";
    for (const text of [`  已生成 ${file} 请查看`, `  已生成 (${file})，请查看`])
      expect(clicked(screen(text), 0, (p) => p === file)).toEqual([file]);
    expect(clicked(screen("  见 /w/a.md （已更新）"), 0, (p) => p === "/w/a.md")).toEqual(["/w/a.md"]);
  });

  it("finds bracketed filenames from either row after a TUI soft break", () => {
    for (const [head, tail, file] of [
      ["  /w/图 1", "    (最终版).png", "/w/图 1 (最终版).png"],
      ["  /w/中文（版本", "    一）.docx", "/w/中文（版本 一）.docx"],
    ]) {
      const rows = screen(head, tail);
      for (const at of [0, 1]) expect(clicked(rows, at, (p) => p === file)).toEqual([file]);
    }
  });
});

describe("Codex's rendered local document links", () => {
  it("reads the complete relative target, including filenames without a directory", () => {
    for (const [label, file] of [
      ["下载文档", "reports/目录/报告（导航版）.docx"],
      ["打开文档", "reports/2-Palantir 研究报告.docx"],
      ["打开文档", "plain.docx"],
    ]) {
      const text = `  ${label} (${file})`;
      expect(texts(text)).toEqual([["path", file, null]]);
      expect(clicked(screen(text), 0, (p) => p === file)).toEqual([file]);
      const link = linkChoices(text, []).flat().find((l) => l.target === file)!;
      expect(text.slice(link.start, link.end)).toBe(`${label} (${file}`);
    }
  });

  it("includes a clear opening label but leaves bullet markers and following prose outside the hit range", () => {
    const file = "reports/目录/报告（导航版）.docx";
    for (const prefix of ["  ", "• ", "  - ", "  1. "])
      for (const label of ["下载《研究报告》Word 文档", "打开 研究报告"])
        for (const tail of ["。这份文档包含完整说明。", "，点击后用默认程序打开。", " 这里还有后续说明。"]){
          const text = `${prefix}${label} (${file})${tail}`;
          const link = linkChoices(text, []).flat().find((l) => l.target === file)!;
          expect(text.slice(link.start, link.end)).toBe(`${label} (${file}`);
        }
  });

  it("keeps ambiguous prose and other links out of a local target's range", () => {
    const text = "  前面的正文提到打开文档 (reports/目录/报告（导航版）.docx)，然后查看第二份 (plain.docx)。";
    const links = linkChoices(text, []).flat();
    for (const file of ["reports/目录/报告（导航版）.docx", "plain.docx"]) {
      const link = links.find((l) => l.target === file)!;
      expect(text.slice(link.start, link.end)).toBe(file);
    }
    expect(clicked(screen("  打开文档 (plain.docx)"), 0, () => false)).toEqual([]);
  });

  it("keeps web URLs and rejected protocols following their existing rules", () => {
    expect(texts("  打开网页 (https://example.com/report)")).toEqual([["url", "https://example.com/report", null]]);
    expect(texts("  打开文档 (javascript:alert(1))")).toEqual([]);
  });

  it("reads paths and URLs inside parenthesized prose without absorbing that prose", () => {
    for (const prefix of ["see", "完整路径"])
      expect(texts(`打开文件 (${prefix} /tmp/report.txt)`)).toEqual([["path", "/tmp/report.txt", null]]);
    expect(texts("打开网页 (see https://example.com/report)")).toEqual([["url", "https://example.com/report", null]]);
    expect(texts("打开网页 (https://example.com/report then read it)")).toEqual([["url", "https://example.com/report", null]]);
    expect(texts("打开文件 (see /tmp/a.md and /tmp/b.md)")).toEqual([["path", "/tmp/a.md", null], ["path", "/tmp/b.md", null]]);
    expect(texts("打开文件 (files/a.md and files/b.md)")).toEqual([["path", "files/a.md", null], ["path", "files/b.md", null]]);
    expect(texts("打开文件 (/tmp/report.txt 请查看)")).toEqual([["path", "/tmp/report.txt", null]]);
  });

  it("uses exact source labels for ordinary titles in other languages and Chinese", () => {
    for (const label of ["Télécharger le rapport", "研究资料完整报告"]) {
      const file = "reports/完整报告.docx";
      const text = `  - ${label} (${file})，后面还有说明。`;
      const source = `[${label}](`; // The remembered message may end partway through its destination.
      const links = linkChoices(text, [], [], source).flat();
      const link = links.find((l) => l.target === file)!;
      expect(text.slice(link.start, link.end)).toBe(`${label} (${file}`);
      const [fallback] = linkChoices(text, []).flat();
      expect(text.slice(fallback.start, fallback.end)).toBe(file);
    }
  });

  it("matches each inline source label without including its neighbors or a different title", () => {
    const text = "先看 Research report (reports/a.docx)，再看 结果汇总 (reports/b.docx)，这里继续说明。";
    const source = "[report](other.docx) 和 [Research report](reports/a.docx) 和 [结果汇总](reports/b.docx)";
    const links = linkChoices(text, [], [], source).flat();
    for (const [label, file] of [["Research report", "reports/a.docx"], ["结果汇总", "reports/b.docx"]]) {
      const link = links.find((l) => l.target === file)!;
      expect(text.slice(link.start, link.end)).toBe(`${label} (${file}`);
    }
    const mismatch = "  Researchreport (reports/a.docx)";
    const [link] = linkChoices(mismatch, [], [], "[report](reports/a.docx)").flat();
    expect(mismatch.slice(link.start, link.end)).toBe("reports/a.docx");
    const unmatching = "  Different title (reports/a.docx)";
    const [pathOnly] = linkChoices(unmatching, [], [], source).flat();
    expect(unmatching.slice(pathOnly.start, pathOnly.end)).toBe("reports/a.docx");
  });

  it("maps a label and a relative target across the first, middle and last folded rows", () => {
    const file = "reports/研究目录/第一部分/第二部分/报告（导航版）.docx";
    const rows = screen("  • 下载文档 (reports/研究目录/", "    第一部分/第二部分/", "    报告（导航版）.docx)，请查看完整文档。");
    for (const at of [0, 1, 2]) {
      const {text, cells, breaks, soft} = glueRows(rows, at);
      const link = linkChoices(text, breaks, soft).flat().find((l) => l.target === file)!;
      expect(clicked(rows, at, (p) => p === file)).toEqual([file]);
      expect(cells[link.start]).toEqual({x: 5, y: 1, w: 2});
      expect(cells[link.end - 1]).toEqual({x: 23, y: 3, w: 1});
    }
  });

  it("uses indexed labels from older long replies without guessing the target from a label", () => {
    const labels = ["年度资料完整说明", "Rapport annuel intégral", "第二份资料汇总"];
    const text = "请看年度资料完整说明 (reports/a.docx)，还有Rapport annuel intégral (reports/b.docx)，以及第二份资料汇总 (reports/c.docx)。";
    const oldReply = "完整报告背景说明。".repeat(50) + "[年度资料完整说明](reports/a.docx) 和 [Rapport annuel intégral](reports/b.docx)";
    const currentReply = "这一轮讨论其它工作。".repeat(50);
    for (const remembered of [oldReply.slice(0, 300), currentReply.slice(0, 300)]) {
      const choices = linkChoices(text, [], [], remembered, labels).flat();
      for (const [i, file] of ["reports/a.docx", "reports/b.docx", "reports/c.docx"].entries()) {
        const link = choices.find((l) => l.target === file)!;
        expect(text.slice(link.start, link.end)).toBe(`${labels[i]} (${file}`);
      }
    }
    const notAWord = "  notreport (reports/a.docx)";
    const [link] = linkChoices(notAWord, [], [], null, ["report"]).flat();
    expect(notAWord.slice(link.start, link.end)).toBe("reports/a.docx");
    expect(linkChoices("年度资料完整说明", [], [], null, labels)).toEqual([]);
  });
});

describe("Codex file link label lookups", () => {
  const scope = {name: "codex-a", kind: "codex", session_id: "session-a", status_since: 1, last_message: "reply"};

  it("looks up labels when only the title is on the hovered row and reuses them on either row", async () => {
    const rows = screen("  自然标题 (", "    reports/资料.docx)");
    rows(1).wrapped = true;
    const cache = new CodexFileLinkLabelCache();
    let requests = 0;
    for (const row of [0, 1, 0]) {
      const {text, cells, breaks, soft} = glueRows(rows, row);
      const direct = linkChoices(text, breaks, soft);
      const hasFile = needsCodexFileLinkLabels(text, direct);
      expect(hasFile).toBe(true);
      const labels = await cache.get(scope, hasFile, async () => { requests++; return ["自然标题"]; });
      const [link] = linkChoices(text, breaks, soft, null, labels).flat();
      expect(link.target).toBe("reports/资料.docx");
      expect(cells[link.start]).toEqual({x: 3, y: 1, w: 2});
      expect(cells[link.start].y <= row + 1 && cells[link.end - 1].y >= row + 1).toBe(true);
    }
    expect(requests).toBe(1);
  });

  it("only fetches for rendered file links from a Codex session and shares the in-flight lookup", async () => {
    const cache = new CodexFileLinkLabelCache();
    let requests = 0;
    let finish!: (labels: string[]) => void;
    const load = () => {
      requests++;
      return new Promise<string[]>((resolve) => { finish = resolve; });
    };
    expect(await cache.get(undefined, true, load)).toEqual([]);
    expect(await cache.get({...scope, kind: "claude"}, true, load)).toEqual([]);
    expect(await cache.get({...scope, session_id: null}, true, load)).toEqual([]);
    expect(await cache.get(scope, false, load)).toEqual([]);
    expect(requests).toBe(0);
    const first = cache.get(scope, true, load);
    const second = cache.get(scope, true, load);
    expect(second).toBe(first);
    await Promise.resolve();
    expect(requests).toBe(1);
    finish(["完整资料"]);
    expect(await first).toEqual(["完整资料"]);
    expect(await cache.get(scope, true, load)).toEqual(["完整资料"]);
    expect(requests).toBe(1);
  });

  it("isolates names and session IDs, and discards a late result from the previous scope", async () => {
    const cache = new CodexFileLinkLabelCache();
    let finish!: (labels: string[]) => void;
    const first = cache.get(scope, true, () => new Promise<string[]>((resolve) => { finish = resolve; }));
    await Promise.resolve();
    const calls: string[] = [];
    const load = async (name: string, session: string) => {
      calls.push(`${name}/${session}`);
      return [session];
    };
    expect(await cache.get({...scope, session_id: "session-b"}, true, load)).toEqual(["session-b"]);
    finish(["来自旧会话的标签"]);
    expect(await first).toEqual([]);
    expect(await cache.get({...scope, name: "codex-b", session_id: "session-b"}, true, load)).toEqual(["session-b"]);
    expect(calls).toEqual(["codex-a/session-b", "codex-b/session-b"]);
  });

  it("keeps failed or empty lookups quiet until the reply changes and preserves the file path fallback", async () => {
    const cache = new CodexFileLinkLabelCache();
    let requests = 0;
    const load = async () => { requests++; throw new Error("index unavailable"); };
    const labels = await cache.get(scope, true, load);
    expect(await cache.get(scope, true, load)).toEqual([]);
    expect(requests).toBe(1);
    const text = "完整资料 (reports/a.docx)";
    const [path] = linkChoices(text, [], [], scope.last_message, labels).flat();
    expect(text.slice(path.start, path.end)).toBe("reports/a.docx");
    expect(path.target).toBe("reports/a.docx");
    await cache.get({...scope, status_since: 2}, true, load);
    await cache.get({...scope, status_since: 2, last_message: "new reply"}, true, load);
    expect(requests).toBe(3);
    const empty = new CodexFileLinkLabelCache();
    let emptyRequests = 0;
    const noIndex = async () => { emptyRequests++; return []; };
    await empty.get(scope, true, noIndex);
    await empty.get(scope, true, noIndex);
    expect(emptyRequests).toBe(1);
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
