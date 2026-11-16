# Chrome 一次性接入

可选的本机Chrome组件。启用一次后，Agent按任务的准确标签页ID和网址后台接入，最多三个页面，不需要逐页点击扩展。工具栏显示AUTO，点击扩展图标可暂停或恢复全局连接；状态保存在当前Chrome资料中。

组件使用Chrome公开的debugger、Native Messaging和storage接口，不开启调试端口、不复制个人浏览器资料、不扫描全部标签页。普通`shifu browser`入口继续使用官方Playwright CLI；本组件使用下面的独立Node入口，不需要重启桌面App。

## 安装与开启

需要macOS、Chrome 118+及Node.js。Chrome通道自身无需安装额外npm依赖。

1. 在仓库执行`node browser/chrome-session/cli.mjs chrome-prepare --output /absolute/new-component-directory`，输出目录需尚不存在。
2. 命令返回`extensionPath`、`nativeManifest`、`chromeManifestDestination`和`runtime`。将生成的宿主JSON复制到返回的Chrome用户登记目录；已有同名登记时先核对，不覆盖未知安装。
3. 打开`chrome://extensions`，开启开发者模式，加载生成的`extensionPath`目录。更新已有组件时重新加载扩展。这个版本首次启用即开启自动接入；若之前明确暂停，则保留暂停状态，点一次图标恢复。
4. AUTO表示连接已就绪。Chrome可能显示调试提示；该提示由Chrome管理。

Chrome组件保持一个本地Node连接进程，以便后续任务页免点击接入。它不创建浏览器或启动项；全局暂停或明确停止后退出。连接意外断开只有限重连传输，旧任务保留待核查，不自动重放页面动作。

## 接入准确任务页

以下`<CLI>`为准备命令返回的`runtime`路径：

```sh
export SHIFU_BROWSER_HOME="$HOME/.shifu/browser-chrome"
node <CLI> status
node <CLI> bindings
node <CLI> request
```

通过stdin传JSON；标签页ID和网址须来自当前实际观察或自己创建的任务页：

```json
{"method":"attach","task":"application","bindingId":"chrome_123","expectedUrl":"https://example.com/application"}
```

再依次发送`join`、`claim`、`observe`和`act`。`join`返回actorId，`claim`返回lease，`observe`返回observationId；动作携带这些值、唯一requestId及从1开始连续的sequence。准确role/name在当前scope里须唯一；页面重载或目标替换后重新观察。结果未知先核查，不换requestId重发。

```json
{"method":"act","task":"application","actorId":"returned-actor","lease":{"epoch":"returned-epoch","generation":1},"observationId":"returned-observation","requestId":"fill-1","sequence":1,"operation":{"type":"fill_form","fields":[{"role":"textbox","name":"Name","value":"Approved application name"}]}}
```

支持普通文本框、准确原生select、原生file input、点击及调用者声明的结果文本。文件先由用户批准登记在私有运行目录的`files.json`，格式为`{"cv":{"path":"/absolute/approved.pdf","sha256":"64-character-sha256"}}`，文件权限0600。每次选择前读取最新登记并重核哈希，动作仅使用fileRef；不把文件选择当成服务器收件。密码及其他凭据由本人输入。

本人控制请求使用`request --control`，包括`pause`、`inspect`、`resume`和`close`。暂停后确认inFlight为false才人工输入，恢复后旧lease失效。close释放连接和三页额度，保留实际页面与登录状态；`stop`在任务释放后结束组件并暂停自动接入。新任务不需要再次点击扩展。

只支持HTTP/HTTPS主文档和原生表单。跨域iframe、自定义上传、需要可信物理点击的组件及任意网站兼容性不作通用保证。Chrome手动停止调试后，该页不会自行重新接入；结果需先核对。全局授权不代替具体文件、条款、发送或最终提交的用户授权。

## English

This optional local component attaches to an exact task tab after one-time enablement. It uses Chrome's debugger/native-messaging APIs, retains the user's existing browser session, and limits active bindings to three. No per-page toolbar click, debugging port, copied profile, or global tab scan is required. AUTO indicates readiness; the toolbar action globally pauses/resumes and persists that setting.

Prepare with `node browser/chrome-session/cli.mjs chrome-prepare --output /absolute/new-directory`, register the returned native-host manifest, then load/reload the unpacked extension. Use the returned Node CLI, set `SHIFU_BROWSER_HOME` to the Chrome runtime directory, and send requests on stdin. `attach` requires `bindingId:"chrome_<actual-tab-id>"` and the exact `expectedUrl`; continue with join/claim/observe/act. Release a task with the user control API to detach while retaining its page. A single local process remains while automatic access is enabled; stop or the toolbar pause ends it.

Ordinary text fields, exact native selects, approved native file inputs and declared text postconditions are supported. Custom widgets and cross-origin frames require further support. Page-level business authorization still applies. This source module is separate from the existing official Playwright CLI and does not update packaged desktop downloads.
