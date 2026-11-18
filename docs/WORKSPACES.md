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

## Parallel agents and the merge step

Several agents can change the same repository at once without overwriting each other:

```sh
# Each change starts in its own worktree. A scope that overlaps an unfinished workspace prints a warning.
shifu workspace create search-fix ../search-fix-wt --base main --goal 'Fix search jumps' --scope app/src/search.ts
# User-visible notes go in this branch's own file instead of CHANGELOG.md, so branches never conflict there.
echo '- Search results jump to the right line.' > changelog.d/shifu-search-fix.md
# After committing and self-checking:
shifu workspace handoff --done 'Implemented and tested' --next 'Merge' --ready
# One merging agent lands every ready branch:
shifu workspace merge --check 'cargo test --release' --check 'cd app && npm test'
```

`merge` works in a fixed integration worktree beside the main checkout. It merges each ready branch with `--no-ff`, moves the `changelog.d/` notes under today's heading in CHANGELOG.md (following the style of the newest dated heading), and runs the checks. Branches that conflict, fail a check, or gained commits after `--ready` are skipped and listed. The base branch then only fast-forwards; if someone's uncommitted edits would be overwritten, Git refuses and the result stays in the integration worktree. `--branch <name>` merges an older branch that has no task note. Nothing is deleted.

Private agent rules that cannot live in a public repository can be registered with `git config shifu.rules /path/to/rules.md`; `create` then writes untracked `AGENTS.md` and `CLAUDE.md` pointers into each new worktree and keeps them out of commits.

中文：多个 Agent 各用 `create` 开自己的工作区（范围与未完成的工作区重叠会提示），更新记录写在本分支的 `changelog.d/<分支>.md`，做完自测后 `handoff --ready`。负责合并的 Agent 运行 `merge --check …`：在固定的集成工作区逐个合入、按 CHANGELOG 现有日期标题样式汇总更新记录、跑检查；冲突、检查失败或标记后又提交的分支跳过并列出；主分支只快进，会覆盖他人未提交改动时由 Git 拒绝。私有约定用 `git config shifu.rules <文件>` 登记，新工作区自动放不提交的指向文件。
