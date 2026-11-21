import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import type { IBufferCellPosition, ILink } from "@xterm/xterm";
import type { GroupPanelPartInitParameters, IContentRenderer } from "dockview";
import { agentResume, clipboardWrite, codexFileLinkLabels, dropSave, pathOpen, pathReveal, pathsResolve, ptyKill, ptyResize, ptyWrite, termAttach, termLeaveScroll, termShiftEnter } from "./api";
import { TerminalInput } from "./terminalinput";
import { decodeOsc52 } from "./clipboard";
import { catchDroppedInput } from "./imeinput";
import { mouseReport } from "./mousereport";
import { pageNotches, ScrollStrip, wheelNotches, wheelReport } from "./scrollstrip";
import { type Cell, type FoundLink, CodexFileLinkLabelCache, fileUris, glueRows, linkChoices, needsCodexFileLinkLabels, opensOutsideShifu, parseLinkTarget, pathCandidates, shellEscape } from "./links";
import { type MenuItem, showMenu } from "./menu";
import { openFile } from "./preview";
import { getTheme, isDark, onThemeChange, terminalTheme, type Theme } from "./theme";
import { beginPtyAttach, register, unregister } from "./ptybus";
import { findAgent, reportError, subscribe } from "./store";

/** Keys the app owns through the native menu; the terminal must not turn them into input. */
const APP_KEYS = new Set(["t", "w", "j"]);

function terminalAppearance(theme: Theme) {
  return {
    theme: terminalTheme(theme),
    // A TUI can keep its own dark cells after Shifu switches to a light foreground.
    // xterm uses half this target for dim text, including the composer placeholder.
    minimumContrastRatio: isDark(theme.bg) ? 1 : 6,
  };
}

/** A dockview panel holding one xterm.js bound to `tmux attach` for one agent. */
export class TerminalPanel implements IContentRenderer {
  readonly element = document.createElement("div");
  private readonly banner = document.createElement("div");
  private readonly host = document.createElement("div");
  private readonly term = new Terminal({
    // Maple Mono NF CN draws CJK at exactly two cells ; Menlo leaves gaps between 汉字.
    fontFamily: '"Maple Mono NF CN", Menlo, monospace',
    fontSize: 14,
    lineHeight: 1.1,
    cursorBlink: true,
    ...terminalAppearance(getTheme()),
    linkHandler: {
      allowNonHttpProtocols: true,
      activate: (e, uri) => void this.openHyperlink(e, uri).catch(reportError),
    },
  });
  private readonly fit = new FitAddon();
  private name = "";
  private readonly fileLinkLabels = new CodexFileLinkLabelCache();
  /** The dock group this terminal sits in; file tabs open beside it. */
  private group: unknown = undefined;
  private ptyId: number | null = null;
  private opened = false;
  private parkTimer: number | undefined;
  private attaching = false;
  private needsReattach = false;
  /** Told about everything typed, so input-method text xterm.js dropped is sent once (see imeinput.ts). */
  private typed: { sent: () => void } | null = null;
  /** A wheel turn since the last key: tmux may be in scroll mode (see `leave_scroll` in session.rs). */
  private scrolled = false;
  /** Keys go out in order, also when one first waits for tmux to leave scroll mode. */
  private readonly outbox = new TerminalInput();
  private destroyed = false;
  private resizeObserver: ResizeObserver | null = null;
  private unsubscribe: (() => void) | null = null;
  private readonly strip = new ScrollStrip({
    wheel: (up, count) => this.wheel(up, count),
    latest: (back) => this.toLatest(back),
    page: () => pageNotches(this.term.rows, findAgent(this.name)?.kind === "claude"),
  });
  // Codex handles Ctrl+End in the transcript, including while a mouse selection holds it in place.
  private readonly followCodex = () => {
    if (this.ptyId !== null) this.key("\x1b[1;5F");
  };
  private readonly unsubscribeTheme = onThemeChange((t) => {
    this.term.options = terminalAppearance(t);
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
    this.host.append(this.strip.element);
    this.element.append(this.banner, this.host);
  }

  init(params: GroupPanelPartInitParameters): void {
    this.name = params.params?.agent ?? params.api.id;
    this.group = params.api.group;
    params.api.onDidGroupChange(() => (this.group = params.api.group));
    if (!this.opened) this.open();
    params.api.onDidActiveChange(({ isActive }) => {
      if (isActive) this.focusWhenShown(params.api);
    });
    // A panel born active (a task switch rebuilds the dock) never gets the change event; focus it here.
    if (params.api.isActive) this.focusWhenShown(params.api);
  }

  /** Dockview schedules its overlay visibility after activation; queue behind that layout frame, otherwise focusing
   *  a still-hidden xterm does nothing. A newer selection, or a text field being typed in, keeps its focus. */
  private focusWhenShown(api: GroupPanelPartInitParameters["api"]): void {
    queueMicrotask(() => requestAnimationFrame(() => {
      if (api.isActive && this.element.isConnected && !(document.activeElement instanceof HTMLInputElement))
        this.term.focus();
    }));
  }

  /** Once per xterm: a panel taken back from the pool (see `parked`) already has all of this. */
  private open(): void {
    this.opened = true;
    this.term.loadAddon(this.fit);
    this.term.open(this.host);
    // Let xterm's screen linkifier see the click, but keep it from reaching the root's PTY mouse reporter:
    // Codex also opens links on mouse input, so forwarding the same click opens a second browser tab.
    const screen = this.host.querySelector<HTMLElement>(".xterm-screen")!;
    screen.addEventListener("mousedown", (e) => {
      if (e.button === 0 && this.term.modes.mouseTrackingMode !== "none" && screen.classList.contains("xterm-cursor-pointer")) {
        e.preventDefault();
        this.term.focus();
        e.stopPropagation();
      }
    });
    this.term.attachCustomKeyEventHandler((e) => this.onKey(e));
    // The OSC 52 arrives after the mouse button is up, outside any click or key, so the page may not be allowed
    // to write the clipboard itself; the backend can.
    this.term.parser.registerOscHandler(52, (data) => {
      const text = decodeOsc52(data);
      if (text !== null) void clipboardWrite(text).catch(reportError);
      return true;
    });
    this.term.registerLinkProvider({ provideLinks: (y, done) => void this.provideLinks(y).then(done, () => done(undefined)) });
    if (this.term.textarea) {
      this.typed = catchDroppedInput(this.host, this.term.textarea, (data) => this.key(data));
      // xterm.js marks its textarea aria-multiline="false", which Chromium reports as a text field; WebKit still
      // reports a text area unless the role is set too. Doubao's voice input (hold fn) only opens on a text field
      // (11-09: it opened in Orca, never in Shifu).
      this.term.textarea.setAttribute("role", "textbox");
    }
    this.term.onData((data) => this.key(data));
    // The right button opens Shifu's menu. Caught before xterm.js sees it, so it is not reported to tmux, which has
    // `mouse on` and would draw its own menu, and a selection survives the click.
    for (const type of ["mousedown", "mouseup", "pointerdown", "pointerup"])
      this.host.addEventListener(type, (e) => {
        if ((e as MouseEvent).button === 2) e.stopPropagation();
      }, true);
    this.host.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      e.stopPropagation();
      this.contextMenu(e);
    }, true);
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
    this.resizeObserver = new ResizeObserver(() => {
      this.fitAndAttach();
      this.strip.layout();
    });
    this.resizeObserver.observe(this.host);
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
        if (kind === "claude" || kind === "codex" || kind === "codebuddy") this.afterScroll(() => termShiftEnter(this.name, this.ptyId ?? undefined).catch(reportError));
        // Kimi and Antigravity take Ctrl-J as a new line.
        else if (kind === "kimi" || kind === "antigravity") this.key("\n");
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
    const { text, cells, breaks, soft } = glueRows(rowAt, y - 1);
    const onRow = (l: FoundLink) => cells[l.start].y <= y && cells[l.end - 1].y >= y;
    const agent = findAgent(this.name);
    const sourceMessage = agent?.kind === "codex" ? agent.last_message : undefined;
    const directChoices = linkChoices(text, breaks, soft, sourceMessage);
    const renderedFileLink = needsCodexFileLinkLabels(text, directChoices);
    const labels = await this.fileLinkLabels.get(agent, renderedFileLink, codexFileLinkLabels);
    const current = findAgent(this.name);
    if (agent && (this.name !== agent.name || current?.session_id !== agent.session_id || current?.status_since !== agent.status_since || current?.last_message !== agent.last_message)) return undefined;
    const choices = (labels.length ? linkChoices(text, breaks, soft, sourceMessage, labels) : directChoices)
      .map((alts) => alts.filter(onRow))
      .filter((alts) => alts.length > 0);
    if (choices.length === 0) return undefined;
    const cwd = agent?.cwd ?? "";
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

  /** OSC 8 labels in Codex/Claude output use the same file menu as visible paths, rather than window.open. */
  private async openHyperlink(e: MouseEvent, uri: string): Promise<void> {
    const link = parseLinkTarget(uri, true);
    if (!link) return;
    if (link.kind === "url") return pathOpen(link.target);
    const paths = await pathsResolve(pathCandidates(link.target), findAgent(this.name)?.cwd ?? "");
    const hit = paths.find((p) => p !== null);
    if (!hit) throw new Error(`文件不存在：${link.target}`);
    this.pathMenu(e, hit.path, hit.is_dir, link.line);
  }

  /** Real paths when the drop names them; otherwise WebKit only hands over bytes, so save a copy and use its path. */
  private async dropFiles(paths: string[], files: File[]): Promise<void> {
    const all = paths.length ? paths : await Promise.all(files.map(async (f) => dropSave(f.name, await f.arrayBuffer())));
    if (all.length === 0) return;
    this.term.paste(all.map(shellEscape).join(" ") + " ");
    this.term.focus();
  }

  /** Session id: what `claude --resume` / `codex resume` take. Terminal id: the agent's name, as in SHIFU_ID and
   *  the `shifu` command line. */
  private contextMenu(e: MouseEvent): void {
    const copy = (text: string) => clipboardWrite(text).catch(reportError);
    const items: MenuItem[] = [];
    if (this.term.hasSelection()) {
      const selection = this.term.getSelection();
      items.push({ label: "复制", run: () => copy(selection) });
    }
    const session = findAgent(this.name)?.session_id;
    if (session) items.push({ label: "复制会话 ID", run: () => copy(session) });
    const name = this.name;
    items.push({ label: "复制终端 ID", run: () => copy(name) });
    showMenu(e.clientX, e.clientY, items);
  }

  private pathMenu(e: MouseEvent, path: string, isDir: boolean, line?: number): void {
    const view = () => openFile(path, line, this.group);
    showMenu(e.clientX, e.clientY, [
      ...(isDir || opensOutsideShifu(path) ? [] : [{ label: "在 Shifu 里查看", run: view }]),
      { label: isDir ? "在 Finder 中打开" : "用默认程序打开", run: () => pathOpen(path).catch(reportError) },
      ...(!isDir && /\.docx$/i.test(path) ? [{ label: "在 Shifu 中预览", run: view }] : []),
      { label: "在 Finder 中显示", run: () => pathReveal(path).catch(reportError) },
    ]);
  }

  /** Typed or mouse input from xterm.js. The first key after a wheel turn takes tmux out of scroll mode first, so
   *  it reaches the program: CodeBuddy does not take the mouse, and its pickers ignored Enter after a scroll. */
  private key(data: string): void {
    this.typed?.sent(); // now, not when it goes out: the input-method check runs before a queued key leaves
    const mouse = mouseReport(data);
    if (mouse) {
      if (mouse === "wheel") {
        this.scrolled = true;
        this.strip.noteWheel(wheelNotches(data));
      }
      this.afterScroll(() => this.send(data), false);
      return;
    }
    // Sending a message takes the program back to its latest output.
    if (data.includes("\r")) this.strip.reset();
    this.afterScroll(() => this.send(data));
  }

  /** Wheel turns from the scroll bar: the same reports a mouse over the middle of the terminal sends. */
  private wheel(up: boolean, count: number): void {
    const report = wheelReport(up, Math.ceil(this.term.cols / 2), Math.ceil(this.term.rows / 2));
    this.scrolled = true;
    this.strip.noteWheel(up ? count : -count);
    this.afterScroll(() => this.send(report.repeat(count)), false);
  }

  /** Back to the latest output: scroll down past where the bar went up, and leave tmux's scroll mode. Claude also
   *  gets its own jump key (Ctrl+End), which a Mac keyboard cannot send. */
  private toLatest(back: number): void {
    this.wheel(false, back + 20);
    if (findAgent(this.name)?.kind === "claude") this.key("\x1b[1;5F");
    else this.afterScroll(() => undefined);
  }

  private afterScroll(run: () => unknown, leave = true): void {
    const scrolled = leave && this.scrolled;
    if (leave) this.scrolled = false;
    const name = this.name;
    void this.outbox.enqueue(async (current, id) => {
      if (scrolled) await termLeaveScroll(name, id).catch(reportError);
      if (current()) await run();
    }).catch(reportError);
  }

  private async send(data: string): Promise<void> {
    if (this.ptyId !== null) {
      if (data) await ptyWrite(this.ptyId, data);
    }
  }

  private fitAndAttach(): void {
    if (this.host.clientWidth <= 0 || this.host.clientHeight <= 0) return;
    this.fit.fit();
    if (this.ptyId === null && !this.attaching && !this.needsReattach) void this.attach();
  }

  private onAgents(): void {
    const agent = findAgent(this.name);
    this.element.classList.toggle("codex-terminal", agent?.kind === "codex");
    this.syncScrollStrip(agent);
    this.banner.hidden = !agent?.exited;
    // The session was gone when the client died; once the agent is running again, connect to the new one.
    if (agent && !agent.exited && this.needsReattach && this.ptyId === null) {
      this.needsReattach = false;
      this.term.reset();
      this.fitAndAttach();
    }
  }

  private syncScrollStrip(agent = findAgent(this.name)): void {
    this.strip.setConnection(this.ptyId !== null && !agent?.exited, agent?.kind === "codex" ? this.followCodex : null);
  }

  private async attach(): Promise<void> {
    this.attaching = true;
    const finish = beginPtyAttach();
    try {
      const id = await termAttach(this.name, this.term.cols, this.term.rows);
      if (this.destroyed) {
        unregister(id);
        void ptyKill(id).catch(() => undefined);
        return;
      }
      this.ptyId = id;
      this.outbox.attach(id);
      this.syncScrollStrip();
      register(id, {
        data: (data) => this.term.write(data),
        exit: (code) => {
          if (this.ptyId !== id) return;
          this.outbox.invalidate();
          this.ptyId = null;
          this.strip.setConnection(false, null);
          this.strip.reset();
          this.needsReattach = true;
          this.term.write(`\r\n[已断开 ${code ?? "?"}]\r\n`);
          // One retry for a client that died while its agent is still running.
          setTimeout(() => this.onAgents(), 1000);
        },
      });
    } catch (e) {
      // During startup recovery the saved conversation may not have its new pane yet.
      // Drop keys intended for the failed attachment and reconnect on a live-agent update.
      this.outbox.invalidate();
      this.needsReattach = true;
      if (!this.destroyed && !findAgent(this.name)?.exited) reportError(e);
    } finally {
      finish();
      this.attaching = false;
    }
  }

  focus(): void {
    this.term.focus();
  }

  /**
   * The dock is cleared and rebuilt on every task switch. Destroying the xterm and its `tmux attach` each time made
   * switching back slow (new client, full repaint) and lost keys typed meanwhile. A removed panel is parked instead:
   * still attached, taken back by the next panel for the same agent (see `TerminalPanel.create`). Orca keeps hidden
   * terminals the same way, a few for a few minutes.
   */
  private static readonly parked = new Map<string, TerminalPanel>();
  private static readonly PARK_LIMIT = 6;
  private static readonly PARK_MS = 10 * 60_000;

  /** The parked panel for `agent` if there is one, else a new panel. */
  static create(agent: string): TerminalPanel {
    const panel = TerminalPanel.parked.get(agent);
    if (!panel) return new TerminalPanel();
    TerminalPanel.parked.delete(agent);
    window.clearTimeout(panel.parkTimer);
    return panel;
  }

  dispose(): void {
    const pool = TerminalPanel.parked;
    if (!this.opened || this.ptyId === null || pool.has(this.name) || findAgent(this.name)?.exited) {
      this.destroy();
      return;
    }
    pool.set(this.name, this);
    this.parkTimer = window.setTimeout(() => {
      if (pool.get(this.name) === this) {
        pool.delete(this.name);
        this.destroy();
      }
    }, TerminalPanel.PARK_MS);
    // Over the limit: the longest parked goes (a Map iterates in insertion order).
    while (pool.size > TerminalPanel.PARK_LIMIT) {
      const [name, oldest] = pool.entries().next().value!;
      pool.delete(name);
      window.clearTimeout(oldest.parkTimer);
      oldest.destroy();
    }
  }

  private destroy(): void {
    this.destroyed = true;
    this.outbox.invalidate();
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
