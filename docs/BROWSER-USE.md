# Shifu Browser Use

`shifu browser` 为 Shifu 中的 Agent 提供网页 DOM 快照、元素引用、点击和输入、iframe、标签页、截图及本机持久化登录。执行器是固定版本的微软官方 [Playwright CLI](https://github.com/microsoft/playwright-cli)，不依赖 Orca，也不修改个人 Chrome 配置。

网页优先走此入口；菜单、文件选择框及其他 macOS 窗口使用 [`shifu computer`](COMPUTER-USE.md)。操作入口不扩大当前任务的发送、上传、交易或前台操作授权。

## 安装与诊断

安装时需要 Node.js 20+、npm，以及本机 Chrome：

```bash
cargo install --path . --locked --force
bash scripts/install-browser.sh
shifu browser doctor
shifu browser guide
```

安装脚本按 `browser/package-lock.json` 安装依赖，将运行所需的 Node 可执行文件、许可和依赖放在 `~/.shifu/browser/runtime`；后续执行不依赖系统 PATH 中的 Node，不在操作时通过 npx 下载代码。再次安装会备份旧运行时。更新依赖前请先逐个关闭自己的浏览器会话；依赖升级可能令旧 daemon 不兼容。

`doctor` 只检查 Node 版本和运行时，不打开浏览器、不连接已有标签、不读取登录信息。它不证明 Chrome 已安装或某个网页可达；浏览器启动失败会由 `open` 返回。`guide` 和 `help` 在运行时缺失时仍可读取。

## 专用浏览器

```bash
# 默认打开可见的专用窗口并保存专用登录状态；不改用或关闭日常 Chrome。
shifu browser open https://example.com
# 后台自动化可显式关闭窗口显示；临时任务可额外加 --ephemeral，不保留登录。
shifu browser --session background open https://example.com --headless
shifu browser --session temporary open https://example.com --headless --ephemeral
shifu browser --session research open https://example.com
shifu browser --session research snapshot
shifu browser --session research click e3
shifu browser --session research fill e5 --text-stdin
shifu browser --session research tab-list
shifu browser --session research tab-new https://example.org
shifu browser --session research tab-select 0
shifu browser --session research screenshot
shifu browser --session research close
```

`e3`、`e5` 必须替换为本会话最新快照中的真实引用。页面变化、导航和切换标签之后重新观察。快照可能是内联的节点数组，也可能是 `snapshot.file` 与新增的绝对 `snapshot.path`；后一种需要调用者先读该文件。`tab-list` 保留官方 CLI 的文本结果，位于 JSON 的 `result.result` 中。

截图不返回 base64；`result.screenshot.path` 是绝对本地路径，Agent 必须使用自己的看图工具打开，拿到文件路径不代表已经看到图片。默认生成的快照和截图在本会话的 `.playwright-cli` 下。

其他命令包括 `find`、`type`、`press`、`select`、`check`、`uncheck`、`hover`、`drag`、`upload`、`go-back`、`go-forward`、`reload`、`console`、`requests`、`eval` 和 `run-code`。运行 `shifu browser help <命令>` 读取固定版本的官方参数，不猜参数或批量盲目执行。

`--text-stdin` 用于 `fill <引用>`、`type`、`eval`、`run-code`，放在命令最后，输入上限 1 MiB。文字会交给浏览器执行器，可能出现在其操作记录和页面内容中，不适合当作秘密存储。相对的上传、配置和输出路径以调用者当前目录解析，而不是内部的会话目录。

## 已有浏览器与登录状态

如需控制日常 Chrome 的现有标签，先由本人通过浏览器调试入口或官方 Playwright 扩展授权，再显式连接：

```bash
shifu browser --session existing attach --cdp=http://127.0.0.1:9222
# 或由本人安装、授权 Playwright 扩展后使用：
shifu browser --session existing attach --extension=chrome
shifu browser --session existing tab-list
shifu browser --session existing detach
```

Shifu 不自动打开 Chrome 的远程调试，不复制个人配置或导出凭据。Chrome 默认配置的调试限制见 [Chrome 官方说明](https://developer.chrome.com/blog/remote-debugging-port)；不要为连接关闭其保护。扩展与浏览器的连接批准由本人完成，不代点。`detach` 只断开控制，外部浏览器继续运行。

默认的专用持久化配置与连接已有浏览器是两种明确的模式。`--ephemeral` 只使用内存配置，关闭浏览器后不保留其登录状态；它不能与 `--persistent` 或 `--profile` 同用。

## 会话、响应和结果不明

- 默认会话名来自 `SHIFU_ID`，外部终端默认 `default`。`--session 名字` 放在命令前；会话名不能是路径。不同 Agent 的会话、配置和输出目录独立，切换项目目录不会丢失当前浏览器。
- 每个会话在 `~/.shifu/browser/sessions/<名字>` 下拥有私有工作区。同一会话命令由文件锁互斥；并发操作返回 `session_busy`，不会换会话或自动重试。不暴露全局 `kill-all`、`close-all`，不清理其他 Playwright 工作区。
- 操作统一输出 JSON，`ok` 同时核对进程退出状态和官方 `isError` 标记。保留原始结果，不把命令成功当作上传、保存、发送或支付完成。
- 等待上限默认 45 秒，可在命令前设置 `--timeout-ms 1000..120000`。超时只停止等待端；页面动作可能已经发生或仍在执行。返回 `result_unknown`、`automaticRetry:false`，先观察再决定下一步。不要重发点击、Enter 或提交。
- `SHIFU_BROWSER_HOME`、`SHIFU_BROWSER_RUNTIME` 和 `SHIFU_BROWSER_NODE` 可显式指向隔离的绝对路径，用于开发或故障检查。Playwright 自己的 daemon 元数据保留在其标准本机缓存中；Shifu 的登录配置与页面产物不放进项目仓库。

## 本机复查

```bash
python3 scripts/verify-browser.py /tmp/shifu-browser-new --cli /absolute/path/to/shifu
```

目录必须尚不存在。脚本只启动自己的无头会话和临时 localhost 页面，核对中文输入、点击回读、失效引用、iframe、标签、截图、会话隔离及专用配置重开；结束时逐个关闭自己的会话。原始响应、截图和 `report.json` 保留在指定目录，不操作日常浏览器。这是脚本驱动的本地功能检查，不代表 Agent 在任意网站上的任务成功率。
