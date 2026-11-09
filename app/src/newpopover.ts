import { open } from "@tauri-apps/plugin-dialog";
import { recentDirs } from "./api";
import { projectName } from "./tasks";
import { shortPath } from "./format";
import { reportError } from "./store";
import { AGENTS, type AgentSpec, effortLabel, effortsFor, modelsOf, pickFor, savePick, specOf } from "./models";
import { vendorIcon } from "./vendoricons";

let kind = "claude";
let dismiss: (() => void) | null = null;
let anchorOpen: { anchor: HTMLElement; which: string } | null = null;

export function closeNewPopover(): void {
  dismiss?.();
}

function button(text: string, className: string): HTMLButtonElement {
  const b = document.createElement("button");
  b.className = className;
  b.textContent = text;
  return b;
}

function label(text: string): HTMLElement {
  const l = document.createElement("div");
  l.className = "pop-label";
  l.textContent = text;
  return l;
}

/** Mark and name, as on the agent buttons and the "+" menu. */
function agentLabel(spec: AgentSpec): Element[] {
  const name = document.createElement("span");
  name.textContent = spec.name;
  return [vendorIcon(spec.kind, "agent-mark"), name];
}

/** A native select (one arrow drawn by CSS); the first option, "", is the CLI's own default. */
function select(options: { id: string; label: string }[], value: string, onChange: (v: string) => void): HTMLSelectElement {
  const sel = document.createElement("select");
  sel.className = "pick-select";
  for (const o of [{ id: "", label: "默认" }, ...options]) sel.append(new Option(o.label, o.id, false, o.id === value));
  sel.addEventListener("change", () => onChange(sel.value));
  return sel;
}

/** Agent buttons (mark and name), then model and effort for the chosen one. Choices are remembered per agent. */
function agentPicker(): HTMLElement {
  const box = document.createElement("div");
  const seg = document.createElement("div");
  seg.className = "seg agent-seg";
  const picks = document.createElement("div");
  picks.className = "pick-row";
  const buttons = AGENTS.map((spec) => {
    const b = button("", "seg-btn agent-btn");
    b.append(...agentLabel(spec));
    b.addEventListener("click", () => {
      kind = spec.kind;
      paint();
    });
    return [spec.kind, b] as const;
  });
  const field = (text: string, control: HTMLElement) => {
    const f = document.createElement("label");
    f.className = "pick-field";
    const t = document.createElement("span");
    t.textContent = text;
    const wrap = document.createElement("span");
    wrap.className = "select-wrap";
    wrap.append(control);
    f.append(t, wrap);
    return f;
  };
  const paint = () => {
    buttons.forEach(([k, b]) => b.setAttribute("aria-pressed", String(k === kind)));
    const spec = specOf(kind) ?? AGENTS[0];
    const models = modelsOf(spec.kind);
    const pick = pickFor(spec.kind);
    const efforts = effortsFor(models, pick.model).map((id) => ({ id, label: effortLabel(id) }));
    picks.replaceChildren(
      field("模型", select(models, pick.model, (model) => {
        savePick(spec.kind, { ...pickFor(spec.kind), model });
        paint(); // the levels on offer depend on the model
      })),
      field("思考", select(efforts, pick.effort, (effort) => savePick(spec.kind, { ...pickFor(spec.kind), effort }))),
    );
  };
  paint();
  seg.append(...buttons.map(([, b]) => b));
  box.append(seg, picks);
  return box;
}

/** Shows `pop` under `anchor` until Esc, a click elsewhere, or the window losing focus. Clicking the anchor again
 *  closes it (`which` tells popovers of one anchor apart). Returns false when it just closed that popover. */
export function present(anchor: HTMLElement, which: string, pop: HTMLElement, blurGuard: { suspended: boolean }): boolean {
  const wasOpenHere = anchorOpen?.anchor === anchor && anchorOpen.which === which;
  closeNewPopover();
  if (wasOpenHere) return false;
  anchorOpen = { anchor, which };
  document.body.append(pop);
  const rect = anchor.getBoundingClientRect();
  const { width, height } = pop.getBoundingClientRect();
  pop.style.left = `${Math.max(4, Math.min(rect.left, window.innerWidth - width - 4))}px`;
  pop.style.top = `${Math.max(4, Math.min(rect.bottom + 4, window.innerHeight - height - 4))}px`;

  const onPointer = (e: PointerEvent) => {
    if (!pop.contains(e.target as Node) && !anchor.contains(e.target as Node)) closeNewPopover();
  };
  const onKey = (e: KeyboardEvent) => {
    // Esc while picking a Pinyin candidate cancels the candidate, not the popover.
    if (e.key === "Escape" && !e.isComposing) closeNewPopover();
  };
  const onBlur = () => {
    if (!blurGuard.suspended) closeNewPopover();
  };
  window.addEventListener("pointerdown", onPointer, true);
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", onBlur);
  dismiss = () => {
    window.removeEventListener("pointerdown", onPointer, true);
    window.removeEventListener("keydown", onKey, true);
    window.removeEventListener("blur", onBlur);
    pop.remove();
    anchorOpen = null;
    dismiss = null;
  };
  return true;
}

type AddAgent = (kind: string) => Promise<void>;

/** Cmd+T / the "+" on a group: Claude, Codex, CodeBuddy or Kimi, started in the current task's folder. */
export function showAddAgentPopover(anchor: HTMLElement, add: AddAgent): void {
  const pop = document.createElement("div");
  pop.className = "popup add-pop";
  for (const spec of AGENTS) {
    const item = button("", "popup-item agent-item");
    const model = modelsOf(spec.kind).find((m) => m.id === pickFor(spec.kind).model)?.label ?? "默认模型";
    const hint = document.createElement("span");
    hint.className = "agent-hint";
    hint.textContent = model;
    item.append(...agentLabel(spec), hint);
    item.addEventListener("click", () => {
      closeNewPopover();
      add(spec.kind).catch(reportError);
    });
    pop.append(item);
  }
  present(anchor, "add-agent", pop, { suspended: false });
}

type CreateTask = (project: string, name: string, kind: string) => Promise<void>;

export interface NewTaskOptions {
  /** Folders that already have tasks. */
  projects: readonly string[];
  /** Folder to start with: the "+" of a project row, else the current task's. */
  preselect?: string;
  create: CreateTask;
}

/** Project, task name (may stay empty), Claude, Codex, CodeBuddy or Kimi. Enter or "建立" creates. */
export async function showNewTaskPopover(anchor: HTMLElement, opts: NewTaskOptions): Promise<void> {
  const guard = { suspended: false };
  const pop = document.createElement("div");
  pop.className = "popup new-pop";

  const folders = [...opts.projects];
  let selected = opts.preselect ?? folders[0] ?? null;
  const list = document.createElement("div");
  list.className = "dir-list";
  const paintList = () => {
    list.replaceChildren();
    for (const dir of folders) {
      const row = button("", "popup-item dir");
      row.setAttribute("aria-pressed", String(dir === selected));
      const name = document.createElement("span");
      name.textContent = projectName(dir);
      const full = document.createElement("span");
      full.className = "dir-path";
      full.textContent = shortPath(dir);
      full.title = dir;
      row.append(name, full);
      row.addEventListener("click", () => {
        selected = dir;
        paintList();
      });
      list.append(row);
    }
  };

  const choose = button("选择文件夹…", "popup-item");
  choose.addEventListener("click", async () => {
    // The system dialog takes focus, which would close the popover through the blur handler.
    guard.suspended = true;
    try {
      const dir = await open({ directory: true, multiple: false, title: "选择文件夹" });
      if (typeof dir === "string") {
        if (!folders.includes(dir)) folders.push(dir);
        selected = dir;
        paintList();
      }
    } catch (e) {
      reportError(e);
    } finally {
      guard.suspended = false;
      input.focus();
    }
  });

  const input = document.createElement("input");
  input.className = "text-input";
  input.placeholder = "不填就叫「任务 N」";
  input.setAttribute("aria-label", "任务名");

  const submit = document.createElement("button");
  submit.className = "btn primary";
  submit.textContent = "建立";
  const create = () => {
    if (!selected) return reportError("先选一个项目文件夹");
    const [project, name] = [selected, input.value];
    closeNewPopover();
    opts.create(project, name, kind).catch(reportError);
  };
  submit.addEventListener("click", create);
  // Enter in the name field submits this form; WebKit does not submit for the Enter that picks a Pinyin candidate.
  const nameForm = document.createElement("form");
  nameForm.className = "inline-form";
  nameForm.append(input);
  nameForm.addEventListener("submit", (e) => {
    e.preventDefault();
    create();
  });

  const foot = document.createElement("div");
  foot.className = "pop-foot";
  foot.append(submit);
  pop.append(label("项目"), list, choose, label("任务名"), nameForm, label("Agent"), agentPicker(), foot);

  paintList();
  if (!present(anchor, "new-task", pop, guard)) return;
  input.focus();

  // Folders used before that are not projects yet.
  try {
    const recent = await recentDirs();
    if (dismiss === null) return;
    for (const dir of recent) if (!folders.includes(dir)) folders.push(dir);
    selected ??= folders[0] ?? null;
    paintList();
  } catch (e) {
    reportError(e);
  }
}
