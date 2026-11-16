import { randomUUID } from 'node:crypto';
import { findText } from './find.mjs';

export class BrowserError extends Error {
  constructor(code, message, phase = 'not_dispatched') {
    super(message);
    this.code = code;
    this.phase = phase;
  }
}

export function targetKey({ role, name }) { return JSON.stringify([role, name]); }

export class PlaywrightProvider {
  constructor(connect, { actionTimeout = 5000, navigationTimeout = 15000 } = {}) {
    this.connect = connect;
    this.actionTimeout = actionTimeout;
    this.navigationTimeout = navigationTimeout;
    this.owned = new Set();
  }

  supports(operation) {
    return ['navigate', 'fill_form', 'click'].includes(operation.type) && operation.expect === undefined;
  }

  async browser() {
    if (!this.connection) {
      this.connection = this.connect().then(async browser => {
        try {
          const session = await browser.newBrowserCDPSession();
          try {
            const version = await session.send('Browser.getVersion');
            if (!version.userAgent.includes('HeadlessChrome')) {
              throw new BrowserError('HEADLESS_REQUIRED', '最小版只连接独立无界面 Chromium，不接管本人浏览器。');
            }
          } finally { await session.detach(); }
          return browser;
        } catch (error) {
          // This Browser was connected, never launched: close only our connection.
          await browser.close();
          throw error;
        }
      });
    }
    return this.connection;
  }

  async open(url) {
    const browser = await this.browser();
    const context = await browser.newContext({ acceptDownloads: false });
    this.owned.add(context);
    context.setDefaultTimeout(this.actionTimeout);
    context.setDefaultNavigationTimeout(this.navigationTimeout);
    try {
      const page = await context.newPage();
      const resource = { context, page, pageId: randomUUID(), observation: null };
      // Popups stay in this owned context and are included in its eventual disposal.
      // A popup is never silently selected as the task's page.
      await page.goto(url, { waitUntil: 'domcontentloaded' });
      return resource;
    } catch (error) {
      await context.close();
      this.owned.delete(context);
      throw error;
    }
  }

  async observe(resource, scope) {
    const locator = scope ? resource.page.getByRole(scope.role, { name: scope.name, exact: true }) : resource.page.locator('body');
    const tree = await locator.ariaSnapshotJSON({ timeout: this.actionTimeout });
    const text = JSON.stringify(tree, null, 2);
    if (Buffer.byteLength(text) > 256 * 1024) {
      throw new BrowserError('OBSERVATION_TOO_LARGE', '观察内容超过256 KiB，请指定正文等区域，例如scope:{role:"main",name:""}。');
    }
    const targets = new Map();
    const visit = node => {
      if (!node || typeof node !== 'object') return;
      if (Array.isArray(node)) { node.forEach(visit); return; }
      if (node.role && typeof node.name === 'string') {
        const key = targetKey(node);
        targets.set(key, (targets.get(key) || 0) + 1);
      }
      if (node.children) visit(node.children);
    };
    visit(tree);
    resource.observation = { id: randomUUID(), text, targets, locator, scope };
    return resource.observation;
  }

  async find(resource, queries, scope) {
    const locator = scope ? resource.page.getByRole(scope.role, { name: scope.name, exact: true }) : resource.page.locator('body');
    const url = resource.page.url();
    const text = await locator.innerText({ timeout: this.actionTimeout });
    if (resource.page.url() !== url) {
      throw new BrowserError('PAGE_CHANGED', '读取期间页面已跳转，请在当前页重新查找。');
    }
    if (Buffer.byteLength(text) > 256 * 1024) {
      throw new BrowserError('READ_TOO_LARGE', '正文超过256 KiB，请缩小scope后查找。');
    }
    return { url, results: findText(text, queries) };
  }

  async act(resource, operation) {
    const observation = resource.observation;
    const locate = target => observation.scope && targetKey(observation.scope) === targetKey(target)
      ? observation.locator : observation.locator.getByRole(target.role, { name: target.name, exact: true });
    if (operation.type === 'fill_form') {
      for (const field of operation.fields) await locate(field).fill(field.value);
    } else if (operation.type === 'click') {
      await locate(operation.target).click();
    } else if (operation.type === 'navigate') {
      await resource.page.goto(operation.url, { waitUntil: 'domcontentloaded' });
    }
    resource.observation = null;
  }

  async close(resource) {
    await resource.context.close();
    if (!resource.context.pages().every(page => page.isClosed())) {
      throw new BrowserError('DISPOSE_FAILED', '任务页面仍未关闭，未归还资源额度。', 'outcome_unknown');
    }
    this.owned.delete(resource.context);
  }

  async disconnect() {
    for (const context of [...this.owned]) {
      await context.close();
      this.owned.delete(context);
    }
    if (this.connection) {
      let browser;
      try { browser = await this.connection; } catch { return; } // Failed connections already reported and closed in browser().
      await browser.close();
    }
  }
}
