/** Reorder folders with pointer events so WKWebView doesn't have to start a native drag session. */
export function bindProjectDrag(list: HTMLElement, scroll: HTMLElement, reorder: (paths: string[]) => void): void {
  let drag: { id: number; source: HTMLElement; x: number; startY: number; y: number; active: boolean } | null = null;
  let mark: HTMLElement | null = null;
  let edge: "before" | "after" = "before";
  let frame = 0;
  let suppressClick = false;

  function showMark(next: HTMLElement | null, side: typeof edge = "before"): void {
    if (mark === next && edge === side) return;
    mark?.removeAttribute("data-project-insert");
    mark = next;
    edge = side;
    if (mark) mark.dataset.projectInsert = edge;
  }

  function position(): string[] | null {
    if (!drag?.active || list.hidden) return null;
    const viewport = scroll.getBoundingClientRect();
    if (drag.x < viewport.left || drag.x >= viewport.right || drag.y < viewport.top ||
        drag.y >= viewport.bottom || drag.y < list.getBoundingClientRect().top - 6) return null;

    const folders = Array.from(list.children) as HTMLElement[];
    const from = folders.indexOf(drag.source);
    if (from < 0) return null;
    // The top half of a heading inserts before that folder. Its lower half and its tasks insert after it.
    const y = drag.y;
    const before = folders.findIndex((folder) => {
      const rect = folder.firstElementChild!.getBoundingClientRect();
      return y < rect.top + rect.height / 2;
    });
    const boundary = before < 0 ? folders.length : before;
    const to = boundary > from ? boundary - 1 : boundary;
    if (from === to) return null;
    showMark(folders[boundary] ?? folders[folders.length - 1], boundary === folders.length ? "after" : "before");
    const paths = folders.map((folder) => folder.dataset.projectPath!);
    paths.splice(to, 0, paths.splice(from, 1)[0]);
    return paths;
  }

  function draw(): void {
    frame = 0;
    if (!drag?.active) return;
    const rect = scroll.getBoundingClientRect();
    if (drag.x >= rect.left && drag.x < rect.right && drag.y >= rect.top && drag.y < rect.bottom) {
      const speed = Math.max(0, drag.y - rect.bottom + 32) - Math.max(0, rect.top + 32 - drag.y);
      const previous = scroll.scrollTop;
      scroll.scrollTop += Math.round(speed / 3);
      if (scroll.scrollTop !== previous) frame = requestAnimationFrame(draw);
    }
    if (!position()) showMark(null);
  }

  function finish(): void {
    if (!drag) return;
    const { id, source } = drag;
    drag = null;
    cancelAnimationFrame(frame);
    frame = 0;
    showMark(null);
    source.classList.remove("project-drag-source");
    list.classList.remove("project-dragging");
    if (list.hasPointerCapture(id)) list.releasePointerCapture(id);
  }

  list.addEventListener("pointerdown", (event) => {
    if (!event.isPrimary || event.button !== 0 || event.pointerType === "touch" || drag) return;
    suppressClick = false;
    if (!(event.target instanceof Element) || event.target.closest("button")) return;
    const source = event.target.closest(".project-row")?.parentElement;
    if (source?.parentElement !== list || list.childElementCount < 2) return;
    drag = { id: event.pointerId, source, x: event.clientX, startY: event.clientY, y: event.clientY, active: false };
  });

  list.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    if (!drag.active) {
      if (Math.abs(event.clientY - drag.startY) < 5) return;
      list.setPointerCapture(event.pointerId);
      drag.active = suppressClick = true;
      drag.source.classList.add("project-drag-source");
      list.classList.add("project-dragging");
    }
    drag.x = event.clientX;
    drag.y = event.clientY;
    event.preventDefault();
    if (!frame) frame = requestAnimationFrame(draw);
  });

  list.addEventListener("pointerup", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    drag.x = event.clientX;
    drag.y = event.clientY;
    const paths = position();
    finish();
    if (paths) reorder(paths);
  });

  list.addEventListener("click", (event) => {
    if (!suppressClick || event.detail === 0) return;
    suppressClick = false;
    event.stopPropagation();
    event.preventDefault();
  }, true);
  list.addEventListener("pointerleave", () => { if (!drag?.active) finish(); });
  for (const type of ["pointercancel", "lostpointercapture"] as const) {
    list.addEventListener(type, (event) => { if (event.pointerId === drag?.id) finish(); });
  }
  window.addEventListener("blur", finish);
  window.addEventListener("keydown", (event) => { if (event.key === "Escape") finish(); });
}
