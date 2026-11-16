# Change log / 改动记录

每次修改保留提交记录；影响使用的变化与代码一起更新本文件。推送后核对远端提交，尚未发布的下载包单独说明。

Keep a commit record for each change, and update this file with user-visible changes in the same commit. Verify the remote commit after pushing and state separately when download packages are unchanged.

## Unreleased / 尚未发布

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
