# Change log / 改动记录

每次修改保留提交记录；影响使用的变化与代码一起更新本文件。推送后核对远端提交，尚未发布的下载包单独说明。

Keep a commit record for each change, and update this file with user-visible changes in the same commit. Verify the remote commit after pushing and state separately when download packages are unchanged.

## Unreleased — 2026-11-16

- 远程页按远端真实 Task ID 分组；同名任务保持独立，自动任务使用会话话题显示标题。可搜索任务与会话，旧远端继续使用未分组列表。
- 普通停下的远程会话显示「等待输入」，待批准状态单独提示。
- 列表自动刷新保留 Task 标题或会话按钮的键盘焦点；搜索中手动收起的任务保持收起，清空搜索恢复普通收起状态。

- Group remote sessions by their actual task ID, keeping tasks with identical names separate and using conversation topics for automatic task titles. Search tasks and sessions; older gateways keep the ungrouped list.
- Show stopped remote sessions as waiting for input, with approval requests displayed separately.
- Preserve task/session keyboard focus across list refreshes. Keep manually collapsed search results closed and restore normal collapse state when search is cleared.

Task grouping requires an updated desktop controller and remote gateway. This entry records source changes; existing preview download packages are unchanged.
