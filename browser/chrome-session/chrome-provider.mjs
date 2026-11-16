import { randomUUID, createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { BrowserError, targetKey } from './playwright.mjs';
import { findText } from './find.mjs';

const reject = (code, message) => { throw new BrowserError(code, message); };
const match = (node, target) => node.role?.value === target.role && node.name?.value === target.name && !node.ignored;
const approvedFile = file => file && isAbsolute(file.path ?? '') && /^[a-f0-9]{64}$/.test(file.sha256 ?? '');

// Only fixed operations are exposed to the broker; callers cannot submit JS/CDP.
export class ChromeProvider {
  constructor(transport, { files = {}, loadFiles, timeoutMs = 5000 } = {}) {
    this.transport = transport;
    this.timeoutMs = timeoutMs;
    this.used = new Set();
    this.files = new Map();
    this.loadFiles = loadFiles;
    this.mode = 'chrome-native-extension';
    this.releaseClosesPage = false;
    this.coverage = '一次开启自动接入后，按任务的准确tab ID和网址后台连接，最多三页；释放保留页面。限主文档和原生表单。';
    for (const [ref, file] of Object.entries(files)) {
      if (!/^[a-zA-Z0-9_-]{1,80}$/.test(ref) || !approvedFile(file)) {
        reject('INVALID_FILE', '文件需登记引用、绝对路径和SHA256。');
      }
      this.files.set(ref, file);
    }
  }

  listBindings() { return [...this.transport.bindings.values()].map(({ grant, ...binding }) => binding); }
  connectionStatus() { return { autoConnectEnabled: this.transport.autoEnabled, extensionConnected: !this.transport.closed }; }
  supports(op) { return ['fill_form', 'select_option', 'upload_file', 'click'].includes(op.type); }
  async open() { reject('ATTACH_REQUIRED', '使用准确bindingId及expectedUrl接入既有任务页；不自行开浏览器。'); }

  async attach(id, expectedUrl) {
    if (this.used.has(id)) reject('TAB_ALREADY_BOUND', '本页已有任务。');
    let binding = this.transport.bindings.get(id);
    if (!binding) {
      if (!this.transport.autoEnabled) reject('AUTO_DISABLED', '自动接入未开启，请核对全局开关。');
      const tabId = /^chrome_([1-9][0-9]*)$/.exec(id)?.[1];
      if (!tabId || !Number.isSafeInteger(Number(tabId)) || !expectedUrl) reject('EXPECTED_URL_REQUIRED', '自动接入需准确tab ID与本次观察到的网址。');
      await this.transport.request(Number(tabId), 'bridge_connect', { expectedUrl });
      binding = this.transport.bindings.get(id);
      if (!binding) reject('BINDING_NOT_FOUND', 'Chrome未返回已连接的准确任务页。');
    }
    const resource = { pageId: randomUUID(), binding: { ...binding }, observation: null,
      target: { browserId: 'chrome', windowId: binding.windowId, tabId: binding.tabId } };
    const info = await this.info(resource);
    if (expectedUrl !== undefined && info.url !== expectedUrl) reject('PAGE_CHANGED', '任务网址已变化，未接入输入。');
    this.used.add(id);
    return resource;
  }

  async cdp(resource, method, params = {}) {
    const binding = resource.binding;
    if (this.transport.bindings.get(binding.id)?.grant !== binding.grant) {
      reject('TAB_AUTHORIZATION_CHANGED', '本页授权已撤销或重新建立，旧任务不能继续。');
    }
    return this.transport.request(binding.tabId, method, params, binding.grant);
  }

  async info(resource) {
    const info = await this.cdp(resource, 'tab_info');
    if (!/^https?:\/\//.test(info.url ?? '')) reject('PAGE_NOT_SUPPORTED', '只支持HTTP/HTTPS页面。');
    if (info.windowId !== resource.binding.windowId) reject('WINDOW_CHANGED', '标签页已移动窗口，请重新授权绑定。');
    return info;
  }

  async document(resource) {
    const { frameTree } = await this.cdp(resource, 'Page.getFrameTree');
    return { frameId: frameTree.frame.id, documentId: frameTree.frame.loaderId };
  }

  async snapshot(resource, scope) {
    const before = await this.info(resource);
    const document = await this.document(resource);
    const { nodes } = await this.cdp(resource, 'Accessibility.getFullAXTree', { frameId: document.frameId });
    const after = await this.document(resource);
    const info = await this.info(resource);
    if (document.documentId !== after.documentId || before.url !== info.url) reject('PAGE_CHANGED', '读取期间页面已变化，请重新观察。');
    let selected = nodes;
    if (scope) {
      const roots = nodes.filter(node => match(node, scope));
      if (roots.length !== 1) reject('SCOPE_NOT_UNIQUE', '观察范围不是唯一准确区域。');
      const byId = new Map(nodes.map(node => [node.nodeId, node]));
      selected = [];
      const visit = node => { selected.push(node); for (const id of node.childIds ?? []) if (byId.has(id)) visit(byId.get(id)); };
      visit(roots[0]);
    }
    return { nodes: selected, url: info.url, ...document };
  }

  async observe(resource, scope) {
    resource.observation = null;
    const snapshot = await this.snapshot(resource, scope);
    const targets = new Map();
    const visible = snapshot.nodes.filter(node => !node.ignored);
    for (const node of visible) {
      if (!node.backendDOMNodeId || typeof node.name?.value !== 'string') continue;
      const key = targetKey({ role: node.role.value, name: node.name.value });
      targets.set(key, (targets.get(key) ?? 0) + 1);
    }
    // Omit control values, including password values, from the observation.
    const text = visible.map(node => JSON.stringify({ role: node.role?.value, name: node.name?.value })).join('\n');
    if (Buffer.byteLength(text) > 256 * 1024) reject('OBSERVATION_TOO_LARGE', '观察超过256 KiB，请缩小scope。');
    resource.observation = { ...snapshot, id: randomUUID(), scope, targets, text };
    return resource.observation;
  }

  node(snapshot, target) {
    const nodes = snapshot.nodes.filter(node => node.backendDOMNodeId && match(node, target));
    if (nodes.length !== 1) reject('TARGET_NOT_UNIQUE', '目标不是唯一准确控件。');
    return nodes[0];
  }

  async element(resource, node, fn, args = []) {
    const { object } = await this.cdp(resource, 'DOM.resolveNode', { backendNodeId: node.backendDOMNodeId });
    try {
      const result = await this.cdp(resource, 'Runtime.callFunctionOn', { objectId: object.objectId,
        functionDeclaration: fn.toString(), arguments: args.map(value => ({ value })), returnByValue: true, awaitPromise: true });
      if (result.exceptionDetails || !result.result || result.result.subtype === 'error') {
        throw new BrowserError('ELEMENT_CALL_FAILED', '页面控件调用失败，请核对结果。', 'outcome_unknown');
      }
      return result.result.value;
    } finally {
      await this.cdp(resource, 'Runtime.releaseObject', { objectId: object.objectId }).catch(() => {});
    }
  }

  async find(resource, queries, scope) {
    const snapshot = await this.snapshot(resource, scope);
    const root = scope ? this.node(snapshot, scope) : snapshot.nodes.find(node => node.role?.value === 'RootWebArea');
    const text = await this.element(resource, root, function () { return (this.body ?? this).innerText ?? ''; });
    if (Buffer.byteLength(text) > 256 * 1024) reject('READ_TOO_LARGE', '正文过大，请缩小scope。');
    if ((await this.info(resource)).url !== snapshot.url || (await this.document(resource)).documentId !== snapshot.documentId) {
      reject('PAGE_CHANGED', '正文读取期间页面变化，请重新读取。');
    }
    return { url: snapshot.url, results: findText(text, queries) };
  }

  async act(resource, op) {
    const observed = resource.observation;
    const current = await this.snapshot(resource, observed.scope);
    if (current.url !== observed.url || current.documentId !== observed.documentId) reject('PAGE_CHANGED', '页面已跳转或重载，旧观察不能用于输入。');
    const locate = target => {
      const node = this.node(observed, target);
      if (this.node(current, target).backendDOMNodeId !== node.backendDOMNodeId) reject('TARGET_CHANGED', '控件已替换，请重新观察。');
      return node;
    };
    const nodes = op.type === 'fill_form' ? op.fields.map(locate) : [locate(op.target)];
    let file;
    if (op.type === 'fill_form') {
      for (const node of nodes) {
        const editable = await this.element(resource, node, function () {
          return (this.tagName === 'TEXTAREA' || this.tagName === 'INPUT' && ['text', 'email', 'tel', 'url', 'search', 'number'].includes(this.type)) &&
            !this.disabled && !this.readOnly;
        });
        if (!editable) reject('HUMAN_REQUIRED', '仅填写可编辑普通文本框，凭据和其他控件交本人处理。');
      }
    } else if (op.type === 'select_option') {
      const valid = await this.element(resource, nodes[0], function (label) {
        return this.tagName === 'SELECT' && !this.disabled && !this.multiple &&
          [...this.options].filter(option => option.label === label && !option.disabled && !option.parentElement?.disabled).length === 1;
      }, [op.label]);
      if (!valid) reject('EXACT_OPTION_REQUIRED', '没有唯一且可选的准确原生选项。');
    } else if (op.type === 'upload_file') {
      const valid = await this.element(resource, nodes[0], function () { return this.tagName === 'INPUT' && this.type === 'file' && !this.disabled; });
      if (!valid) reject('NATIVE_FILE_INPUT_REQUIRED', '首版上传需准确指向原生文件输入，暂不触发自定义文件弹窗。');
      file = this.loadFiles ? (await this.loadFiles())[op.fileRef] : this.files.get(op.fileRef);
      if (!file) reject('FILE_NOT_APPROVED', '该文件引用尚未登记批准。');
      if (!approvedFile(file)) reject('INVALID_FILE', '文件登记需要绝对路径和SHA256。');
      if (createHash('sha256').update(await readFile(file.path)).digest('hex') !== file.sha256) reject('FILE_CHANGED', '文件内容已变化，未上传。');
    }
    // A later error after any mutation is unknown, even if a new call was rejected.
    resource.observation = null;
    try {
      if (op.type === 'fill_form') {
        for (let i = 0; i < nodes.length; i++) {
          const value = op.fields[i].value;
          const actual = await this.element(resource, nodes[i], function (value) {
            const prototype = this.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
            Object.getOwnPropertyDescriptor(prototype, 'value').set.call(this, value);
            this.dispatchEvent(new Event('input', { bubbles: true }));
            this.dispatchEvent(new Event('change', { bubbles: true }));
            return this.value;
          }, [value]);
          if (actual !== value) throw new Error('VALUE_NOT_VERIFIED');
        }
        return { state: 'matched', scope: 'field_values', taskCompletion: 'not_checked' };
      }
      if (op.type === 'select_option') {
        const actual = await this.element(resource, nodes[0], function (label) {
          this.selectedIndex = [...this.options].findIndex(option => option.label === label && !option.disabled && !option.parentElement?.disabled);
          this.dispatchEvent(new Event('input', { bubbles: true }));
          this.dispatchEvent(new Event('change', { bubbles: true }));
          return this.selectedOptions[0]?.label;
        }, [op.label]);
        if (actual !== op.label) throw new Error('OPTION_NOT_VERIFIED');
        return { state: 'matched', scope: 'selected_option', taskCompletion: 'not_checked' };
      }
      if (op.type === 'upload_file') {
        await this.cdp(resource, 'DOM.setFileInputFiles', { backendNodeId: nodes[0].backendDOMNodeId, files: [file.path] });
        const selected = await this.element(resource, nodes[0], function () { return this.files.length; });
        if (selected !== 1) throw new Error('FILE_NOT_VERIFIED');
        return { state: 'file_selected', scope: 'native_file_input', fileRef: op.fileRef, sha256: file.sha256,
          draftSaved: 'not_checked', taskCompletion: 'not_checked' };
      }
      await this.element(resource, nodes[0], function () { this.click(); });
      if (!op.expect) return { state: 'not_checked', scope: 'element_action', taskCompletion: 'not_checked' };
      const deadline = Date.now() + this.timeoutMs;
      do {
        const latest = await this.snapshot(resource, observed.scope);
        const matches = latest.nodes.filter(node => match(node, op.expect.target) && node.backendDOMNodeId);
        if (matches.length === 1) {
          const text = await this.element(resource, matches[0], function () { return this.innerText; });
          if (text?.trim() === op.expect.text.trim()) return { state: 'matched', scope: 'declared_condition', taskCompletion: 'not_checked' };
        }
        await delay(100);
      } while (Date.now() < deadline);
      throw new Error('POSTCONDITION_NOT_VERIFIED');
    } catch (error) {
      throw new BrowserError(error.code ?? 'ACTION_NOT_VERIFIED', '操作可能已生效，保留页面核对，不自动重做。', 'outcome_unknown');
    }
  }

  async close(resource) {
    if (this.transport.autoEnabled && this.transport.bindings.get(resource.binding.id)?.grant === resource.binding.grant) {
      await this.cdp(resource, 'bridge_release');
    }
    resource.observation = null; this.used.delete(resource.binding.id);
  }
  async disconnect() {
    if (this.transport.autoEnabled && !this.transport.closed) await this.transport.request(null, 'bridge_shutdown');
    this.used.clear();
  }
}
