# 工作区与会话接续 / Workspaces and session recovery

`shifu workspace` uses ordinary Git worktrees and a small task note stored in each worktree's Git metadata. It does not copy conversation history or automatically merge, delete, or send messages to agents. Task notes are limited to 16 KiB; artifact references must stay inside the worktree and point to existing files.

```sh
shifu workspace list --json
shifu workspace init --goal 'Fix the selected issue' --scope src/example.rs
shifu workspace brief --json
shifu workspace create example ../example-wt --base main \
  --goal 'Fix the selected issue' --scope src/example.rs
shifu workspace handoff --done 'Implementation ready' \
  --next 'Review the diff' --artifact src/example.rs
shifu workspace plan --base main --json
```

Use `init` for an existing checkout. Use `create` only when a separate worktree is needed; source directories cannot be placed under build-output directories. A single owner updates a task note at a time. `plan` is a read-only inventory, not authorization to remove worktrees or running sessions.

To restore a desktop agent whose process has exited:

```sh
shifu status
shifu resume exact-agent-name
```

The command uses the saved conversation ID and model. A live pane is left alone. When its tmux server must be recreated, start it through the desktop app first so macOS permissions belong to the intended app.

中文：`workspace list/brief` 读取工作区与当前接续说明；`init` 在现有工作区登记任务，`create` 从指定 Git 版本建立独立工作区，`handoff` 更新已做/下一步及产物引用。`plan` 只列出可供核对的工作区，不删除内容。任务说明限16 KiB，不复制会话历史，不自动合并或给其他会话传话。退出的桌面会话可用 `shifu resume 精确会话名` 恢复；已有运行进程会保留。
