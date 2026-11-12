import { createCloseButton } from "dockview";
import type { ITabRenderer, TabPartInitParameters } from "dockview";
import { agentKill, agentRename, agentsList } from "./api";
import { type AgentView, agentLabel, viewKey } from "./format";
import { type MenuItem, showMenu } from "./menu";
import { specOf } from "./models";
import { findAgent, reportError, setAgents, setTaskFile, subscribe } from "./store";

/** What ending does, said the same way here and in the sidebar. */
export const ENDS = "关掉它，侧栏里也拿掉";

/** Closing always offers to put the panel away or end its agent .
 *  Cmd+W only drops the panel.
 *  Right-click or double-click renames it; the name is kept on the agent, so the sidebar shows it too. */
export class AgentTab implements ITabRenderer {
  readonly element = document.createElement("div");
  private readonly dot = document.createElement("span");
  private readonly title = document.createElement("div");
  private unsubscribe: (() => void) | null = null;
  private editing = false;

  constructor() {
    // dv-default-tab brings dockview's own hover/active styling for the close button.
    this.element.className = "dv-default-tab agent-tab";
    this.dot.className = "dot";
    this.title.className = "dv-default-tab-content";
    const close = document.createElement("div");
    close.className = "dv-default-tab-action";
    close.setAttribute("role", "button");
    close.setAttribute("tabindex", "-1");
    close.setAttribute("aria-label", "收起或结束会话");
    close.setAttribute("title", "收起或结束会话");
    close.append(createCloseButton());
    this.element.append(this.dot, this.title, close);
  }

  init(params: TabPartInitParameters): void {
    const name: string = params.params?.agent ?? params.api.id;
    const close = this.element.querySelector(".dv-default-tab-action")!;
    close.addEventListener("pointerdown", (e) => e.preventDefault());
    const end = () =>
      agentKill(name)
        .then((file) => {
          setTaskFile(file);
          params.api.close();
        })
        .catch(reportError);
    const closeOnly: MenuItem = { label: "收起，继续运行", hint: "会话留在原任务中，点侧栏可再打开", run: () => params.api.close() };
    /** A working agent takes two clicks to end; putting a panel away never ends it. */
    const endItems = (agent: AgentView): MenuItem[] => {
      const who = specOf(agent.kind)?.name ?? "它";
      const working = agent.status === "running" || agent.status === "needs_approval";
      const endIt: MenuItem = working
        ? { label: `结束 ${who}`, hint: "它还在干活", confirmLabel: "确认结束", confirmHint: "再点一次就结束", danger: true, run: end }
        : { label: `结束 ${who}`, hint: ENDS, danger: true, run: end };
      return [closeOnly, endIt];
    };
    close.addEventListener("click", (event) => {
      const e = event as MouseEvent;
      e.preventDefault();
      const agent = findAgent(name);
      if (!agent) return params.api.close();
      showMenu(e.clientX, e.clientY, endItems(agent));
    });
    this.element.addEventListener("dblclick", () => this.startEdit(name));
    this.element.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const agent = findAgent(name);
      const items: MenuItem[] = [{ label: "重命名", run: () => this.startEdit(name) }];
      if (agent?.label) items.push({ label: "用回话题名", run: () => void rename(name, "") });
      // Split this tab off its group; with only one tab there is nothing to split from (the group's 分屏 button
      // opens a new agent instead).
      const group = params.api.group;
      if (group.panels.length > 1)
        items.push(
          { label: "移到下面", run: () => params.api.moveTo({ group, position: "bottom" }) },
          { label: "移到右边", run: () => params.api.moveTo({ group, position: "right" }) },
        );
      items.push(...(agent ? endItems(agent) : [closeOnly]));
      showMenu(e.clientX, e.clientY, items);
    });
    const paint = () => {
      const agent = findAgent(name);
      this.dot.className = `dot s-${agent ? viewKey(agent) : "exited"}`;
      this.title.textContent = agent ? agentLabel(agent) : name;
      // The session name stays reachable for anyone who needs it (`shifu attach`, logs).
      this.element.title = name;
    };
    this.unsubscribe = subscribe(paint);
    paint();
  }

  /** Rename in place. Enter confirms, Esc cancels; the form lets WebKit tell a Pinyin pick from a plain Enter. */
  private startEdit(name: string): void {
    if (this.editing) return;
    this.editing = true;
    const shown = this.title.textContent ?? "";
    const form = document.createElement("form");
    form.className = "inline-form";
    const input = document.createElement("input");
    input.className = "task-edit";
    input.value = shown;
    form.append(input);
    // While typing, the tab must not be dragged or let dockview take the pointer (selecting text in the input).
    const dragHost = this.element.closest<HTMLElement>("[draggable]");
    const wasDraggable = dragHost?.draggable ?? false;
    if (dragHost) dragHost.draggable = false;
    for (const type of ["pointerdown", "mousedown", "dblclick", "contextmenu"])
      input.addEventListener(type, (e) => e.stopPropagation());
    this.title.replaceWith(form);
    input.focus();
    input.select();
    let done = false;
    const finish = (save: boolean) => {
      if (done) return;
      done = true;
      this.editing = false;
      if (dragHost) dragHost.draggable = wasDraggable;
      const label = input.value.trim();
      form.replaceWith(this.title);
      if (save && label && label !== shown) {
        this.title.textContent = label;
        void rename(name, label);
      }
    };
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      finish(true);
    });
    input.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape" && !e.isComposing) finish(false);
    });
    input.addEventListener("blur", () => finish(true));
  }

  dispose(): void {
    this.unsubscribe?.();
  }
}

/** Saves the name on the agent and reloads the list, so every place that shows the agent changes at once. */
async function rename(name: string, label: string): Promise<void> {
  try {
    await agentRename(name, label);
    setAgents(await agentsList());
  } catch (e) {
    reportError(e);
  }
}
