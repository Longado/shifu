import { present } from "./newpopover";
import { PRESETS, type Theme, getTheme, resetTheme, sameTheme, setTheme } from "./theme";

const FIELDS: { key: keyof Theme; name: string }[] = [
  { key: "bg", name: "背景" },
  { key: "text", name: "文字" },
  { key: "accent", name: "强调色" },
];

/** Shifu 菜单「外观…」: four presets, then the three colors everything else is mixed from. Changes apply live. */
export function showAppearancePopover(anchor: HTMLElement): void {
  const pop = document.createElement("div");
  pop.className = "popup theme-pop";

  const label = (text: string) => Object.assign(document.createElement("div"), { className: "pop-label", textContent: text });
  const presets = document.createElement("div");
  presets.className = "theme-presets";
  const presetButtons = PRESETS.map((p) => {
    const b = document.createElement("button");
    b.className = "theme-preset";
    const chip = document.createElement("span");
    chip.className = "theme-chip";
    chip.style.background = p.theme.bg;
    chip.style.setProperty("--chip-accent", p.theme.accent);
    b.append(chip, p.name);
    b.addEventListener("click", () => {
      setTheme(p.theme);
      paint();
    });
    presets.append(b);
    return [p.theme, b] as const;
  });

  const inputs = FIELDS.map(({ key, name }) => {
    const row = document.createElement("label");
    row.className = "theme-color";
    const input = document.createElement("input");
    input.type = "color";
    input.addEventListener("input", () => {
      setTheme({ ...getTheme(), [key]: input.value });
      paint();
    });
    row.append(name, input);
    return [key, input, row] as const;
  });

  const foot = document.createElement("div");
  foot.className = "pop-foot";
  const reset = document.createElement("button");
  reset.className = "btn";
  reset.textContent = "恢复默认";
  reset.addEventListener("click", () => {
    resetTheme();
    paint();
  });
  foot.append(reset);

  const paint = () => {
    const t = getTheme();
    presetButtons.forEach(([theme, b]) => b.setAttribute("aria-pressed", String(sameTheme(theme, t))));
    inputs.forEach(([key, input]) => {
      if (input.value !== t[key]) input.value = t[key];
    });
  };
  paint();

  pop.append(label("配色"), presets, label("自定义"), ...inputs.map(([, , row]) => row), foot);
  // The system color picker takes focus from the window; that must not close the popover.
  present(anchor, "appearance", pop, { suspended: true });
}

