import { inboxRecent, inboxCancel, type InboxReceipt } from "./api";
import { present, closeNewPopover } from "./newpopover";
import "./inboxview.css";

export const receiptLabel = (phase: InboxReceipt["phase"]): string => ({
  queued: "等待发送", delivering: "正在发送", written: "已写入终端，等待确认",
  received: "原会话已收到", unknown: "发送结果待核对", cancelled: "已取消",
})[phase];
const button = (text: string) => Object.assign(document.createElement("button"), { type: "button", className: "btn", textContent: text });

export async function showReceipts(agent: string, anchor: HTMLElement, title = agent): Promise<void> {
  const conversation = title === agent ? agent : `${title}（${agent}）`;
  const pop = document.createElement("section");
  pop.className = "popup inbox-pop";
  pop.setAttribute("role", "dialog");
  pop.setAttribute("aria-label", `${conversation}的消息送达状态`);
  const head = Object.assign(document.createElement("div"), { className: "inbox-head" });
  const heading = Object.assign(document.createElement("strong"), { textContent: `${conversation}的消息送达状态` });
  const refresh = button("刷新"), close = button("关闭");
  close.onclick = () => { closeNewPopover(); anchor.focus(); };
  head.append(heading, refresh, close);
  const error = Object.assign(document.createElement("p"), { className: "inbox-error", hidden: true });
  error.setAttribute("role", "alert");
  const list = Object.assign(document.createElement("div"), { className: "inbox-list" });
  list.setAttribute("aria-live", "polite");
  pop.append(head, error, list);
  if (!present(anchor, "receipts", pop, { suspended: false })) return;
  // Keep this tall, asynchronously populated panel inside the viewport even from a bottom split.
  // Other popovers retain present's anchor placement and dismissal behavior.
  pop.style.top = "8px";
  pop.style.left = "50%";
  refresh.focus();
  let busy = false;
  const load = async () => {
    list.setAttribute("aria-busy", "true");
    try {
      const rows = await inboxRecent(agent);
      list.replaceChildren();
      if (!rows.length) list.textContent = "这个对话还没有排队消息";
      for (const row of rows) {
        const item = Object.assign(document.createElement("article"), { className: "inbox-row" });
        const state = Object.assign(document.createElement("strong"), { textContent: receiptLabel(row.phase) });
        const time = Object.assign(document.createElement("time"), { textContent: new Date(row.created_at * 1000).toLocaleString() });
        const summary = Object.assign(document.createElement("p"), { textContent: row.summary });
        item.append(state, time, summary);
        if (row.phase === "queued") {
          const cancel = button("取消发送");
          let armed = false;
          cancel.onclick = async () => {
            if (busy) return;
            if (!armed) { armed = true; cancel.textContent = "确认取消发送"; return; }
            busy = true; refresh.disabled = true; cancel.disabled = true; error.hidden = true;
            try { await inboxCancel(agent, row.id); await load(); }
            catch {
              error.textContent = "取消失败，消息可能已经开始发送。请刷新后核对状态。";
              error.hidden = false; cancel.disabled = false;
            } finally { busy = false; refresh.disabled = false; }
          };
          item.append(cancel);
        }
        list.append(item);
      }
    } catch {
      error.textContent = "读取消息状态失败，请刷新重试。";
      error.hidden = false;
    } finally { list.setAttribute("aria-busy", "false"); }
  };
  refresh.onclick = async () => {
    if (busy) return;
    busy = true; refresh.disabled = true; error.hidden = true;
    await load();
    busy = false; refresh.disabled = false;
  };
  busy = true; refresh.disabled = true;
  await load();
  busy = false; refresh.disabled = false;
}
