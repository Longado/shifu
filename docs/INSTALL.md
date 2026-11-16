# 安装与使用

[回到 Shifu](../README.md)　[English](INSTALL.en.md)

Shifu 提供 [Apple Silicon 预发布包](https://github.com/Longado/shifu/releases/tag/remote-v0.3.5-preview.1)，内含 App、匹配的 Shifu CLI 和安装说明；使用普通会话无需 Rust，StepFun 另需 Node/Pi；tmux、Agent 和账号登录需自行准备。按[远程安装与配对](REMOTE-RELAY.md)完成首次设置。

普通会话优先使用下载包；源码构建方式收在下方开发者说明。

Shifu 默认用免确认模式启动 Claude、Codex、CodeBuddy 和 Kimi（如 `--dangerously-skip-permissions`、`--dangerously-bypass-approvals-and-sandbox`、`--yolo`）。它们执行文件修改和命令时不会逐项请求批准；如需调整，先检查 `src/launch.rs`。

应用使用临时签名，尚未公证。首次打开若被 macOS 拦截，确认来源后按「系统设置 → 隐私与安全性」中的对应提示处理。

## 快速开始

[下载 Apple Silicon 预发布包](https://github.com/Longado/shifu/releases/tag/remote-v0.3.5-preview.1)，内含 App、匹配 CLI 和安装说明。普通会话无需 Rust；另备 tmux 和已登录的 Agent 即可。StepFun 需[单独设置 Node/Pi 和订阅密钥](STEPFUN.md)。

### 方式 A：让你的 AI 帮你装

把下面这段贴进 Claude Code 或 Codex：

```text
帮我安装 Shifu（https://github.com/Longado/shifu）。先读 README 和 docs/INSTALL.md。
1. 确认当前 Mac 使用 Apple Silicon，检查 tmux 和我要使用的 Agent 是否已安装、登录；缺什么先告诉我。
2. 下载 README 指向的同一 Release 中的 ZIP 和 SHA256SUMS，核对下载文件及解压后的文件校验。默认使用现成 App 和 CLI，无需安装 Rust 或从源码编译。
3. 按包内说明，将 Shifu.app 放到 ~/Applications，将同包 bin/shifu 装到 ~/.cargo/bin/shifu。将 ~/.cargo/bin 加入当前终端 PATH，验证 shifu --help，并说明新终端可用完整路径运行。已有 Shifu 时先说明升级需要短暂退出，保留旧 App、CLI 和会话；不要覆盖运行中的 App。
4. 提醒我它是临时签名、未公证，且默认用免确认模式启动 Agent；系统安全提示由我处理。StepFun、电脑操作、远程中继和后台托管仅在我要求时另行配置。
5. 打开后带我选一个项目、创建第一条 Agent 对话；首次启动 Codex 时，让我检查它提示的 hooks 后决定是否信任。
```

### 方式 B：自己装

1. 下载同一 Release 的 ZIP 和 `SHA256SUMS`，先在下载目录执行 `shasum -a 256 -c SHA256SUMS`，再解压。
2. 将 `Shifu.app` 放到 `~/Applications`。已有 Shifu 时，先退出并备份旧 App 与 CLI；App 和 CLI 使用同一包的版本。在解压后的包目录执行：

   ```bash
   shasum -a 256 -c SHA256SUMS
   mkdir -p "$HOME/.cargo/bin"
   install -m 755 bin/shifu "$HOME/.cargo/bin/shifu"
   export PATH="$HOME/.cargo/bin:$PATH"
   brew install tmux   # 已有 tmux 时跳过
   ```

3. 打开 Shifu，选择项目和已登录的 Agent，创建第一条对话。完整说明见[安装指南](INSTALL.md)；连接另一台 Mac 时再看[远程安装与配对](REMOTE-RELAY.md)。

- 应用采用临时签名，尚未公证；首次打开若被拦，确认来源后按「系统设置 → 隐私与安全性」中的对应提示处理。
- 第一次启动 Codex 会提示检查 hooks，请阅读后决定是否信任。CLI 安装路径保持不变时，无需每次重复设置。
- PATH 设置只对当前终端生效。新终端尚未配置该路径时，可运行 `"$HOME/.cargo/bin/shifu"`，或自行将该目录加入 shell 配置。
- 只想用命令行：`shifu` 打开工作台，`shifu new claude ~/proj` 开一个 Agent，`shifu ls` 列出会话，`shifu jump` 跳到等待处理的会话。

<details>
<summary>从源码构建（开发者）</summary>

需要 Rust，以及 Node 20.19 或 22.12 以上版本。

```bash
brew install tmux
git clone https://github.com/Longado/shifu && cd shifu
cargo install --path .
cd app && npm install
npm run build:desktop -- --debug
# 按构建日志中的受管 .noindex App 路径打开
```

</details>

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


StepFun 需额外配置 Node/Pi 和订阅密钥，见[StepFun设置](STEPFUN.md)。App与配套CLI应成对更新。后台托管脚本见[AUTOPILOT.md](AUTOPILOT.md)，不会因安装自动启动。
