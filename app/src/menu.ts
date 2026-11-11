/** The right-click menu: drawn by us, so it matches the app and needs no browser default. */

export interface MenuItem {
  label: string;
  run: () => void | Promise<void>;
  danger?: boolean;
  /** A grey line under the label: what the item does, when the label alone does not say. */
  hint?: string;
  /** Shown after the first click; the action runs on the second. */
  confirmLabel?: string;
  /** Replaces the hint after the first click. */
  confirmHint?: string;
}

let close: (() => void) | null = null;

export function closeMenu(): void {
  close?.();
}

export function showMenu(x: number, y: number, items: MenuItem[]): void {
  closeMenu();
  const menu = document.createElement("div");
  menu.className = "popup ctx-menu";
  menu.setAttribute("role", "menu");
  for (const item of items) {
    const row = document.createElement("button");
    row.className = "popup-item" + (item.danger ? " danger" : "");
    row.setAttribute("role", "menuitem");
    const label = document.createElement("span");
    label.textContent = item.label;
    row.append(label);
    const hint = document.createElement("span");
    hint.className = "popup-hint";
    if (item.hint) {
      row.classList.add("with-hint");
      hint.textContent = item.hint;
      row.append(hint);
    }
    let armed = false;
    row.addEventListener("click", () => {
      if (item.confirmLabel && !armed) {
        armed = true;
        label.textContent = item.confirmLabel;
        if (item.confirmHint) hint.textContent = item.confirmHint;
        return;
      }
      closeMenu();
      void item.run();
    });
    menu.append(row);
  }
  document.body.append(menu);
  // Keep the menu inside the window.
  const { width, height } = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(4, Math.min(x, window.innerWidth - width - 4))}px`;
  menu.style.top = `${Math.max(4, Math.min(y, window.innerHeight - height - 4))}px`;

  const onPointer = (e: PointerEvent) => {
    if (!menu.contains(e.target as Node)) closeMenu();
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") closeMenu();
  };
  window.addEventListener("pointerdown", onPointer, true);
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", closeMenu);
  close = () => {
    window.removeEventListener("pointerdown", onPointer, true);
    window.removeEventListener("keydown", onKey, true);
    window.removeEventListener("blur", closeMenu);
    menu.remove();
    close = null;
  };
}
