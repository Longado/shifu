# 更新记录

[回到 Shifu](../README.md)　[English](#english)

## 0.3.8 预发布 · 2026-11-18

下载包现已包含 0.3.7 之后的全部改动，App、CLI 和可选电脑操作组件配套提供：

- 界面：默认墨黑主题，侧栏和弹窗的辅助文字更清楚；浅色主题下 Codex 输入区为白底深字。任务的「＋」可直接打开普通终端。
- 侧栏：项目文件夹和同一项目里的任务都可拖动排序；会话多的任务可单独展开或收起。
- 用量：左下角每四秒轮到下一项额度，不再被一项占住；展开可看 Codex credits，新增 Qwen Token Plan 额度。
- 终端里的中文路径、Markdown 文件标签和多行长路径都能点开。
- 新增 `shifu web` 网页任务池、`shifu remote` 远程命令行与 MCP、后台读窗口按控件筛选，以及多个 Agent 各开工作区并行改动后一步合并。
- 0.3.7 及更早的包继续保留。

`shifu web` 的运行文件随 CLI 放在 `bin/web`，安装时一起复制（见包内安装说明）。

本包由 GitHub 云端从公开源码构建，临时签名、未公证；构建来源见包内 BUILD-INFO.json。

## 2026-11-18

- 后台读取窗口时可按控件类型、某个元素的子树或其上级筛选，只返回需要的那部分，长窗口读起来更快、更省上下文。用法见[电脑操作说明](COMPUTER-USE.md)。源码更新，下载包未重发。

- 侧栏底部用量行加入 Qwen：显示百炼 Token Plan 30 天额度的已用百分比（通过已安装的 `bl` 命令只读查询），悬停可看今天 Qwen Code 用掉的 token；查不到时保留上次的数字。源码更新，下载包未重发。

- 终端文件链接支持 URI 编码中文路径、Markdown／OSC 8 文件标签、引号或 Markdown 内的空格与括号，以及多行长路径；文件标签使用原有查看／默认程序／Finder 菜单，网页仍只打开一次。这是源码更新，既有下载包未包含。

- 新增 `shifu web`：几个 Agent 可同时在各自独立的无界面网页里查资料，同时开几个按本机空闲内存自动决定，满了排队，查完自己释放；加载慢或头部资源卡住的页面也能读到正文。用法见[网页任务池](WEB-POOL.md)。源码更新，下载包未重发。

## 0.3.7 预发布 · 2026-11-17

- 公开下载包现已包含 PR #25：原会话输入排队，以及按批次和回执显示成果、待核与停滞提示；App、CLI 和脚本配套提供。
- 保留后台窗口能力说明和严格模式限制；0.3.6 旧包继续保留。配置和用法见[后台托管说明](AUTOPILOT.md)。

## 2026-11-17

- Apple Silicon 0.3.6 预发布更新配套 App、CLI 与可选电脑操作组件，包含已合并的终端切换／输入响应、任务进度和会话状态改进。
- 新建 Claude、Codex、Kimi 会话直接获知 Shifu 自带后台窗口操作；指南说明后台截图用法及严格模式的保守限制。

- 当前源码支持给原有 Codex/Claude 会话追加持久输入，保留草稿、批准和未知发送边界，并区分写入终端与原生接收。
- 可选按回执记录本轮实际成果，在原任务下显示数量、成果待核和停滞提示；停滞会交回原总控调整，不把运行或完成自述当作达标。[配置说明](AUTOPILOT.md)。需配套更新 App/CLI；既有下载包和本机安装未随源码自动更新。

- 新增 Qwen Code / 百炼 Token Plan 入口，包含模型选择、会话恢复和状态回传；支持环境变量、本人准备的本地密钥文件或兼容密钥库。[设置](QWEN.md)。
- 补齐 `shifu workspace` 工作区接续与 `shifu resume` 退出会话恢复命令。Step 5 Preview 上下文为500000，两个 Flash 仍为128000，输出上限8192。以上为源码更新，既有下载包尚未更新。
- 每日分析可向已就绪的新 Codex 会话发送第一条指令；新增可选定时脚本，按缺失日期逐天补跑，忙时等待、未产出时有限重试。登录、目录信任和批准仍由本人处理。[配置方法](DAILY-ANALYSIS.md)。源码更新，现有下载包尚未包含。

## 2026-11-16

- Step 5 Preview 的上下文窗口提高到 50 万 token，两个 Flash 模型保持 12.8 万；更新 CLI 并重新载入会话后生效。现有安装包不会随源码自动更新。
- 新增可选[Chrome连接组件](CHROME-BRIDGE.md)：一次开启后按任务准确接入最多三个页面，支持后台原生表单和批准附件、释放保留页及全局暂停。源码模块单独使用Node入口，既有桌面下载包不随源码更新。
- StepFun Step Plan 独立模型和会话入口，支持退出后继续准确会话；另需 Node/Pi 和密钥设置。
- 后台托管受阻或完成时回访原总控；补齐人工接手保护、未知通知阻断、只读产物和图片元数据支持。
- 远程会话按 Task 分组，自动刷新保留焦点及收起状态；保留远程新建会话。
- 构建缓存采用8 GiB/3天默认值，失败构建也回收其他闲置缓存。新 Apple Silicon 预发布包含匹配 App/CLI 及托管脚本；未公证。

## 2026-11-15

- 新增每日分析常驻入口，公开版按手动请求汇总项目推进与阻碍；安全、云机和用量来源及定时任务需自行配置。远程会话可搜索并收起保留连接，终端链接避免重复打开；电脑操作新增严格后台模式。既有下载包未包含这些源码增量。

- 源码新增[轻量后台托管](AUTOPILOT.md)：按已审阅目标与截止时间跟进一至两个现有 Codex 会话，支持后台启动、只读查看和停止。新回复复用未变交付文件的完整内容，状态可查看实际文件读取与缓存次数；人工接手和未知发送会保留停止。未增加桌面按钮，既有预发布下载包未包含此脚本。
- 合盖运行被其他电源管理程序覆盖时，会响应系统设置变化并恢复本次保持；主动结束、定时到点、过热与低电保护继续生效。短暂读取失败会复核，持续异常仍安全暂停并保留原因；杯子菜单支持直接重试。[使用说明](STEAMCUP.md)。这是源码更新，既有预发布下载包尚未包含本次修复。

## 2026-11-14

- 新增 `shifu browser`：固定版本的官方 Playwright CLI，支持 DOM 快照、元素引用、iframe、标签页、截图和专用浏览器登录状态保留；已有 Chrome 通过本人授权的 CDP 或扩展显式连接。新增 Computer Use 安装脚本和 `doctor`，分别检查原生辅助程序及浏览器运行时。[浏览器使用说明](BROWSER-USE.md)。
- 恢复任务布局时清理已移到其他任务的旧会话标签，并立即保存修复结果，解决侧栏有 2 个会话、标签栏却有 3 个的问题。归属再次变化时，旧的待加入记录不会重新带回已移走的会话。新保存的布局记录会话归属，保留主动打开的跨任务视图；旧布局没有归属记录的跨任务标签需重新打开。
- 提供 Apple Silicon 远程预发布下载包，包含 App、匹配的 CLI 和中英安装说明，无需在使用端编译。
- 本地 Shifu 可经本人配置的 HTTPS 中继连接另一台 Mac，经远端批准后进入已有 Agent 终端，或选择远端项目和 Claude/Codex 新建会话；支持保存连接、撤销设备及断线不自动重发。
- 合盖偏好和侧栏用量随本次候选同步；点餐桥已移除。预发布包尚未公证；目标 Mac 首次启动、真实 Agent、跨国网络、原生中文与 iPhone 仍待实测。[安装与限制](REMOTE-RELAY.md)。

## 2026-11-13

- 应用图标更新为「方扣」：两个相扣的几何构件，深色底配白色图形。

## 2026-11-12

- Markdown 文件可以直接编辑、保存，支持 ⌘S、草稿恢复及文件冲突提示；拖标签到左右约三分之一处即可分屏。
- 标签点叉每次都可选「收起，继续运行」或「结束」；跨任务查看的会话仍留在原任务。
- 杯子菜单提供正常睡眠、保持运行、屏幕常亮、定时和合盖运行；包含低电量、过热暂停及异常退出恢复。[使用说明](STEAMCUP.md)。
- 手机网页可查看并回复电脑上已有的终端，支持中文输入和断线重连。可选本机服务需要单独安装，外网连接需配置私有网络。[安装说明](REMOTE.md)。
- 电脑操作新增 `shifu computer guide`，改进多窗口、系统选文件框和焦点检查；操作后可只返回变化的控件，动作结果不确定时保留回执。默认后台仍可能弹出应用自己的窗口。
- 新建任务支持 Antigravity；可选本机账号的模型和思考档位，并恢复原对话。点餐按钮可连接已有的本机点餐服务；本仓库不包含点餐服务及账号。

## 2026-11-11

- 电脑操作支持独立小输入窗口；按窗口 ID 或索引切换时会激活指定目标，临时前台读取后恢复原焦点窗口。附带可手动运行的 Finder/Safari 真实任务检查。

- Antigravity 额度监测：通过当前已登录的 `agy`（1.1.11+）读取[官方 `/usage` 报告](https://antigravity.google/docs/cli/commands/usage/)，底栏分 Gemini 与 Claude/GPT 显示已用周额度、5小时额度和重置时间，每5分钟刷新；失败会标明旧数据或不可用。无需 Shifu 保存账号凭据。

- 独立的 macOS 电脑操作：`shifu computer` 可读取窗口、截图、点击、输入、滚动和拖动，运行不依赖 Orca。需要给 Shifu 自有辅助程序授予系统权限；[安装与参数说明](COMPUTER-USE.md)。
- 控件可以按文字筛选、限制返回数量，等待目标文字出现；Safari 可显式选择短暂前台读取后切回。文件条件未满足会返回失败，控件赋值成功不等于文件任务完成。
- 项目经理可以选择 agent，查看当前对话状态并通过本地协作命令传话；搜索可查当前对话及其内容，侧栏可隐藏，支持上下分屏。
- 底栏显示额度与进程占用，杯子按钮控制保持运行或保持屏幕开启；退出后释放保持唤醒。文件标签支持 HTML、音视频预览。

Finder 的独立改名输入框现已可读取、输入并提交；操作结果仍应核对文件路径。浏览器后台可能只有工具栏，等待文字时应选正文中的明确片段；目前没有 DOM 操作通道。

## English

### 0.3.8 preview · November 18, 2026

The download now includes every change since 0.3.7, with a matching app, CLI and optional Computer Use helper:

- Interface: near-black default theme with more legible secondary text; in light themes the Codex composer is white with dark text. A task's "+" opens a plain terminal.
- Sidebar: drag to reorder project folders and the tasks inside a project; expand or collapse a task's conversations.
- Usage: the bottom-left footer rotates through each quota every four seconds; the expanded panel shows Codex credits, and the Qwen Token Plan quota is included.
- Terminal file links open for encoded Chinese paths, Markdown file labels and long multi-line paths.
- New `shifu web` page pool, `shifu remote` command line and MCP, filtered background window reads, and parallel agent workspaces with a single merge step.
- 0.3.7 and earlier packages remain available.

The `shifu web` runtime ships next to the CLI in `bin/web` and is copied with it (see the install guide in the package).

Built by GitHub Actions from the public source; ad-hoc signed and not notarized. The build source is recorded in BUILD-INFO.json inside the package.

### November 18, 2026

- Background window reads can be filtered by control type, by an element's subtree or by its ancestors, returning only the part needed, so long windows read faster and use less context. See the [computer-use guide](COMPUTER-USE.md). Source update; downloads not rebuilt.

- The sidebar usage footer now includes Qwen: the 30-day percentage used of the Bailian Token Plan (read-only through the installed `bl` client), with today's Qwen Code tokens on hover; the last numbers stay when the quota cannot be read. Source update; downloads not rebuilt.

- New `shifu web`: several agents can research at once, each in its own headless page. How many run together is set from free memory and CPU cores; extra tasks queue, and agents release their own task when done. Slow pages and pages whose head resources stall can still be read. See the [web task pool guide](WEB-POOL.md) (Chinese). Source only; download packages were not rebuilt.

### 0.3.7 preview · November 17, 2026

- Public downloads now include PR #25: queued input for existing conversations and batch/receipt-based outcome, verification and stall indicators, with matching app, CLI and script.
- Native-background guidance and strict-mode limits remain unchanged. The 0.3.6 package remains available. See [configuration and usage](AUTOPILOT.md#english).

### November 17, 2026

- The Apple Silicon 0.3.6 preview updates the matching app, CLI and optional Computer Use helper, including merged terminal responsiveness, task progress and session-status changes.
- New Claude, Codex and Kimi sessions receive an explicit native-background capability summary. The guide explains screenshot-only use and the conservative strict-mode limits.

- Current source adds durable queued input for existing Codex/Claude conversations, preserving drafts, approvals and unknown-send boundaries; terminal writes and native acceptance are separate receipts.
- Optional receipt-based batch outcomes appear under the existing Task, with invalid-proof and stall prompts returned to the original controller for adjustment. Running state and completion claims do not satisfy the target. [Configuration](AUTOPILOT.md#english). Matching app/CLI updates are required; existing downloads and local installations are not automatically replaced.

- Added Qwen Code / Bailian Token Plan with model selection, exact-session resume and status hooks. Credentials may come from the environment, a user-managed local file or a compatible vault. [Setup](QWEN.md#english).
- Added the existing local `shifu workspace` handoff commands and `shifu resume` for exited sessions. Step 5 Preview uses a 500000-token context; both Flash models retain 128000 and output limits remain 8192. These are source updates; existing release packages are unchanged.
- Daily analysis can send the first prompt to a fresh Codex session at its ready welcome composer. An optional scheduler catches up missing dated reports, waits for busy agents, and bounds retries when no report is produced. Login, folder trust, and approvals remain manual. [Setup](DAILY-ANALYSIS.md#english). Source update; existing release packages are unchanged.
- Terminal file links recognize URI-encoded names, Markdown and OSC 8 file labels, spaces and brackets inside quoted or Markdown destinations, and long wrapped paths. File labels use the existing viewer, default-app and Finder menu; web links still open once. This is a source update, not a new download package.

### November 16, 2026

- Raised the Step 5 Preview context window to 500,000 tokens while keeping both Flash models at 128,000. Update the CLI and reload the session to apply it; existing download packages do not update with source changes.
- Added an optional [Chrome bridge](CHROME-BRIDGE.md) with one-time enablement, up to three exact task bindings, background native forms and approved file inputs, page-preserving release and global pause. Existing desktop downloads are unchanged.
- Added StepFun Step Plan model/session entry and exact-session resume; Node/Pi and key setup are required separately.
- Bounded follow-up reports completion/blockers to the original controller, preserving human takeover and unknown-send stops; read-only artifacts and media metadata are supported.
- Remote tasks group sessions and preserve keyboard focus/collapsed state during refresh; remote session creation remains available.
- Managed builds use 8 GiB/3-day defaults and reclaim other inactive caches after failures. The new Apple Silicon preview includes matching app/CLI and the autopilot script; it is not notarized.

### November 15, 2026

Current source adds a pinned daily-analysis session for project progress and blockers. Security, cloud spend and agent usage reports require user-configured sources; scheduling is configured separately. Remote sessions support search, agent status and minimizing without disconnecting. Terminal links avoid duplicate opens from one click. Optional strict background computer use allows reads and accessibility text assignment, rejecting clicks, keystrokes and foreground fallbacks. Existing preview downloads do not include these source updates.

- Added source-only [bounded background follow-up](AUTOPILOT.md#english) for one or two existing Codex sessions, with reviewed goals, authorization and a deadline. Start detached, inspect read-only status or stop the controller. New replies reuse complete unchanged artifacts and expose file I/O/cache metrics. Human takeover and uncertain sends remain halted. No new desktop button; existing preview downloads do not include the script.
- Closed-lid operation responds to power-setting changes and restores its active hold if another power manager clears it. Explicit stop, expiry, thermal and low-battery protection retain priority. Transient reads are retried once; persistent failures still pause safely and keep the latest reason. Failed holds can be retried from the cup menu. See the [power guide](STEAMCUP.md). This source update is not included in existing preview downloads.

### 2026-11-14

- Added `shifu browser` using the pinned official Playwright CLI for DOM snapshots, element references, frames, tabs, screenshots and persistent isolated browser profiles. Existing Chrome requires explicit CDP/extension attachment approved by the user. Added installation scripts and `doctor` commands for native Computer Use and the browser runtime. [Browser guide](BROWSER-USE.md).
- Restoring a task removes and persists stale tabs whose agents have moved to another task, fixing cases where two sidebar sessions had three tabs. Pending joins are checked against current ownership so an interrupted later move cannot restore an obsolete tab. New layouts record agent ownership to preserve intentional cross-task views. Cross-task tabs in older layouts without ownership records need to be reopened once.
- Added an Apple Silicon remote preview package with the app, matching CLI and bilingual setup instructions; no Rust/Node compilation is needed on the user Mac.
- Local Shifu connects to another Mac through your own HTTPS relay. After remote approval, enter existing agent terminals or create a Claude/Codex session in a registered remote project. Connections can be saved and devices revoked; reconnection does not automatically replay input.
- Includes the candidate's closed-lid preferences and sidebar quotas; the meal-ordering bridge has been removed. Not notarized. Target-machine first launch, real agents, cross-country networking, native Chinese input and iPhone still need testing. [Setup and limits](REMOTE-RELAY.en.md).

### November 13, 2026

- Updated the app icon to the corner-joint mark: two interlocking geometric pieces in white on a dark background.

### November 12, 2026

- Edit and save Markdown in file tabs, with Cmd+S, draft recovery and conflict detection. Drag a tab into the outer third of the content area to split it.
- Closing a tab always offers to hide it while the agent keeps running, or end it. Agents viewed across tasks keep their original task membership.
- The cup menu adds timers and closed-lid operation, with low-battery and thermal pauses and recovery after an unexpected app exit. See the [power guide](STEAMCUP.md).
- An optional local service lets a phone view and reply to existing desktop terminals, with Chinese input and reconnection. Remote access requires a private network you configure. See [setup](REMOTE.md).
- `shifu computer guide` documents the shared computer-use entry point. Window selection, native file pickers and focus checks are improved; action readbacks can return changed controls while preserving receipts when the outcome is uncertain. Background operation can still trigger an application's own dialogs.
- Create and restore Antigravity conversations with locally available models and thinking levels. A meal-ordering button can connect to an existing local service; that service and its accounts are not included.

### November 11, 2026

- Computer use now recognizes standalone input windows, activates the selected window by ID or index, and restores the previous focused window after a temporary foreground read. An opt-in Finder/Safari task check is included.

- Antigravity quota monitoring uses the signed-in `agy` CLI (1.1.11+) and its [official `/usage` report](https://antigravity.google/docs/cli/commands/usage/). The footer shows used weekly and five-hour quotas for Gemini and Claude/GPT separately, with reset times and a five-minute refresh interval. Failed refreshes are marked stale or unavailable; Shifu does not store account credentials.

- Independent macOS computer use through `shifu computer`: inspect windows, capture screenshots, click, type, scroll and drag without running Orca. Grant permissions to Shifu's own helper; see the [setup and command guide](COMPUTER-USE.md).
- Filter controls by text, limit output, wait for visible text, and check file path conditions. An explicit foreground fallback briefly reads the window and restores the previous app. A changed control value does not prove that a file operation completed.
- Choose an agent for the project manager, inspect conversation status and relay messages through local commands. Search active conversations, hide the sidebar and split panes vertically.
- A bottom bar shows usage and process memory, with controls to keep the system or display awake. File tabs support HTML and audio/video previews.

Finder standalone rename editors can now be observed, edited and committed; verify the resulting file paths. Background browser reads may return only toolbars; wait for specific body text. There is no browser DOM channel yet.
