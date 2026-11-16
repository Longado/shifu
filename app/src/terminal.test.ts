import type { GroupPanelPartInitParameters } from "dockview";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TerminalPanel } from "./terminal";

const boundary = vi.hoisted(() => ({
  focus: vi.fn(),
  fit: vi.fn(),
  attach: vi.fn(async () => 1),
  tracking: "none",
  resize: null as (() => void) | null,
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options = {};
    cols = 80;
    rows = 24;
    get modes() { return { mouseTrackingMode: boundary.tracking }; }
    parser = { registerOscHandler: vi.fn() };
    focus = boundary.focus;
    loadAddon() {}
    open(host: HTMLElement) {
      const screen = document.createElement("div");
      screen.className = "xterm-screen";
      host.append(screen);
    }
    attachCustomKeyEventHandler() {}
    registerLinkProvider() {}
    onData() {}
    onResize() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit = boundary.fit; } }));
vi.mock("./api", () => ({ termAttach: boundary.attach, ptyKill: vi.fn(async () => {}) }));
vi.mock("./imeinput", () => ({ catchDroppedInput: vi.fn() }));
vi.mock("./menu", () => ({ showMenu: vi.fn() }));
vi.mock("./preview", () => ({ openFile: vi.fn() }));
vi.mock("./theme", () => ({
  getTheme: () => "dark",
  terminalTheme: () => ({}),
  onThemeChange: () => () => {},
}));
vi.mock("./ptybus", () => ({ register: vi.fn(), unregister: vi.fn() }));
vi.mock("./store", () => ({
  findAgent: () => undefined,
  subscribe: () => () => {},
  reportError: vi.fn(),
}));

// Only DOM layout and xterm/IPC are stand-ins. Each test initializes the real panel and runs its observer/listener.
class ElementBoundary extends EventTarget {
  className = "";
  hidden = false;
  textContent = "";
  clientWidth = 0;
  clientHeight = 0;
  children: ElementBoundary[] = [];
  classList = { contains: (name: string) => this.className.split(" ").includes(name) };
  append(...children: ElementBoundary[]) { this.children.push(...children); }
  querySelector(selector: string): ElementBoundary | null {
    for (const child of this.children) {
      if (child.classList.contains(selector.slice(1))) return child;
      const nested = child.querySelector(selector);
      if (nested) return nested;
    }
    return null;
  }
}

let panel: TerminalPanel;

beforeEach(() => {
  vi.clearAllMocks();
  boundary.tracking = "none";
  boundary.resize = null;
  vi.stubGlobal("document", { createElement: () => new ElementBoundary() });
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: ResizeObserverCallback) {
      boundary.resize = () => callback([], this as unknown as ResizeObserver);
    }
    observe() {}
    disconnect() {}
  });
});

afterEach(() => {
  panel?.dispose();
  vi.unstubAllGlobals();
});

function setup(isActive: boolean) {
  const api = { id: "agent", group: {}, isActive, onDidGroupChange: vi.fn(), onDidActiveChange: vi.fn() };
  panel = new TerminalPanel();
  panel.init({ api, params: { agent: "agent" } } as unknown as GroupPanelPartInitParameters);
  const host = (panel.element as unknown as ElementBoundary).children[1];
  const layout = (visible = true) => {
    host.clientWidth = visible ? 800 : 0;
    host.clientHeight = visible ? 400 : 0;
    boundary.resize!();
  };
  return { api, host, layout };
}

describe("terminal first layout", () => {
  it("focuses an already-active panel once it becomes visible without an activation event", () => {
    const t = setup(true);
    t.layout(false);
    expect(boundary.focus).not.toHaveBeenCalled();
    expect(boundary.attach).not.toHaveBeenCalled();
    t.layout();
    expect(boundary.focus).toHaveBeenCalledOnce();
    expect(boundary.attach).toHaveBeenCalledWith("agent", 80, 24);
    t.layout();
    expect(boundary.focus).toHaveBeenCalledOnce();
  });

  it("does not focus a background panel when it is first laid out", () => {
    const t = setup(false);
    t.layout();
    expect(boundary.fit).toHaveBeenCalledOnce();
    expect(boundary.focus).not.toHaveBeenCalled();
  });

  it("does not steal focus when another panel was selected before its first layout", () => {
    const t = setup(true);
    t.api.isActive = false;
    t.layout();
    t.api.isActive = true;
    t.layout();
    expect(boundary.focus).not.toHaveBeenCalled();
  });
});

describe("terminal link mouse handling", () => {
  it("keeps a link click out of PTY mouse reporting so it is not opened a second time", () => {
    const t = setup(false);
    boundary.tracking = "vt200";
    const screen = t.host.querySelector(".xterm-screen")!;
    screen.className += " xterm-cursor-pointer";
    const event = Object.assign(new Event("mousedown", { cancelable: true }), { button: 0 });
    const stop = vi.spyOn(event, "stopPropagation");
    screen.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(stop).toHaveBeenCalledOnce();
    expect(boundary.focus).toHaveBeenCalledOnce();
  });

  it("leaves ordinary terminal mouse presses to the existing reporter", () => {
    const t = setup(false);
    boundary.tracking = "vt200";
    const event = Object.assign(new Event("mousedown", { cancelable: true }), { button: 0 });
    const stop = vi.spyOn(event, "stopPropagation");
    t.host.querySelector(".xterm-screen")!.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(stop).not.toHaveBeenCalled();
  });
});
