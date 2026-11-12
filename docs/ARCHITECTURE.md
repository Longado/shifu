# Shifu 架构

一句话：**tmux 管进程，hooks 报状态，文件当接口，界面只是挂上去看。**

```
 Claude Code / Codex / Kimi / CodeBuddy        （每个 agent 一个 tmux 会话，专用 socket）
        │  hooks：SessionStart、UserPromptSubmit、PermissionRequest、
        │         PreToolUse、PostToolUse、Stop、SessionEnd …
        ▼
 shifu hook <kind> <事件>                      （Rust 命令行，永远 exit 0、不输出）
        │  按转换规则改状态，原子写入
        ▼
 ~/.shifu/app/agents/<name>.json               （一条 agent 记录：kind、目录、状态、起始时间、最后一句…）
        │  每 500 毫秒读一次，再对照 tmux 里真实存在的窗格
        ▼
 Shifu 桌面版后端（Tauri / Rust）── 列表变了才推给前端
        │
        ▼
 前端（TypeScript）：侧栏、dockview 分屏标签、xterm.js 终端
        │  每个终端标签 = 一个 PTY 里跑的 `tmux attach -t <name>`
        ▼
 你看到并操作的，就是 agent 自己原本的终端界面
```

## 为什么这样分

- **进程归 tmux。** 桌面版崩了、重启了、被关了，agent 都不受影响；再打开时重新 attach。命令行版 `shifu` 和桌面版各用各的 socket（`shifu` / `shifu-app`）和目录（`~/.shifu` / `~/.shifu/app`），互不干扰，也不碰你自己的 tmux。
- **状态来自 hooks，不来自屏幕。** 各家 agent 的界面经常变，靠识别屏幕文字判断状态很脆。hooks 是它们正式提供的事件，含义稳定。
- **文件当接口。** hook 进程是 agent 每次事件时临时拉起的，写完一个 JSON 就退出，不需要常驻服务、不需要端口。界面读文件，所以命令行版、桌面版、调试脚本看到的是同一份事实。
- **hooks 只挂在这次启动上。** Claude Code 和 CodeBuddy 通过 `--settings <文件>`、Codex 通过一组 `-c hooks.…=…` 参数接上，不改用户的全局设置。Kimi 只认自己的配置文件，是唯一需要写入用户配置的（会先备份）。

## 状态怎么变

转换规则在 `src/status.rs` 的 `next_status`，有单元测试覆盖。要点：

| 事件 | 变成 |
|---|---|
| SessionStart | 空闲 |
| UserPromptSubmit | 运行中 |
| PermissionRequest | 等你批准 |
| PreToolUse 且工具是 AskUserQuestion | 在问你 |
| 批准后的 PostToolUse | 运行中 |
| 在等你/空闲时又出现 PreToolUse、PostToolUse | 运行中（被后台任务或消息叫醒，自己接着干活） |
| Stop、StopFailure、Interrupt | 在问你（这一轮结束，等你回复） |
| SessionEnd | 已退出 |

子 agent 的 hook 事件在写入前就被丢掉，免得主 agent 的状态被它们来回改。tmux 里窗格没了但记录还在的 agent，界面按「已退出」显示，可以一键恢复原来的会话（`--resume`）。

## 代码在哪

| 目录 | 内容 |
|---|---|
| `src/` | 命令行 `shifu`：tmux 封装（`tmux.rs`）、agent 记录（`agent.rs`）、hook 入口（`hook.rs`）、状态规则（`status.rs`）、各家启动参数与 hooks 拼装（`launch.rs`）、tmux 里的侧栏（`sidebar.rs`） |
| `app/src-tauri/src/` | 桌面版后端：读 agent 列表并推送（`agents.rs`）、开/结束/恢复会话（`session.rs`）、PTY（`pty.rs`）、任务与归属（`tasks.rs`）、每个任务的布局（`layout.rs`）、额度（`limits.rs`、`statusline.rs`、`antigravity.rs`）、系统通知（`notify.rs`）、文件读写和拖入（`files.rs`）、保持唤醒与合盖恢复（`power.rs`、`lid.rs`） |
| `app/src/` | 前端：侧栏（`sidebar.ts`）、分屏与标签（`workspace.ts`、`taskdock.ts`、`tab.ts`）、终端（`terminal.ts`、`imeinput.ts`、`links.ts`）、额度栏（`limits.ts`、`limitsbar.ts`）、新建任务（`newpopover.ts`） |
| `tests/`、`scripts/smoke.sh` | hook 的集成测试；用专用 socket、临时目录和假 agent 的冒烟测试 |
| `remote/`、`app/remote/` | 可选手机入口：仅监听本机的 HTTP/WebSocket 服务、现有终端连接和手机网页；安装与私有网络说明见 [REMOTE.md](REMOTE.md) |
| `native/computer-use-macos/`、`src/computer/` | 独立 macOS 电脑操作辅助程序、CLI、观察与使用指南；见 [COMPUTER-USE.md](COMPUTER-USE.md) |

## 任务、归属和布局

- **任务**是一组 agent，存在 `tasks.json`。一个 agent 只属于一个任务。
- **布局**是每个任务自己的 dockview 分屏，存在 `layouts/<任务 id>.json`，切任务时保存旧的、恢复新的。
- 两者分开存：布局决定「屏幕上怎么摆」，归属决定「左边栏挂在哪个任务下」。把 agent 拖到另一个任务上会改归属，它的标签也跟着离开当前任务、出现在目标任务里。

## 额度从哪来

| 来源 | 怎么取 |
|---|---|
| Claude | Claude 只在交给 statusLine 命令的 JSON 里给出 5 小时和 7 天用量。Shifu 把 statusLine 指向一个小脚本：先存下这份 JSON，再原样交给用户自己的 statusLine，所以 Claude 状态栏的显示不变 |
| Codex | 读本机会话记录里最新的 `rate_limits` |
| CodeBuddy | 读本机会话记录里每轮的积分和免费额度 |
| Kimi | 读取 Kimi Code 本机仍有效的登录令牌查询 `/usages`，不另存令牌；登录过期时保留上次用量，等 Kimi 更新登录后再刷新 |
| Antigravity | 通过当前登录的 `agy` CLI 读取 `/usage` JSON，按模型组显示周额度、5小时额度和重置时间 |
