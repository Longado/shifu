/** A read-only file tab in the dock: text, images and PDFs clicked in a terminal. */
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
    head.append(where, button("用默认程序打开", () => pathOpen(path)), button("在 Finder 中显示", () => pathReveal(path)));
    this.element.append(head, this.body);
    void this.load(path, params.params?.line);
  }

  /** Clicking the same path again, maybe with another line number. */
  update(event: PanelUpdateEvent<Parameters>): void {
    const line = event.params.line;
    if (typeof line === "number") this.scrollTo(line);
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

function note(text: string): HTMLElement {
  const p = document.createElement("p");
  p.className = "file-note";
  p.textContent = text;
  return p;
}
