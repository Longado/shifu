import { createCloseButton } from "dockview";
import type { ITabRenderer, TabPartInitParameters } from "dockview";
import { agentKill, agentRename, agentsList } from "./api";
import { type AgentView, agentLabel, viewKey } from "./format";
import { type MenuItem, showMenu } from "./menu";
import { specOf } from "./models";
import { findAgent, reportError, setAgents, setTaskFile, subscribe } from "./store";

const SKIP_CLOSE_ASK_KEY = "shifu.tabCloseNoAsk";
/** What ending does, said the same way here and in the sidebar. */
export const ENDS = "关掉它，侧栏里也拿掉";
const skipCloseAsk = () => localStorage.getItem(SKIP_CLOSE_ASK_KEY) === "1";

/** Tab = status dot + agent name + close button. The close button ends the agent too and takes it off its task, so
 *  the sidebar row goes with the tab . It asks first, like the sidebar's 结束 agent ; the ask can be turned off for idle agents, never for one still working.
 *  "只关标签" in the right-click menu (and Cmd+W) only drops the panel.
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
    const closeOnly: MenuItem = { label: "只关标签", hint: "它留在侧栏，点一下就能再打开", run: () => params.api.close() };
    /** 结束 and 只关标签, the same in the close button's menu and the right-click menu (11-10 用户 saw only 只关标签
     *  on right-click and took it for an old tab). A working agent takes two clicks to end. */
    const endItems = (agent: AgentView): MenuItem[] => {
      const who = specOf(agent.kind)?.name ?? "它";
      const working = agent.status === "running" || agent.status === "needs_approval";
      const endIt: MenuItem = working
        ? { label: `结束 ${who}`, hint: "它还在干活", confirmLabel: "确认结束", confirmHint: "再点一次就结束", danger: true, run: end }
        : { label: `结束 ${who}`, hint: ENDS, danger: true, run: end };
      return [endIt, closeOnly];
    };
    close.addEventListener("click", (event) => {
      const e = event as MouseEvent;
      e.preventDefault();
      const agent = findAgent(name);
      if (!agent) return params.api.close();
      const working = agent.status === "running" || agent.status === "needs_approval";
      if (working) return showMenu(e.clientX, e.clientY, endItems(agent));
      if (skipCloseAsk()) return void end();
      showMenu(e.clientX, e.clientY, [
        ...endItems(agent),
        {
          label: "结束，以后点叉不再问",
          hint: "空闲时点叉直接结束，右键标签能改回",
          run: () => {
            localStorage.setItem(SKIP_CLOSE_ASK_KEY, "1");
            return end();
          },
        },
      ]);
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
      if (skipCloseAsk())
        items.push({ label: "点叉时先问我", run: () => localStorage.removeItem(SKIP_CLOSE_ASK_KEY) });
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
