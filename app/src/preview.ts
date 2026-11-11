/** A read-only file tab in the dock: text, images, PDFs, web pages, video and audio clicked in a terminal. */
import { convertFileSrc } from "@tauri-apps/api/core";
import type { GroupPanelPartInitParameters, IContentRenderer, PanelUpdateEvent, Parameters } from "dockview";
import { fileRead, pathOpen, pathReveal } from "./api";
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

  constructor() {
    this.element.className = "file-panel";
    this.body.className = "file-body";
  }

  init(params: GroupPanelPartInitParameters): void {
    const path: string = params.params?.path ?? "";
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
    this.element.append(head, this.body);
    if (HTML.has(ext)) this.showPage(path);
    else if (MEDIA[ext]) this.showMedia(path, MEDIA[ext]);
    else void this.load(path, params.params?.line);
  }

  /** Clicking the same path again, maybe with another line number. */
  update(event: PanelUpdateEvent<Parameters>): void {
    const line = event.params.line;
    if (typeof line === "number") this.scrollTo(line);
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
      const text = new TextDecoder().decode(bytes.subarray(0, MAX_TEXT));
      const pre = document.createElement("pre");
      pre.className = "file-text";
      pre.style.lineHeight = `${LINE_HEIGHT}px`;
      pre.textContent = bytes.length > MAX_TEXT ? text + "\n…（太长，后面没显示）" : text;
      this.body.append(pre);
      this.loaded = true;
      if (line) this.scrollTo(line);
    }
  }

  private scrollTo(line: number): void {
    if (this.loaded) this.body.scrollTop = Math.max(0, (line - 4) * LINE_HEIGHT);
  }

  dispose(): void {
    this.urls.forEach((u) => URL.revokeObjectURL(u));
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
