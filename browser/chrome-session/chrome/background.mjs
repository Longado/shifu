// No foreground operations, content scripts, cookies, or global tab scan.
const allowed = new Map();
const revoking = new Set();
const stoppedTabs = new Set();
const commands = new Set(['Accessibility.getFullAXTree', 'DOM.resolveNode',
  'DOM.setFileInputFiles', 'Runtime.callFunctionOn', 'Runtime.releaseObject', 'Page.getFrameTree']);
let autoEnabled = false;
let port;
let connecting;
let queue = Promise.resolve();
let reconnects = 0;
let reconnectTimer;

const enqueue = fn => {
  const result = queue.then(fn);
  queue = result.catch(() => {});
  return result;
};
const fail = code => { throw Object.assign(new Error(code), { code }); };

async function badge() {
  await chrome.action.setBadgeText({ text: autoEnabled ? port ? 'AUTO' : '!' : '' });
  await chrome.action.setTitle({ title: autoEnabled
    ? port ? 'Shifu 自动接入已开启；点击暂停全部连接' : 'Shifu 连接未就绪；点击重试'
    : 'Shifu 自动接入已暂停；点击开启' });
}

function publish() {
  try {
    port?.postMessage({ kind: 'mode', autoEnabled });
    port?.postMessage({ kind: 'bindings', bindings: [...allowed.values()] });
  } catch { /* onDisconnect revokes grants; no page operation is replayed. */ }
}

async function revoke(tabId) {
  allowed.delete(tabId);
  revoking.add(tabId);
  publish();
  try { await chrome.debugger.detach({ tabId }); } catch { /* Already detached/closed. */ }
  finally { revoking.delete(tabId); }
}

async function disable() {
  autoEnabled = false;
  clearTimeout(reconnectTimer);
  await chrome.storage.local.set({ autoConnectEnabled: false });
  publish();
  for (const tabId of [...allowed.keys()]) await revoke(tabId);
  await badge();
}

async function attachTab(tabId, expectedUrl) {
  if (!autoEnabled) fail('AUTO_DISABLED');
  if (!Number.isSafeInteger(tabId) || tabId < 1) fail('INVALID_TAB');
  let url;
  try { url = new URL(expectedUrl); } catch { fail('EXPECTED_URL_REQUIRED'); }
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) fail('PAGE_NOT_SUPPORTED');
  if (stoppedTabs.has(tabId)) fail('TAB_STOPPED');
  const tab = await chrome.tabs.get(tabId);
  if (allowed.has(tabId)) {
    const { frameTree } = await chrome.debugger.sendCommand({ tabId }, 'Page.getFrameTree');
    if (frameTree.frame.url !== url.href) fail('PAGE_CHANGED');
    return allowed.get(tabId);
  }
  if (allowed.size >= 3) fail('CAPACITY_REACHED');
  await chrome.debugger.attach({ tabId }, '1.3');
  try {
    // Read only the explicitly requested target using the installed debugger
    // permission. No activeTab, all_urls, tabs query or foreground click.
    const { frameTree } = await chrome.debugger.sendCommand({ tabId }, 'Page.getFrameTree');
    if (frameTree.frame.url !== url.href) fail('PAGE_CHANGED');
    if (!autoEnabled) fail('AUTO_DISABLED');
    const binding = { id: `chrome_${tabId}`, tabId, windowId: tab.windowId,
      url: frameTree.frame.url, grant: crypto.randomUUID() };
    allowed.set(tabId, binding);
    publish();
    return binding;
  } catch (error) { await revoke(tabId); throw error; }
}

async function command(message, channel) {
  if (!message || message.kind !== 'command' || typeof message.id !== 'string') return;
  const reply = { kind: 'reply', id: message.id };
  try {
    if (message.method === 'bridge_connect') {
      reply.result = await enqueue(() => attachTab(message.tabId, message.params?.expectedUrl));
    } else if (message.method === 'bridge_shutdown') {
      await enqueue(disable);
      reply.result = { autoEnabled: false };
    } else {
      if (!autoEnabled) fail('AUTO_DISABLED');
      if (allowed.get(message.tabId)?.grant !== message.grant || !message.grant) fail('TAB_NOT_AUTHORIZED');
      if (message.method === 'bridge_release') {
        await enqueue(() => revoke(message.tabId));
        reply.result = { released: true };
      } else if (message.method === 'tab_info') {
        const tab = await chrome.tabs.get(message.tabId);
        const { frameTree } = await chrome.debugger.sendCommand({ tabId: message.tabId }, 'Page.getFrameTree');
        reply.result = { tabId: tab.id, windowId: tab.windowId, url: frameTree.frame.url };
      } else {
        if (!commands.has(message.method)) fail('COMMAND_NOT_SUPPORTED');
        reply.result = await chrome.debugger.sendCommand({ tabId: message.tabId }, message.method, message.params ?? {});
      }
    }
  } catch (error) {
    const known = ['AUTO_DISABLED', 'INVALID_TAB', 'EXPECTED_URL_REQUIRED', 'PAGE_NOT_SUPPORTED', 'PAGE_CHANGED',
      'TAB_STOPPED', 'CAPACITY_REACHED', 'TAB_NOT_AUTHORIZED', 'COMMAND_NOT_SUPPORTED'];
    reply.error = { code: known.includes(error.code) ? error.code : 'CHROME_COMMAND_FAILED',
      phase: message.method === 'bridge_connect' ? 'not_dispatched' : 'outcome_unknown' };
  }
  try { channel.postMessage(reply); } catch { /* Keep the lost call's outcome unknown. */ }
}

async function connect() {
  if (port) return;
  if (connecting) return connecting;
  connecting = new Promise((resolve, reject) => {
    const candidate = chrome.runtime.connectNative('dev.longado.shifu.browser');
    const timer = setTimeout(() => { candidate.disconnect(); reject(new Error('HOST_TIMEOUT')); }, 8000);
    candidate.onMessage.addListener(message => {
      if (message.kind === 'ready') {
        clearTimeout(timer);
        port = candidate;
        publish();
        void badge();
        resolve();
      } else if (message.kind === 'command') void command(message, candidate);
    });
    candidate.onDisconnect.addListener(() => {
      clearTimeout(timer);
      void chrome.runtime.lastError;
      if (port === candidate) port = undefined;
      reject(new Error('HOST_DISCONNECTED'));
      for (const tabId of [...allowed.keys()]) void revoke(tabId);
      void badge();
      // Retry only the transport, at most three times; never replay page actions.
      if (autoEnabled && reconnects < 3) {
        const wait = [1000, 3000, 10000][reconnects++];
        reconnectTimer = setTimeout(() => { void connect().catch(() => {}); }, wait);
      }
    });
  });
  try { await connecting; } finally { connecting = undefined; }
}

const initialized = (async () => {
  const stored = await chrome.storage.local.get('autoConnectEnabled');
  // Enabling this version is the one-time opt-in. Later pauses persist.
  autoEnabled = stored.autoConnectEnabled !== false;
  if (stored.autoConnectEnabled === undefined) await chrome.storage.local.set({ autoConnectEnabled: true });
  await badge();
  if (autoEnabled) await connect();
})();
initialized.catch(() => {});
chrome.runtime.onStartup.addListener(() => { void initialized.catch(() => {}); });
chrome.runtime.onInstalled.addListener(() => { void initialized.catch(() => {}); });

chrome.action.onClicked.addListener(() => {
  void enqueue(async () => {
    await initialized.catch(() => {});
    if (autoEnabled && port) {
      await disable();
      const previous = port; port = undefined; previous.disconnect();
    } else {
      autoEnabled = true;
      reconnects = 0;
      stoppedTabs.clear();
      await chrome.storage.local.set({ autoConnectEnabled: true });
      await connect();
      await badge();
    }
  }).catch(() => { void badge(); });
});

chrome.debugger.onDetach.addListener(source => {
  if (allowed.has(source.tabId)) {
    allowed.delete(source.tabId);
    if (!revoking.has(source.tabId)) stoppedTabs.add(source.tabId);
    publish();
  }
});
chrome.tabs.onRemoved.addListener(tabId => {
  allowed.delete(tabId); stoppedTabs.delete(tabId); publish();
});
