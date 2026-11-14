import {
  remoteStatus, remoteStart, remoteStop, remoteConfigure, remotePair,
  remoteApprove, remoteRevoke, remoteAutostart, type RemoteStatus, type RemotePair,
} from "./api";
import { present } from "./newpopover";
import { showRemoteController } from "./remotecontrol";
import "./remote.css";

function button(text: string): HTMLButtonElement {
  return Object.assign(document.createElement("button"), {type: "button", className: "btn", textContent: text});
}

function statusLabel(s: RemoteStatus): string {
  if (!s.available) return "远程组件未安装";
  if (!s.running) return s.managed ? "服务恢复中" : "未启用";
  if (!s.compatible) return "服务需要更新";
  if (!s.relay) return "中继未配置";
  if (s.status?.state === "online") return "中继已连接";
  return s.status?.state === "offline" ? s.status.error ?? "中继暂时离线" : "正在连接中继";
}

export class RemoteButton {
  readonly element = document.createElement("button");

  constructor() {
    this.element.className = "icon-btn remote-button";
    this.element.title = "远程连接";
    this.element.setAttribute("aria-label", "远程连接");
    this.element.innerHTML = '<svg width="18" height="18" viewBox="0 0 18 18" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2" y="3" width="14" height="10" rx="1.5"/><path d="M6 16h6M9 13v3M6 8h6m-2-2 2 2-2 2"/></svg>';
    this.element.addEventListener("click", () => showRemoteController(this.element, () => this.showHostSettings(), value => {this.element.dataset.online = String(value);}));
  }

  private showHostSettings(): void {
    const pop = document.createElement("section");
    pop.className = "popup remote-pop";
    pop.setAttribute("role", "dialog");
    pop.setAttribute("aria-label", "本机被控设置");
    const head = document.createElement("div");
    head.className = "remote-head";
    const title = Object.assign(document.createElement("strong"), {textContent: "本机被控设置"});
    const toggle = button("启用");
    head.append(title, toggle);
    const state = Object.assign(document.createElement("p"), {className: "remote-state", textContent: "正在读取"});
    state.setAttribute("aria-live", "polite");
    const error = Object.assign(document.createElement("p"), {className: "remote-error", hidden: true});
    error.setAttribute("role", "alert");
    const update = button("接管后台运行");
    update.hidden = true;
    const form = document.createElement("form");
    form.className = "remote-form";
    const relay = Object.assign(document.createElement("input"), {type: "url", placeholder: "https://…", required: true, autocomplete: "off"});
    const token = Object.assign(document.createElement("input"), {type: "password", autocomplete: "off", placeholder: "首次设置时填写"});
    const field = (name: string, input: HTMLInputElement) => {
      const label = document.createElement("label");
      label.append(Object.assign(document.createElement("span"), {textContent: name}), input);
      return label;
    };
    const save = button("保存");
    save.type = "submit";
    form.append(field("中继地址", relay), field("接入凭证", token), save);
    const login = document.createElement("label");
    login.className = "remote-login";
    const autostart = Object.assign(document.createElement("input"), {type: "checkbox"});
    login.append(autostart, "登录后启动");
    const pair = button("配对设备");
    const qr = Object.assign(document.createElement("div"), {className: "remote-qr", hidden: true});
    const devices = Object.assign(document.createElement("div"), {className: "remote-devices"});
    const note = Object.assign(document.createElement("p"), {className: "remote-note", textContent: "授权设备可操作本机全部Shifu会话。保持Shifu打开，并在杯子菜单选择「保持运行」。"});
    pop.append(head, state, update, error, form, login, pair, qr, devices, note);
    if (!present(this.element, "remote-host", pop, {suspended: true})) return;

    let status: RemoteStatus | null = null;
    let busy = false;
    let reading = false;
    let formLoaded = false;
    let shownDevices = "";
    let pairing: RemotePair | null = null;
    const place = () => {
      const rect = this.element.getBoundingClientRect();
      pop.style.top = "auto";
      pop.style.bottom = `${Math.max(4, window.innerHeight - rect.top + 4)}px`;
      pop.style.left = `${Math.max(4, Math.min(rect.left, window.innerWidth - pop.offsetWidth - 4))}px`;
    };
    const clearPair = () => { pairing = null; qr.replaceChildren(); qr.hidden = true; };
    const paint = () => {
      if (!status) { toggle.disabled = true; save.disabled = true; autostart.disabled = true; pair.disabled = true; return; }
      const s = status;
      const active = s.running || s.managed;
      state.textContent = statusLabel(s);
      state.title = s.status?.error ?? "";
      toggle.textContent = active ? "停用" : "启用";
      toggle.disabled = busy || !s.available;
      save.disabled = busy || !s.available;
      autostart.checked = s.autostart;
      autostart.disabled = busy || !s.running || !s.managed;
      pair.disabled = busy || !s.running || !s.relay || !s.compatible;
      update.hidden = !s.running || (s.managed && s.compatible);
      update.textContent = s.compatible ? "接管后台运行" : "更新远程服务";
      update.disabled = busy;
      if (!formLoaded) { relay.value = s.relay ?? ""; formLoaded = true; }
      token.placeholder = s.relay ? "留空保留现有凭证" : "首次设置时填写";
      if (!active) clearPair();
      const deviceKey = JSON.stringify([s.devices, s.pending]);
      if (deviceKey !== shownDevices) {
        shownDevices = deviceKey;
        devices.replaceChildren();
        if (s.pending.length) {
          devices.append(Object.assign(document.createElement("div"), {className: "remote-label", textContent: "待批准"}));
          for (const request of s.pending) {
            const row = document.createElement("div"); row.className = "remote-device";
            const approve = button("批准");
            approve.addEventListener("click", () => void change(async () => { await remoteApprove(request.id); clearPair(); }));
            row.append(Object.assign(document.createElement("span"), {textContent: request.name}), approve);
            devices.append(row);
          }
        }
        devices.append(Object.assign(document.createElement("div"), {className: "remote-label", textContent: "已授权设备"}));
        if (!s.devices.length) devices.append(Object.assign(document.createElement("p"), {className: "remote-note", textContent: "暂无设备"}));
        for (const device of s.devices) {
          const row = document.createElement("div"); row.className = "remote-device";
          row.append(Object.assign(document.createElement("span"), {textContent: device.name}));
          if (device.revoked) row.append(Object.assign(document.createElement("span"), {className: "remote-note", textContent: "已撤销"}));
          else {
            const revoke = button("撤销");
            revoke.addEventListener("click", () => void change(() => remoteRevoke(device.id)));
            row.append(revoke);
          }
          devices.append(row);
        }
      }
      devices.querySelectorAll("button").forEach(b => { b.disabled = busy; });
      if (pairing && pairing.expires <= Date.now() / 1000) {
        clearPair(); qr.hidden = false;
        qr.textContent = "配对码已过期，请重新生成。";
      }
      place();
    };
    const refresh = async () => {
      if (reading) return;
      reading = true;
      try { status = await remoteStatus(); paint(); }
      finally { reading = false; }
    };
    const showError = (e: unknown) => { error.textContent = String(e); error.hidden = false; };
    const change = async (action: () => Promise<unknown>) => {
      if (busy) return;
      busy = true; error.hidden = true; paint();
      try { await action(); await refresh(); }
      catch (e) { showError(e); }
      finally { busy = false; paint(); }
    };
    form.addEventListener("submit", e => {
      e.preventDefault();
      void change(async () => {
        await remoteConfigure(relay.value.trim(), token.value.trim());
        token.value = ""; clearPair();
      });
    });
    toggle.addEventListener("click", () => void change(status?.running || status?.managed ? remoteStop : remoteStart));
    update.addEventListener("click", () => void change(remoteStart));
    autostart.addEventListener("change", () => {
      const enabled = autostart.checked;
      void change(() => remoteAutostart(enabled));
    });
    pair.addEventListener("click", () => void change(async () => {
      pairing = await remotePair();
      qr.hidden = false;
      qr.innerHTML = pairing.svg;
      const address = Object.assign(document.createElement("input"), {type: "text", value: pairing.url, readOnly: true});
      address.setAttribute("aria-label", "配对地址"); address.className = "remote-pair-address";
      qr.append(address, Object.assign(document.createElement("p"), {className: "remote-note", textContent: "在控制端 Shifu 粘贴配对地址，或用手机扫码。十分钟内在这里批准。"}));
    }));
    paint(); place();
    void refresh().catch(showError);
    const timer = window.setInterval(() => {
      if (!pop.isConnected) { window.clearInterval(timer); window.removeEventListener("resize", place); return; }
      if (!busy) void refresh().catch(showError);
    }, 2000);
    window.addEventListener("resize", place);
  }
}
