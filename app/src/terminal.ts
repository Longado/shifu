import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import type { IBufferCellPosition, ILink } from "@xterm/xterm";
import type { GroupPanelPartInitParameters, IContentRenderer } from "dockview";
import { agentResume, clipboardWrite, dropSave, pathOpen, pathReveal, pathsResolve, ptyKill, ptyResize, ptyWrite, termAttach, termLeaveScroll, termShiftEnter } from "./api";
import { decodeOsc52 } from "./clipboard";
import { catchDroppedInput } from "./imeinput";
import { mouseReport } from "./mousereport";
import { type Cell, type FoundLink, fileUris, glueRows, linkChoices, shellEscape } from "./links";
import { showMenu } from "./menu";
import { openFile } from "./preview";
import { getTheme, onThemeChange, terminalTheme } from "./theme";
import { register, unregister } from "./ptybus";
import { findAgent, reportError, subscribe } from "./store";

/** Keys the app owns through the native menu; the terminal must not turn them into input. */
const APP_KEYS = new Set(["t", "w", "j"]);

/** A dockview panel holding one xterm.js bound to `tmux attach` for one agent. */
export class TerminalPanel implements IContentRenderer {
  readonly element = document.createElement("div");
  private readonly banner = document.createElement("div");
  private readonly host = document.createElement("div");
  private readonly term = new Terminal({
    // Maple Mono NF CN draws CJK at exactly two cells; Menlo leaves gaps between 汉字.
    fontFamily: '"Maple Mono NF CN", Menlo, monospace',
    fontSize: 14,
    lineHeight: 1.1,
    cursorBlink: true,
    theme: terminalTheme(getTheme()),
  });
  private readonly fit = new FitAddon();
  private name = "";
  /** The dock group this terminal sits in; file tabs open beside it. */
  private group: unknown = undefined;
  private ptyId: number | null = null;
  private attaching = false;
  private needsReattach = false;
  /** Told about everything typed, so input-method text xterm.js dropped is sent once (see imeinput.ts). */
  private typed: { sent: () => void } | null = null;
  /** A wheel turn since the last key: tmux may be in scroll mode (see `leave_scroll` in session.rs). */
  private scrolled = false;
  /** Keys go out in order, also when one first waits for tmux to leave scroll mode. */
  private outbox: Promise<unknown> = Promise.resolve();
  private resizeObserver: ResizeObserver | null = null;
  private unsubscribe: (() => void) | null = null;
  private readonly unsubscribeTheme = onThemeChange((t) => {
    this.term.options.theme = terminalTheme(t);
  });

  constructor() {
    this.element.className = "term-panel";
    this.banner.className = "term-banner";
    this.banner.hidden = true;
    const label = document.createElement("span");
    label.textContent = "已退出";
    const resume = document.createElement("button");
    resume.className = "btn";
    resume.textContent = "恢复会话";
    resume.addEventListener("click", () => agentResume(this.name).catch(reportError));
    this.banner.append(label, resume);
    this.host.className = "term-host";
    this.element.append(this.banner, this.host);
  }

  init(params: GroupPanelPartInitParameters): void {
    this.name = params.params?.agent ?? params.api.id;
    this.group = params.api.group;
    params.api.onDidGroupChange(() => (this.group = params.api.group));
    this.term.loadAddon(this.fit);
    this.term.open(this.host);
    this.term.attachCustomKeyEventHandler((e) => this.onKey(e));
    // The OSC 52 arrives after the mouse button is up, outside any click or key, so the page may not be allowed
    // to write the clipboard itself; the backend can.
    this.term.parser.registerOscHandler(52, (data) => {
      const text = decodeOsc52(data);
      if (text !== null) void clipboardWrite(text).catch(reportError);
      return true;
    });
    this.term.registerLinkProvider({ provideLinks: (y, done) => void this.provideLinks(y).then(done, () => done(undefined)) });
    if (this.term.textarea) this.typed = catchDroppedInput(this.host, this.term.textarea, (data) => this.key(data));
    this.term.onData((data) => this.key(data));
    // A file dropped here goes in as its path, as in Terminal.app; Claude and Codex attach an image path as the image.
    this.element.addEventListener("dragover", (e) => {
      if (!e.dataTransfer?.types.includes("Files")) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    });
    this.element.addEventListener("drop", (e) => {
      const data = e.dataTransfer;
      if (!data?.types.includes("Files")) return;
      e.preventDefault();
      e.stopPropagation();
      // The DataTransfer is emptied once this handler returns; take what is needed now.
      void this.dropFiles(fileUris(data.getData("text/uri-list")), Array.from(data.files)).catch(reportError);
    });
    this.term.onResize(({ cols, rows }) => {
      if (this.ptyId !== null) void ptyResize(this.ptyId, cols, rows).catch(reportError);
    });
    // A hidden panel has no size; fit and attach only once it is laid out.
    this.resizeObserver = new ResizeObserver(() => this.fitAndAttach());
    this.resizeObserver.observe(this.host);
    params.api.onDidActiveChange(({ isActive }) => {
      // Dockview schedules its overlay visibility after the activation event. Queue behind that layout frame,
      // otherwise focusing a still-hidden xterm does nothing. A newer selection or rename keeps its focus.
      if (isActive) queueMicrotask(() => requestAnimationFrame(() => {
        if (params.api.isActive && this.element.isConnected && !(document.activeElement instanceof HTMLInputElement))
          this.term.focus();
      }));
    });
    this.unsubscribe = subscribe(() => this.onAgents());
    this.onAgents();
  }

  private onKey(e: KeyboardEvent): boolean {
    // Shift+Enter means "new line" to Claude and Codex; the backend writes it into the pane (see
    // `term_shift_enter`). Other programs get Enter, as in a plain terminal. The keypress that follows the
    // keydown must be swallowed too, or xterm.js sends its own CR for it and the message is submitted.
    if (e.key === "Enter" && e.shiftKey) {
      if (e.type === "keydown") {
        const kind = findAgent(this.name)?.kind;
        if (kind === "claude" || kind === "codex" || kind === "codebuddy") this.afterScroll(() => termShiftEnter(this.name).catch(reportError));
        // Kimi's prompt takes Ctrl-J as its new line.
        else if (kind === "kimi") this.key("\n");
        else this.key("\r");
      }
      return false;
    }
    // Doubao's voice input delivers a whole sentence as one synthetic key: keydown/keypress with `key` set to the
    // text (keyCode 65, charCode its first character). xterm.js sends only `charCode`, i.e. the first character, and
    // ignores the input event that follows. A keypress only ever names a character or "Enter", so several code
    // points here are text.
    if (e.type === "keypress" && e.key !== "Enter" && [...e.key].length > 1) {
      this.key(e.key);
      return false;
    }
    if (e.type !== "keydown") return true;
    if (e.metaKey && e.key === "c" && this.term.hasSelection()) {
      void navigator.clipboard.writeText(this.term.getSelection());
      return false;
    }
    return !(e.metaKey && APP_KEYS.has(e.key.toLowerCase()));
  }

  /**
   * Links on buffer line `y` (1-based). A path a TUI wrapped continues on the next rows, so those are read together
   * (see `glueRows`); each character keeps the cell it came from.
   */
  private async provideLinks(y: number): Promise<ILink[] | undefined> {
    const buffer = this.term.buffer.active;
    const cols = this.term.cols;
    const rowAt = (i: number) => {
      const line = i >= 0 ? buffer.getLine(i) : undefined;
      if (!line) return undefined;
      const cells: Cell[] = [];
      for (let x = 0; x < cols; x++) {
        const cell = line.getCell(x);
        cells.push({ chars: cell?.getChars() ?? "", width: cell?.getWidth() ?? 1 });
      }
      return { cells, wrapped: line.isWrapped };
    };
    const { text, cells, breaks } = glueRows(rowAt, y - 1);
    const onRow = (l: FoundLink) => cells[l.start].y <= y && cells[l.end - 1].y >= y;
    const choices = linkChoices(text, breaks)
      .map((alts) => alts.filter(onRow))
      .filter((alts) => alts.length > 0);
    if (choices.length === 0) return undefined;
    const cwd = findAgent(this.name)?.cwd ?? "";
    const paths = choices.flat().filter((l) => l.kind === "path");
    const resolved = paths.length ? await pathsResolve(paths.map((l) => l.target), cwd) : [];
    const links: ILink[] = [];
    for (const alts of choices) {
      // The first choice that is a web link or names something that exists.
      const l = alts.find((a) => a.kind === "url" || resolved[paths.indexOf(a)]);
      if (!l) continue;
      const start: IBufferCellPosition = { x: cells[l.start].x, y: cells[l.start].y };
      const tail = cells[l.end - 1];
      const range = { start, end: { x: tail.x + tail.w - 1, y: tail.y } };
      if (l.kind === "url") {
        links.push({ range, text: l.target, activate: () => void pathOpen(l.target).catch(reportError) });
        continue;
      }
      const hit = resolved[paths.indexOf(l)]!;
      links.push({ range, text: hit.path, activate: (e) => this.pathMenu(e, hit.path, hit.is_dir, l.line) });
    }
    return links;
  }

  /** Real paths when the drop names them; otherwise WebKit only hands over bytes, so save a copy and use its path. */
  private async dropFiles(paths: string[], files: File[]): Promise<void> {
    const all = paths.length ? paths : await Promise.all(files.map(async (f) => dropSave(f.name, await f.arrayBuffer())));
    if (all.length === 0) return;
    this.term.paste(all.map(shellEscape).join(" ") + " ");
    this.term.focus();
  }

  private pathMenu(e: MouseEvent, path: string, isDir: boolean, line?: number): void {
    showMenu(e.clientX, e.clientY, [
      ...(isDir ? [] : [{ label: "在 Shifu 里查看", run: () => openFile(path, line, this.group) }]),
      { label: isDir ? "在 Finder 中打开" : "用默认程序打开", run: () => pathOpen(path).catch(reportError) },
      { label: "在 Finder 中显示", run: () => pathReveal(path).catch(reportError) },
    ]);
  }

  /** Typed or mouse input from xterm.js. The first key after a wheel turn takes tmux out of scroll mode first, so
   *  it reaches the program: CodeBuddy does not take the mouse, and its pickers ignored Enter after a scroll. */
  private key(data: string): void {
    this.typed?.sent(); // now, not when it goes out: the input-method check runs before a queued key leaves
    const mouse = mouseReport(data);
    if (mouse) {
      if (mouse === "wheel") this.scrolled = true;
      this.afterScroll(() => this.send(data), false);
      return;
    }
    this.afterScroll(() => this.send(data));
  }

  private afterScroll(run: () => unknown, leave = true): void {
    const scrolled = leave && this.scrolled;
    if (leave) this.scrolled = false;
    const name = this.name;
    this.outbox = this.outbox.then(async () => {
      if (scrolled) await termLeaveScroll(name).catch(reportError);
      await run();
    });
  }

  private send(data: string): void {
    if (this.ptyId !== null) void ptyWrite(this.ptyId, data).catch(reportError);
  }

  private fitAndAttach(): void {
    if (this.host.clientWidth <= 0 || this.host.clientHeight <= 0) return;
    this.fit.fit();
    if (this.ptyId === null && !this.attaching && !this.needsReattach) void this.attach();
  }

  private onAgents(): void {
    const agent = findAgent(this.name);
    this.banner.hidden = !agent?.exited;
    // The session was gone when the client died; once the agent is running again, connect to the new one.
    if (agent && !agent.exited && this.needsReattach && this.ptyId === null) {
      this.needsReattach = false;
      this.term.reset();
      this.fitAndAttach();
    }
  }

  private async attach(): Promise<void> {
    this.attaching = true;
    try {
      const id = await termAttach(this.name, this.term.cols, this.term.rows);
      this.ptyId = id;
      register(id, {
        data: (data) => this.term.write(data),
        exit: (code) => {
          unregister(id);
          if (this.ptyId !== id) return;
          this.ptyId = null;
          this.needsReattach = true;
          this.term.write(`\r\n[已断开 ${code ?? "?"}]\r\n`);
          // One retry for a client that died while its agent is still running.
          setTimeout(() => this.onAgents(), 1000);
        },
      });
    } catch (e) {
      reportError(e);
    } finally {
      this.attaching = false;
    }
  }

  dispose(): void {
    this.unsubscribe?.();
    this.unsubscribeTheme();
    this.resizeObserver?.disconnect();
    if (this.ptyId !== null) {
      unregister(this.ptyId);
      void ptyKill(this.ptyId).catch(() => {
        // The client may already have exited on its own; nothing left to kill.
      });
      this.ptyId = null;
    }
    this.term.dispose();
  }
}
