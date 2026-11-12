/** File tabs: editable Markdown and previews for text, images, PDFs, web pages, video and audio. */
import { convertFileSrc } from "@tauri-apps/api/core";
import type { GroupPanelPartInitParameters, IContentRenderer, PanelUpdateEvent, Parameters } from "dockview";
import MarkdownIt from "markdown-it";
import { fileRead, fileSave, pathOpen, pathReveal } from "./api";
import { reportError } from "./store";

const IMAGE: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
};
/** Keep the DOM small; anything longer opens in the default app instead. */
const MAX_TEXT = 2 * 1024 * 1024;
const HTML = new Set(["html", "htm"]);
const MARKDOWN = new Set(["md", "markdown", "mdown"]);
// File contents run in the app's own document: raw HTML must stay escaped.
const markdown = new MarkdownIt({ html: false, linkify: true });
/**
 * Played straight from disk over the asset protocol, which answers range requests: a 100 MB video starts at once,
 * seeks, and never goes through `file_read` (that copies the whole file over IPC and refuses anything past 30 MB).
 */
const MEDIA: Record<string, "video" | "audio"> = {
  mp4: "video",
  m4v: "video",
  mov: "video",
  webm: "video",
  mp3: "audio",
  m4a: "audio",
  aac: "audio",
  wav: "audio",
  ogg: "audio",
};
const LINE_HEIGHT = 20;

export const FILE_PREFIX = "file:";
export const isFilePanel = (id: string) => id.startsWith(FILE_PREFIX);

/** Set by the workspace: open `path` as a tab beside `fromGroup` (the terminal it was clicked in). */
let opener: ((path: string, line: number | undefined, fromGroup: unknown) => void) | null = null;
export function setFileOpener(fn: typeof opener): void {
  opener = fn;
}
export function openFile(path: string, line: number | undefined, fromGroup: unknown): void {
  opener?.(path, line, fromGroup);
}

export class FilePanel implements IContentRenderer {
  readonly element = document.createElement("div");
  private readonly body = document.createElement("div");
  private readonly urls: string[] = [];
  private loaded = false;
  private markdownToggle: HTMLButtonElement | null = null;
  private markdownSource = false;
  private line?: number;
  private path = "";
  private editor: HTMLTextAreaElement | null = null;
  private saveButton: HTMLButtonElement | null = null;
  private reloadButton: HTMLButtonElement | null = null;
  private readonly editStatus = document.createElement("span");
  private readonly editError = document.createElement("div");
  private original = "";
  private saving = false;

  constructor() {
    this.element.className = "file-panel";
    this.body.className = "file-body";
  }

  init(params: GroupPanelPartInitParameters): void {
    const path: string = params.params?.path ?? "";
    this.path = path;
    this.line = params.params?.line;
    const head = document.createElement("div");
    head.className = "file-head";
    const where = document.createElement("div");
    where.className = "file-path";
    where.textContent = path;
    where.title = path;
    const button = (label: string, run: () => Promise<void>) => {
      const b = document.createElement("button");
      b.className = "btn";
      b.textContent = label;
      b.addEventListener("click", () => void run().catch(reportError));
      return b;
    };
    head.append(where);
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    if (MARKDOWN.has(ext)) {
      this.element.classList.add("markdown-panel");
      this.markdownSource = typeof this.line === "number";
      this.markdownToggle = button(this.markdownSource ? "看预览" : "编辑", async () => {
        this.markdownSource = !this.markdownSource;
        if (!this.markdownSource && this.editor) {
          this.body.querySelector(".file-markdown")?.replaceWith(this.renderMarkdown(path, this.editor.value));
        }
        this.updateMarkdownMode();
        if (this.markdownSource) this.editor?.focus();
      });
      this.markdownToggle.disabled = true;
      this.saveButton = button("保存", () => this.saveMarkdown());
      this.saveButton.title = "保存文件（⌘S / Ctrl+S）";
      this.saveButton.disabled = true;
      this.reloadButton = button("重新载入", async () => {
        if (this.dirty && !window.confirm("重新载入会放弃未保存的修改，继续吗？")) return;
        localStorage.removeItem(this.draftKey);
        this.editor = null;
        this.editError.textContent = "";
        this.markdownSource = false;
        this.body.replaceChildren();
        await this.load(path);
      });
      this.reloadButton.hidden = true;
      this.editStatus.className = "file-edit-status";
      this.editStatus.setAttribute("role", "status");
      this.editError.className = "file-edit-error";
      this.editError.setAttribute("role", "alert");
      this.editError.hidden = true;
      head.append(this.markdownToggle, this.saveButton, this.reloadButton, this.editStatus);
      this.element.addEventListener("keydown", (event) => {
        if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s" && !event.isComposing) {
          event.preventDefault();
          event.stopPropagation();
          void this.saveMarkdown();
        }
      });
    }
    // A web page is shown rendered; the button flips to its source, which is where a clicked line number points.
    if (HTML.has(ext)) {
      const toggle = button("看源码", async () => {
        const showSource = toggle.textContent === "看源码";
        toggle.textContent = showSource ? "看页面" : "看源码";
        this.body.replaceChildren();
        this.loaded = false;
        if (showSource) await this.load(path, params.params?.line);
        else this.showPage(path);
      });
      head.append(toggle);
    }
    head.append(button("用默认程序打开", () => pathOpen(path)), button("在 Finder 中显示", () => pathReveal(path)));
    this.element.append(head, this.editError, this.body);
    if (HTML.has(ext)) this.showPage(path);
    else if (MEDIA[ext]) this.showMedia(path, MEDIA[ext]);
    else void this.load(path, params.params?.line);
  }

  /** Clicking the same path again, maybe with another line number. */
  update(event: PanelUpdateEvent<Parameters>): void {
    const line = event.params.line;
    if (typeof line === "number") {
      this.line = line;
      if (this.markdownToggle) {
        this.markdownSource = true;
        this.updateMarkdownMode();
      } else this.scrollTo(line);
    }
  }

  private updateMarkdownMode(): void {
    const preview = this.body.querySelector<HTMLElement>(".file-markdown");
    const source = this.editor ?? this.body.querySelector<HTMLElement>(".file-text");
    if (preview) preview.hidden = this.markdownSource;
    if (source) source.hidden = !this.markdownSource;
    if (this.markdownToggle) this.markdownToggle.textContent = this.markdownSource ? "看预览" : "编辑";
    this.loaded = !!source && this.markdownSource;
    this.body.scrollTop = 0;
    if (this.line) this.scrollTo(this.line);
  }

  private get draftKey(): string { return `shifu.markdownDraft:${this.path}`; }
  private get dirty(): boolean {
    return !!this.editor && this.editor.value !== this.original.replace(/\r\n?/g, "\n");
  }

  private updateEditing(): void {
    if (this.saveButton) this.saveButton.disabled = !this.dirty || this.saving;
    if (this.reloadButton) {
      this.reloadButton.hidden = !this.dirty && !this.editError.textContent;
      this.reloadButton.disabled = this.saving;
    }
    this.editStatus.textContent = this.saving ? "保存中…" : this.dirty ? "未保存" : this.editor ? "已保存" : "";
    this.editError.hidden = !this.editError.textContent;
    if (!this.editor) return;
    try {
      if (this.dirty) localStorage.setItem(this.draftKey, JSON.stringify({ original: this.original, text: this.editor.value }));
      else localStorage.removeItem(this.draftKey);
    } catch (error) {
      this.editError.textContent = `草稿未能暂存，请先保存文件：${String(error)}`;
      this.editError.hidden = false;
    }
  }

  private async saveMarkdown(): Promise<void> {
    if (!this.editor || !this.dirty || this.saving) return;
    const text = this.original.includes("\r\n") ? this.editor.value.replace(/\n/g, "\r\n") : this.editor.value;
    this.saving = true;
    this.editError.textContent = "";
    this.updateEditing();
    try {
      await fileSave(this.path, this.original, text);
      this.original = text;
    } catch (error) {
      this.editError.textContent = String(error);
    } finally {
      this.saving = false;
      this.updateEditing();
    }
  }

  private renderMarkdown(path: string, text: string): HTMLElement {
    const article = document.createElement("article");
    article.className = "file-markdown";
    article.innerHTML = markdown.render(text);
    const base = new URL(`file://${path.split("/").map(encodeURIComponent).join("/")}`);
    article.querySelectorAll("img").forEach((img) => {
      const target = markdownUrl(img.getAttribute("src") ?? "", base);
      if (!target) img.removeAttribute("src");
      else img.src = target.protocol === "file:" ? assetUrl(decodeURIComponent(target.pathname)) : target.href;
    });
    article.addEventListener("click", (event) => {
      const link = (event.target as Element).closest<HTMLAnchorElement>("a[href]");
      if (!link) return;
      event.preventDefault();
      const target = markdownUrl(link.getAttribute("href")!, base);
      if (target?.protocol === "file:") openFile(decodeURIComponent(target.pathname), undefined, undefined);
      else if (target?.protocol === "http:" || target?.protocol === "https:") void pathOpen(target.href).catch(reportError);
    });
    return article;
  }

  /**
   * Loaded from the file's own location (asset protocol), so its relative CSS, scripts and images come along.
   * Sandboxed without top navigation, so a page's script cannot move the Shifu window away.
   */
  private showPage(path: string): void {
    const frame = document.createElement("iframe");
    frame.className = "file-page";
    frame.sandbox.add("allow-scripts", "allow-same-origin", "allow-forms", "allow-modals");
    frame.src = assetUrl(path);
    this.body.append(frame);
  }

  private showMedia(path: string, kind: "video" | "audio"): void {
    const media = document.createElement(kind);
    media.className = `file-${kind}`;
    media.controls = true;
    media.preload = "metadata";
    media.addEventListener(
      "error",
      () => this.body.replaceChildren(note("这个文件 Shifu 里放不了，请用默认程序打开。")),
      { once: true },
    );
    media.src = assetUrl(path);
    this.body.append(media);
  }

  private async load(path: string, line?: number): Promise<void> {
    let bytes: Uint8Array;
    try {
      bytes = new Uint8Array(await fileRead(path));
    } catch (e) {
      this.body.append(note(String(e)));
      return;
    }
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    const blobUrl = (type: string) => {
      const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
      this.urls.push(url);
      return url;
    };
    if (IMAGE[ext]) {
      const img = document.createElement("img");
      img.className = "file-image";
      img.src = blobUrl(IMAGE[ext]);
      this.body.append(img);
    } else if (ext === "pdf") {
      const frame = document.createElement("iframe");
      frame.className = "file-pdf";
      frame.src = blobUrl("application/pdf");
      this.body.append(frame);
    } else if (bytes.subarray(0, 8000).includes(0)) {
      this.body.append(note("这不是文本文件，Shifu 里看不了，请用默认程序打开。"));
    } else {
      const text = new TextDecoder("utf-8", { ignoreBOM: true }).decode(bytes.subarray(0, MAX_TEXT));
      const pre = document.createElement("pre");
      pre.className = "file-text";
      pre.style.lineHeight = `${LINE_HEIGHT}px`;
      pre.textContent = bytes.length > MAX_TEXT ? text + "\n…（太长，后面没显示）" : text;
      if (MARKDOWN.has(ext)) {
        let editable = bytes.length <= MAX_TEXT;
        if (editable) {
          try { new TextDecoder("utf-8", { fatal: true }).decode(bytes); } catch { editable = false; }
        }
        if (editable) {
          this.original = text;
          this.editor = document.createElement("textarea");
          this.editor.className = "file-text file-editor";
          this.editor.setAttribute("aria-label", "Markdown 源码");
          this.editor.spellcheck = false;
          this.editor.wrap = "off";
          this.editor.style.lineHeight = `${LINE_HEIGHT}px`;
          this.editor.value = text;
          try {
            const raw = localStorage.getItem(this.draftKey);
            const draft: unknown = raw ? JSON.parse(raw) : null;
            if (draft && typeof draft === "object" && "original" in draft && typeof draft.original === "string" && "text" in draft && typeof draft.text === "string") {
              this.original = draft.original;
              this.editor.value = draft.text;
              this.markdownSource = true;
            }
          } catch (error) { this.editError.textContent = `草稿读取失败：${String(error)}`; }
          this.editor.addEventListener("input", () => this.updateEditing());
          this.markdownToggle!.disabled = false;
          this.body.append(this.renderMarkdown(path, this.editor.value), this.editor);
          this.updateEditing();
        } else {
          this.markdownToggle!.title = "内容未完整显示或不是 UTF-8，不能在此编辑";
          this.body.append(this.renderMarkdown(path, pre.textContent), pre);
        }
      } else this.body.append(pre);
      if (this.markdownToggle) this.updateMarkdownMode();
      else {
        this.loaded = true;
        if (line) this.scrollTo(line);
      }
    }
  }

  private scrollTo(line: number): void {
    if (this.loaded) (this.editor ?? this.body).scrollTop = Math.max(0, (line - 4) * LINE_HEIGHT);
  }

  dispose(): void {
    this.urls.forEach((u) => URL.revokeObjectURL(u));
  }
}

function markdownUrl(value: string, base: URL): URL | null {
  try {
    return new URL(value, base);
  } catch {
    return null;
  }
}

/**
 * convertFileSrc encodes every `/` as %2F, so the page's relative links resolve to the protocol root. Keep the
 * folders as real segments; only the leading slash stays encoded, since Tauri strips one `/` before decoding.
 */
function assetUrl(path: string): string {
  const base = convertFileSrc("");
  return base + "%2F" + path.replace(/^\/+/, "").split("/").map(encodeURIComponent).join("/");
}

function note(text: string): HTMLElement {
  const p = document.createElement("p");
  p.className = "file-note";
  p.textContent = text;
  return p;
}
