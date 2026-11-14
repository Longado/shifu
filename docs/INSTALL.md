# 安装与使用

[回到 Shifu](../README.md)　[English](INSTALL.en.md)

Shifu 提供 [Apple Silicon 预发布包](https://github.com/Longado/shifu/releases/tag/remote-v0.3.4-preview.1)，内含 App、匹配的 Shifu CLI 和安装说明；使用包时无需 Rust/Node，tmux、Agent 和账号登录需自行准备。按[远程安装与配对](REMOTE-RELAY.md)完成首次设置。

以下保留源码构建方式，需要 Homebrew tmux、Rust，以及 Node 20.19 或 22.12 以上版本。

Shifu 默认用免确认模式启动 Claude、Codex、CodeBuddy 和 Kimi（如 `--dangerously-skip-permissions`、`--dangerously-bypass-approvals-and-sandbox`、`--yolo`）。它们执行文件修改和命令时不会逐项请求批准；如需调整，先检查 `src/launch.rs`。

应用使用临时签名，尚未公证。首次打开如果被 macOS 拦住，在 Finder 里右键选择“打开”。

## 快速开始

### 方式 A：让你的 AI 帮你装

把下面这段贴进 Claude Code 或 Codex：

```text
帮我装 Shifu（https://github.com/Longado/shifu）。先读它的 README 和 docs/ARCHITECTURE.md。
1. 检查 macOS 上有没有 Homebrew 的 tmux、Rust（cargo）、Node 20.19 或 22.12 以上，缺的告诉我怎么装。
2. 克隆仓库，在根目录 cargo install --path . 装 shifu 命令行（桌面版的 hooks 调用的就是 ~/.cargo/bin/shifu）。
3. 进 app/，npm install，再 npm run tauri build -- --debug。
4. 告诉我 Shifu.app 生成在哪，提醒我：它没有签名公证，首次打开若被拦，确认来源后按「隐私与安全性」中的对应提示处理；它默认用免确认模式启动 agent。
5. 第一次在 Shifu 里开 Codex 时，Codex 会问 hooks 要不要信任，提醒我选 Trust all。
```

### 方式 B：自己装

```bash
brew install tmux
git clone https://github.com/Longado/shifu && cd shifu
cargo install --path .                      # shifu 命令行，装到 ~/.cargo/bin
cd app && npm install
npm run tauri build -- --debug              # 生成 src-tauri/target/debug/bundle/macos/Shifu.app
open src-tauri/target/debug/bundle/macos/Shifu.app
```

- 应用是临时签名（ad-hoc）的，没有公证。首次打开若被拦，确认来源后按「系统设置 → 隐私与安全性」中的对应提示处理。
- 第一次在 Shifu 里开 Codex，它会显示「Hooks need review」，选 **Trust all and continue**。只要 `shifu` 的安装路径不变，以后不再问。
- 只想用命令行版：`shifu` 打开工作台，`shifu new claude ~/proj` 开一个 agent，`shifu ls` 列出全部，`shifu jump` 跳到等你最久的那个。

## 快捷键

| 键 | 作用 |
|---|---|
| Cmd+N | 新建任务 |
| Cmd+T | 在当前任务里加一个 agent |
| Cmd+W | 关闭当前面板（agent 不停） |
| Cmd+J | 跳到等你最久的 agent |
| Cmd+, | 外观 |
| 双击标签 | 给这个 agent 改名，留空就用回话题名 |
| 点标签上的 × | 选择「收起，继续运行」或「结束」；运行中结束另有确认 |

## 隐私

- Shifu 不提供公共托管账号，不收集遥测。状态、任务、布局保存在本机 `~/.shifu/app`（命令行版在 `~/.shifu`）。本机手机入口监听 `127.0.0.1`；新远程模块可连接本人配置的 HTTPS 中继，端到端通道传输终端数据与操作请求，中继网页也应来自自己信任的服务器。[连接说明](REMOTE-RELAY.md)。
- Claude Code、Codex、CodeBuddy 的 hooks 通过启动参数挂在 Shifu 开的那个进程上，**不改**你的 `~/.claude/settings.json`、`~/.codex/config.toml`。
- **Kimi 例外**：Kimi 只认自己的配置文件，Shifu 会把 hooks 写进 `~/.kimi-code/config.toml`，你自己的 hook 原样保留，第一次写之前备份成 `config.toml.before-shifu`。在 Shifu 外面开的 Kimi 也会调用这些 hook，但它们发现不是 Shifu 开的就什么都不做。
- 额度显示：Claude 的额度从它交给 statusLine 的数据里取（你自己的 statusLine 照常显示）；Codex 和 CodeBuddy 的额度读本机会话记录；Kimi 的额度复用 Kimi Code 本机有效登录令牌查询，不另存该令牌；Antigravity 通过当前登录的 `agy` 读取用量。
- Shifu 用自己的 tmux socket（`shifu-app`，命令行版是 `shifu`），不碰你原来的 tmux。

## 已知问题

- 只在 macOS（Apple Silicon）上用过。Linux 和 Windows 没试过，通知、拖放这些用的是 macOS 的接口。
- 某个项目目录里的 `git` 命令卡住时（比如在等 macOS 的「访问桌面文件夹」授权），整个窗口会跟着卡住，直到 git 返回。原因已经找到：读分支名时拿着锁跑 git；还没修。
- 终端里折成三行以上的长路径，点后面几行打不开。原因没查清。
- Kimi Code 登录过期时不能刷新额度；会保留上次结果，运行 Kimi 更新登录后再刷新。
- 手机外网连接需要另行配置私有网络。
- 另外接了两个可选的本机数据源：进度页读各项目的 `GOALS.md` 和 WorkBuddy 的本地任务；新回复通知前会读一个叫 Jev 的本机判断日志。没有这些文件时自动跳过。

## 一起做

**多个 agent 同时干活时，人怎么才能只在真正需要自己的时候被叫到？**

| 你喜欢…… | 可以贡献 |
|---|---|
| Rust、并发 | 把读分支名的 git 挪出锁外、加超时，修好上面那个整窗卡住的问题 |
| 终端、xterm.js | 修长路径折行后点不开；终端里的搜索 |
| 接新的 agent | Gemini CLI、Cursor CLI 等：写 hooks 适配，把状态接进侧栏 |
| 跨平台 | Linux 上的通知、拖放和打包 |
| 交互设计 | 「在等你」怎么排、什么时候该发通知、什么时候不该打扰 |

欢迎提 issue 或 pull request。

## 开发

```bash
cargo test && cargo clippy -- -D warnings           # 命令行和 hooks
cargo build --release && bash scripts/smoke.sh       # 冒烟：专用 socket、临时目录、假 agent
cd app && npm test && npm run build                  # 前端单测和类型检查
cd app/src-tauri && cargo test                       # 桌面版后端
cd app && npm run tauri dev                          # 开发模式
```

开发时设 `SHIFU_APP_HOME`、`SHIFU_APP_SOCKET`，测试数据就不会和正在用的混在一起。
