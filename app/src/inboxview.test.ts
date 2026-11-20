import { afterEach, describe, expect, it, vi } from "vitest";
import { inboxRecent, inboxCancel, type InboxReceipt } from "./api";
import { present, closeNewPopover } from "./newpopover";
import { receiptLabel, showReceipts } from "./inboxview";
vi.mock("./api", () => ({ inboxRecent: vi.fn(), inboxCancel: vi.fn() }));
vi.mock("./newpopover", () => ({
  present: vi.fn((_anchor, _which, pop) => {
    // Simulate an empty popover initially placed beneath a bottom split tab.
    pop.style.top = "740px"; pop.style.left = "900px";
    return true;
  }),
  closeNewPopover: vi.fn(),
}));

class Element {
  style: Record<string, string> = {};
  className = ""; textContent = ""; hidden = false; disabled = false;
  children: Element[] = []; attrs = new Map<string, string>();
  onclick?: () => Promise<void> | void;
  append(...nodes: Element[]) { this.children.push(...nodes); }
  replaceChildren() { this.children = []; }
  setAttribute(key: string, value: string) { this.attrs.set(key, value); }
  focus() {}
  find(cls: string): Element[] { return [...(this.className === cls ? [this] : []), ...this.children.flatMap(c => c.find(cls))]; }
}
const row = (id: string, phase: InboxReceipt["phase"] = "queued"): InboxReceipt => ({ id, summary: "消息摘要", phase, created_at: 1 });
const open = async () => {
  vi.stubGlobal("document", { createElement: () => new Element() });
  await showReceipts("codex-demo", new Element() as unknown as HTMLElement, "修复收件箱");
  return vi.mocked(present).mock.calls.at(-1)![2] as unknown as Element;
};
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); });

describe("message receipts", () => {
  it("distinguishes terminal write from native receipt", () => {
    expect(receiptLabel("written")).toBe("已写入终端，等待确认");
    expect(receiptLabel("received")).toBe("原会话已收到");
    expect(receiptLabel("unknown")).toBe("发送结果待核对");
  });
  it("renders 30 readable rows, names the conversation and only offers unsent cancellation", async () => {
    vi.mocked(inboxRecent).mockResolvedValue(Array.from({ length: 30 }, (_, n) => row(String(n), n ? "written" : "queued")));
    const pop = await open();
    expect(pop.attrs.get("role")).toBe("dialog");
    expect(pop.style.top).toBe("8px");
    expect(pop.style.left).toBe("50%");
    expect(pop.attrs.get("aria-label")).toContain("修复收件箱");
    expect(pop.find("inbox-row")).toHaveLength(30);
    expect(pop.find("inbox-row").flatMap(r => r.children).filter(c => c.textContent === "取消发送")).toHaveLength(1);
    await pop.find("inbox-head")[0].children[2].onclick!();
    expect(closeNewPopover).toHaveBeenCalledOnce();
  });
  it("shows read failures and recovers on refresh", async () => {
    vi.mocked(inboxRecent).mockRejectedValueOnce(new Error("private path"));
    const pop = await open(), error = pop.find("inbox-error")[0];
    expect(error.hidden).toBe(false);
    expect(error.textContent).not.toContain("private path");
    vi.mocked(inboxRecent).mockResolvedValue([row("one")]);
    await pop.find("inbox-head")[0].children[1].onclick!();
    expect(error.hidden).toBe(true);
    expect(pop.find("inbox-row")).toHaveLength(1);
  });
  it("blocks duplicate cancellation while the request is pending and shows success", async () => {
    vi.mocked(inboxRecent).mockResolvedValue([row("one")]);
    let finish!: (value: InboxReceipt) => void;
    vi.mocked(inboxCancel).mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const pop = await open(), cancel = pop.find("inbox-row")[0].children[3];
    await cancel.onclick!();
    const pending = cancel.onclick!();
    await cancel.onclick!();
    expect(inboxCancel).toHaveBeenCalledOnce();
    expect(cancel.disabled).toBe(true);
    vi.mocked(inboxRecent).mockResolvedValue([row("one", "cancelled")]);
    finish(row("one", "cancelled"));
    await pending;
    expect(pop.find("inbox-row")[0].children[0].textContent).toBe("已取消");
    expect(pop.find("inbox-row")[0].children).toHaveLength(3);
  });
  it("confirms cancellation, displays racing send failure and refreshes authoritative status", async () => {
    vi.mocked(inboxRecent).mockResolvedValue([row("one")]);
    vi.mocked(inboxCancel).mockRejectedValueOnce(new Error("already written"));
    const pop = await open(), cancel = pop.find("inbox-row")[0].children[3];
    await cancel.onclick!();
    expect(inboxCancel).not.toHaveBeenCalled();
    await cancel.onclick!();
    expect(inboxCancel).toHaveBeenCalledWith("codex-demo", "one");
    expect(pop.find("inbox-error")[0].hidden).toBe(false);
    vi.mocked(inboxRecent).mockResolvedValue([row("one", "written")]);
    await pop.find("inbox-head")[0].children[1].onclick!();
    expect(pop.find("inbox-row")[0].children).toHaveLength(3);
    expect(pop.find("inbox-error")[0].hidden).toBe(true);
  });
});
