/** Pointer reordering keeps folder and task drags independent of native agent drag and drop. */
export function bindSidebarReorder(
  root: HTMLElement,
  scroll: HTMLElement,
  reorderProject: (paths: string[]) => void,
  reorderTask: (project: string, ids: string[]) => void,
): void {
  let drag: {
    id: number; source: HTMLElement; list: HTMLElement; x: number; startY: number; y: number; active: boolean;
  } | null = null;
  let mark: HTMLElement | null = null;
  let edge: "before" | "after" = "before";
  let frame = 0;
  let suppressClick = false;

  function showMark(next: HTMLElement | null, side: typeof edge = "before"): void {
    if (mark === next && edge === side) return;
    mark?.removeAttribute("data-sidebar-insert");
    mark = next;
    edge = side;
    if (mark) mark.dataset.sidebarInsert = edge;
  }

  function position(): string[] | null {
    if (!drag?.active || root.hidden) return null;
    const { list, source, x, y } = drag;
    const viewport = scroll.getBoundingClientRect();
    const bounds = list.getBoundingClientRect();
    if (x < viewport.left || x >= viewport.right || y < viewport.top || y >= viewport.bottom ||
        y < bounds.top - 6 || (list !== root && y > bounds.bottom + 6)) return null;

    const rows = Array.from(list.children) as HTMLElement[];
    const from = rows.indexOf(source);
    if (from < 0) return null;
    // Use the heading midpoint even when the folder or task has expanded children.
    const heading = list === root ? ".project-row" : ".task-row:not([hidden]), .task-solo > .row";
    const before = rows.findIndex((row) => {
      const rect = (row.querySelector(heading) ?? row).getBoundingClientRect();
      return y < rect.top + rect.height / 2;
    });
    const boundary = before < 0 ? rows.length : before;
    const to = boundary > from ? boundary - 1 : boundary;
    if (from === to) return null;
    showMark(rows[boundary] ?? rows[rows.length - 1], boundary === rows.length ? "after" : "before");
    const keys = rows.map((row) => (list === root ? row.dataset.projectPath : row.dataset.taskId)!);
    keys.splice(to, 0, keys.splice(from, 1)[0]);
    return keys;
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
    source.classList.remove("sidebar-drag-source");
    root.classList.remove("sidebar-dragging");
    if (root.hasPointerCapture(id)) root.releasePointerCapture(id);
  }

  root.addEventListener("pointerdown", (event) => {
    if (!event.isPrimary || event.button !== 0 || event.pointerType === "touch" || drag) return;
    suppressClick = false;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const grip = target.closest(".task-reorder-handle");
    if (!grip && target.closest("button, input, form")) return;
    const source = grip || target.closest(".task-row, .row-task")
      ? target.closest<HTMLElement>(".task")
      : target.closest(".project-row")?.parentElement;
    const list = source?.parentElement;
    if (!source || !list || list.childElementCount < 2) return;
    drag = { id: event.pointerId, source, list, x: event.clientX, startY: event.clientY, y: event.clientY, active: false };
  });

  root.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    if (!drag.active) {
      if (Math.abs(event.clientY - drag.startY) < 5) return;
      root.setPointerCapture(event.pointerId);
      drag.active = suppressClick = true;
      drag.source.classList.add("sidebar-drag-source");
      root.classList.add("sidebar-dragging");
    }
    drag.x = event.clientX;
    drag.y = event.clientY;
    event.preventDefault();
    if (!frame) frame = requestAnimationFrame(draw);
  });

  root.addEventListener("pointerup", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    drag.x = event.clientX;
    drag.y = event.clientY;
    const order = position();
    const { list } = drag;
    finish();
    if (order) {
      if (list === root) reorderProject(order);
      else reorderTask(list.dataset.projectPath!, order);
    }
  });

  root.addEventListener("click", (event) => {
    if (!suppressClick || event.detail === 0) return;
    suppressClick = false;
    event.stopPropagation();
    event.preventDefault();
  }, true);
  root.addEventListener("pointerleave", () => { if (!drag?.active) finish(); });
  root.addEventListener("keydown", (event) => {
    if (drag || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    if (!(event.target instanceof HTMLButtonElement) || !event.target.matches(".task-reorder-handle")) return;
    const source = event.target.closest<HTMLElement>(".task")!;
    const list = source.parentElement!;
    const ids = Array.from(list.children, (row) => (row as HTMLElement).dataset.taskId!);
    const from = ids.indexOf(source.dataset.taskId!);
    const to = from + (event.key === "ArrowUp" ? -1 : 1);
    event.preventDefault();
    if (to < 0 || to >= ids.length) return;
    [ids[from], ids[to]] = [ids[to], ids[from]];
    reorderTask(list.dataset.projectPath!, ids);
  });
  for (const type of ["pointercancel", "lostpointercapture"] as const) {
    root.addEventListener(type, (event) => { if (event.pointerId === drag?.id) finish(); });
  }
  window.addEventListener("blur", finish);
  window.addEventListener("keydown", (event) => { if (event.key === "Escape") finish(); });
}
