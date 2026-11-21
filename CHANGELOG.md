# Change log / 改动记录

每次修改保留提交记录；影响使用的变化与代码一起更新本文件。推送后核对远端提交，尚未发布的下载包单独说明。

Keep a commit record for each change, and update this file with user-visible changes in the same commit. Verify the remote commit after pushing and state separately when download packages are unchanged.

## Source update — 2026-11-18 (not in preview downloads)

- 多个 Agent 可以各开工作区同时改：范围撞车会提示，更新记录各写各的，做完标记可合并，由合并 Agent 一条命令合入主分支；冲突或检查不过的列出来不合，别人未提交的改动不会被覆盖。
- Several agents can work in parallel: overlapping scopes are flagged, notes go in per-branch files, and one merge step lands every ready branch, listing conflicts and failed checks instead of merging them.

- 进度页改成按任务分：每个任务一张卡，写着终点（做完是什么样）、重点（现在最该推的一件）和清单进度；可以在卡上自己写，也可以让会话起草，点「告诉这个任务的会话」后它们空下来就会收到；之后新加进任务的会话会自动被告知。
- The progress page is now one card per task with its end goal, current focus and checklist; write it on the card or have a conversation draft it, tell the task's conversations with one click (delivered when they are free), and conversations added later are told on their own.
- 同一个任务里的会话可以上下拖动调整顺序，顺序会记住；新开的会话排在后面。
- Drag a task's conversations up or down to reorder them; the order is kept, and new ones go last.
- 点多会话任务那一行就能展开或收起它的会话：点当前任务是收起／展开，点别的任务会切过去并展开。
- Click a multi-conversation task's row to fold or unfold its conversations; clicking another task switches to it unfolded.
- 多会话任务右侧的箭头可以展开或收起会话列表，无需切换当前任务。
- Expand or collapse a task's conversations with the arrow without switching tasks.

- 拖动任务标题或左侧六点手柄可在项目内调整顺序，松手后保存。移动会话请拖终端标签或展开后的会话行。
- Drag a task heading or its grip to reorder tasks within a project. Move an agent by dragging its terminal tab or its expanded agent row.

- 侧栏项目文件夹可向上或向下拖动，插入线显示落点，松手保存顺序。
- Drag project folders up or down in the sidebar, with an insertion line and a saved order.

- 浅色主题里的 Codex 输入区显示白底，输入文字保持深色。
- Show the Codex composer on white in light themes, with dark input text.

- 展开底部「用量」即可查看 Codex credits 余额，额度未用满时也会显示。
- Expand Usage to see the Codex credits balance whenever Codex reports it.

- 浅色主题下，Codex 输入区保留深色背景时，输入文字和提示仍清晰可读。
- In the light theme, Codex input and its placeholder stay readable when the composer keeps a dark background.

- 默认使用墨黑主题，侧栏、弹窗和悬停行的辅助文字一起提高可读性；已保存的主题选择保留，未保存选择的用户采用新默认。
- Use the near-black default and improve muted text on sidebar, popover and hover surfaces. Saved theme choices are preserved; users without a saved choice receive the new default. Not installed or included in preview downloads.

## Source update — 2026-11-17 (not in preview downloads)

- 新建任务和标签页的「＋」菜单可选择普通终端，在任务目录中打开默认登录 shell，支持分屏和退出后重新打开；不需要模型。
- Open a plain terminal from the new-task or tab “+” menu, using the default login shell in the task folder, with splits and restart after exit. No model is required. Not installed or included in preview downloads.

## Source update — 2026-11-16 (not in preview downloads)

- 恢复已有 Codex 对话时显示空闲，不再因缺少启动回调一直转圈；已打开、仍保留旧“启动中”记录的会话也会正确显示，无需重启会话。新的输入或工具活动会切回运行中，结束后保留正常完成状态。此项尚未安装到现有 App。
- Restored Codex conversations start idle instead of spinning indefinitely when no startup callback arrives. Already open conversations with legacy startup records receive the same display correction without restarting their panes. Prompt/tool activity still marks them running, and completed turns keep their normal completion state. Existing app installations are unchanged.

## Unreleased / 尚未发布

- 一个 Task 还有未回答的追问时，收起后仍显示问号气泡，不再被其他已完成或停着的会话盖住；运行、启动和待批准状态仍优先。此项尚未安装，现有下载包未更新。
- A task with a pending follow-up keeps its question-bubble indicator when collapsed, even when another member has completed or been read. Running, starting and approval states retain priority. Not installed or included in current downloads.

- 已识别的 AI follow-up 追问改用蓝色问号气泡图标；常规侧栏不再展开追问内容或在图标旁重复状态文字，悬停显示“待回复”。打开会话查看后，图标仍保留到会话状态改变；关注计数与通知规则不变。此项尚未安装，现有下载包未更新。
- Recognized AI follow-up questions use a blue question-bubble icon. Regular sidebar rows omit the question preview and adjacent status label; hover shows “待回复” (awaiting reply). Reading a conversation retains its question icon until its status changes, without changing attention counts or notifications. Not installed or included in current downloads.

- 会话持续启动一分钟仍未收到就绪状态时，显示静态的“启动待检查”，悬停提示打开原终端查看；Task、常驻入口与会话标签使用同样提示。实际运行、批准和完成状态保持原样。此项尚未安装，现有下载包未更新。
- Sessions without a readiness update after one minute show a static “启动待检查” (check startup) indicator with a prompt to inspect the terminal. Task, pinned-role and tab indicators agree; running, approval and completion states retain their existing behavior. Not installed or included in current downloads.

- 进度页可读取工作区的可选待办数据库，按项目和最近截止日显示未完成工作；卡片默认收起，刷新时保留展开状态。现有下载包尚未包含此项。
- The progress page can read an optional workspace task database, group open work by project and nearest deadline, and preserve expanded cards across refreshes. Existing download packages are unchanged.
- 展开、收起或刷新进度卡片时保留标题按钮的键盘焦点；不会抢走其他输入框的焦点。
- Keep keyboard focus on the same progress heading across toggles and refreshes without taking focus from other controls. Not installed yet.

- 后台托管在人工接手、停止或到期时保留已经发生的判断耗时，未收到的 token 用量仍显示为空；现有下载包尚未包含此修改。
- Preserve judgment time spent before human takeover, stop or expiry; unreported token usage remains null. Existing download packages do not include this change.

- 安装指南先校验解压内容，再移动 App，避免因文件被移走而导致校验失败。
- Installation guides now verify extracted contents before moving the app, so verification can find every packaged file.

- 首页明确 Claude Code/Codex 的 macOS 工作台用途，中英安装默认使用同包 App/CLI；源码构建保留给开发者，并补操作演示与分享图片。
- Clarified the macOS Claude Code/Codex workbench introduction and made packaged app/CLI installation the default in both languages; source builds remain available for developers, with an interaction demo and social preview image.

- 成品安装补齐当前终端 PATH 设置和新终端的完整路径用法；无需先安装 Rust。
- Packaged installation now sets the current shell PATH and explains the full-path command for new terminals, without requiring Rust.

## remote-v0.3.5-preview.1 — 2026-11-16

- StepFun 可选独立模型会话，使用 Step Plan 订阅并准确续接；另需 Node/Pi 和自己的密钥库或环境注入。
- 后台托管可向固定原总控报告受阻或完成，保留人工接手和未知发送保护；可读范围与可写范围分开，媒体只报告元数据。
- 开发缓存采用8 GiB/3天默认值，失败构建也回收其他闲置缓存。非正式App输出到 `.noindex` 目录。
- Apple Silicon 预发布配套 App/CLI、Computer Use 组件及自动驾驶脚本；安装不会启动托管，运行凭证和会话不随包分发。

- Added StepFun Step Plan sessions and exact resume, with separate Node/Pi and key setup.
- Bounded follow-up can report completion/blockers to the original controller, preserving human takeover and uncertain-send stops; read-only artifacts and media metadata are supported.
- Managed developer builds use 8 GiB/3-day defaults, reclaim other inactive caches after failures and keep development app bundles under `.noindex` directories.
- The Apple Silicon preview bundles matching app/CLI, Computer Use helper and the autopilot script. Installation does not start the controller; credentials and sessions are excluded.

## Source update — earlier on 2026-11-16

- 远程页按远端真实 Task ID 分组；同名任务保持独立，自动任务使用会话话题显示标题。可搜索任务与会话，旧远端继续使用未分组列表。
- 普通停下的远程会话显示「等待输入」，待批准状态单独提示。
- 列表自动刷新保留 Task 标题或会话按钮的键盘焦点；搜索中手动收起的任务保持收起，清空搜索恢复普通收起状态。

- Group remote sessions by their actual task ID, keeping tasks with identical names separate and using conversation topics for automatic task titles. Search tasks and sessions; older gateways keep the ungrouped list.
- Show stopped remote sessions as waiting for input, with approval requests displayed separately.
- Preserve task/session keyboard focus across list refreshes. Keep manually collapsed search results closed and restore normal collapse state when search is cleared.

Task grouping requires an updated desktop controller and remote gateway. This entry records source changes; existing preview download packages are unchanged.
