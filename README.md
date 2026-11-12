<p align="center"><img src="app/src-tauri/icons/128x128@2x.png" width="112" alt="Shifu"></p>

<h1 align="center">Shifu</h1>
<p align="center"><strong>同时运行多个编程 Agent，一眼找到需要你接手的会话。</strong></p>
<p align="center">给同时使用多种编程 Agent 的人用的 macOS 终端工作台。</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/tested%20on-macOS-black" alt="tested on macOS">
  <img src="https://img.shields.io/badge/Rust%20%2B%20Tauri%202-orange" alt="Rust and Tauri 2">
  <img src="https://img.shields.io/badge/status-Alpha-orange" alt="status: Alpha">
</p>

<p align="center">
  <a href="README.en.md">English</a> ·
  <a href="#快速开始">快速开始</a> ·
  <a href="#可以试试这些">可以试试</a> ·
  <a href="#一起做">一起做</a> ·
  <a href="docs/ARCHITECTURE.md">架构</a> ·
  <a href="docs/UPDATES.md">更新记录</a>
</p>

Agent 多起来以后，不用逐个翻终端：Shifu 把需要你接手的会话排在一起。

**各家终端原样保留** · **关掉窗口继续运行** · **任务与布局保存在本机**

## 是什么

Shifu 把 Claude Code、Codex、Kimi、CodeBuddy 和 Antigravity 放进同一个 macOS 窗口。左侧按项目和任务组织会话，显示谁在运行、谁在等批准、谁在等你回复；右侧是各家 Agent 自己的终端，可以拖动标签、左右或上下分屏。

会话由独立的 tmux 服务管理，桌面版只负责显示和操作。关掉或重开窗口不会结束 Agent；回来时恢复任务与分屏布局。命令行也能单独使用，设计见[架构说明](docs/ARCHITECTURE.md)。

> **早期版本。** 目前主要在 Apple Silicon Mac 上使用，需要自己编译，应用尚未公证。Shifu 默认以免确认模式启动 Claude、Codex、CodeBuddy 和 Kimi，它们修改文件、执行命令时不会逐项询问；安装前请读[启动说明](#快速开始)和[已知问题](#已知问题)。

<p align="center"><img src="docs/images/shifu-workspace.png" width="960" alt="Shifu 当前界面的模拟演示：侧栏显示会话状态，两个 Agent 的终端左右并排。"></p>
<p align="center"><sub>这是模拟会话演示：界面来自当前版本，终端内容、会话状态与额度为演示数据。</sub></p>

## 它怎么工作

| 你…… | 它会…… |
|---|---|
| 新建任务，选 Agent、目录和模型 | 在专用 tmux 服务里启动会话，接收对应 Agent 的状态 |
| Agent 停下来等待批准或回复 | 在侧栏标记状态，放到顶部「在等你」 |
| 按 ⌘J，或点击一条等待中的会话 | 跳到对应任务，激活原来的终端 |
| 拖动标签，或右键选择分屏方向 | 将终端左右或上下并排，保留各自的交互方式 |
| 点开终端里的文件路径 | 在对话旁边打开文件；Markdown 可以直接编辑、保存 |
| 关掉 Shifu 再打开 | Agent 继续运行，任务与分屏布局恢复 |

## 可以试试这些

| 试试 | 会看到 |
|---|---|
| 同一任务开两个 Agent，右键其中一个标签 →「移到右边」 | 两个 Agent 各自的终端界面左右并排 |
| 点击顶部「在等你」里的另一条会话 | 切到它所在的任务，并激活对应终端 |
| 点左上角「+」，再切换 Agent | 在同一张表单选择项目、Agent、模型与思考档位；选项以本机实际可用内容为准 |

## 快速开始

### 方式 A：让你的 AI 帮你装

把下面这段贴进 Claude Code 或 Codex：

```text
帮我装 Shifu（https://github.com/Longado/shifu）。先读它的 README 和 docs/ARCHITECTURE.md。
1. 检查 macOS 上有没有 Homebrew 的 tmux、Rust（cargo）、Node 20.19 或 22.12 以上，缺的告诉我怎么装。
2. 克隆仓库，在根目录 cargo install --path . 装 shifu 命令行（桌面版的 hooks 调用的就是 ~/.cargo/bin/shifu）。
3. 进 app/，npm install，再 npm run tauri build -- --debug。
4. 告诉我 Shifu.app 生成在哪，提醒我：它没有签名公证，第一次要右键「打开」；它默认用免确认模式启动 agent。
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

- 应用是临时签名（ad-hoc）的，没有公证。第一次打开如果被拦，在 Finder 里右键「打开」。
- 第一次在 Shifu 里开 Codex，它会显示「Hooks need review」，选 **Trust all and continue**。只要 `shifu` 的安装路径不变，以后不再问。
- 只想用命令行版：`shifu` 打开工作台，`shifu new claude ~/proj` 开一个 agent，`shifu ls` 列出全部，`shifu jump` 跳到等待处理的会话。

## 快捷键

| 键 | 作用 |
|---|---|
| Cmd+N | 新建任务 |
| Cmd+T | 在当前任务里加一个 agent |
| Cmd+W | 关闭当前面板（agent 不停） |
| Cmd+J | 跳到等待处理的会话，优先处理批准请求 |
| Cmd+, | 外观 |
| 双击标签 | 给这个 agent 改名，留空就用回话题名 |
| 点标签上的 × | 选择「收起，继续运行」或「结束」；运行中结束另有确认 |

## 功能入口

| 想做的事 | 入口 |
|---|---|
| 让 Agent 查看和操作本机窗口 | [`shifu computer`](docs/COMPUTER-USE.md)，使用独立的 macOS 辅助程序 |
| 从手机接回正在运行的会话 | [手机连接](docs/REMOTE.md)，单独安装本机服务并配置私有网络 |
| 让长任务继续运行 | [杯子菜单](docs/STEAMCUP.md)，屏幕模式、定时与合盖运行 |
| 查看全局或在会话中传话 | 左侧「项目经理」；传话仍需本人明确授权 |

会话搜索、额度和进程占用也在同一个窗口里。点餐是可选的本机服务桥接，服务与账号需要另外配置。

## 隐私

- Shifu 不提供托管服务，不收集遥测。状态、任务、布局都存在本机 `~/.shifu/app`（命令行版在 `~/.shifu`）；可选手机入口只监听本机 `127.0.0.1`，远程连接通过本人配置的私有网络。
- Claude Code、Codex、CodeBuddy 的 hooks 通过启动参数挂在 Shifu 开的那个进程上，**不改**你的 `~/.claude/settings.json`、`~/.codex/config.toml`。
- **Kimi**：Kimi 只认自己的配置文件，Shifu 会把 hooks 写进 `~/.kimi-code/config.toml`，你自己的 hook 原样保留，第一次写之前备份成 `config.toml.before-shifu`。在 Shifu 外面开的 Kimi 也会调用这些 hook，但它们发现不是 Shifu 开的就什么都不做。
- **Antigravity**：会在 `~/.gemini/config/hooks.json` 写入 `shifu-status` 分组，保留其他分组；首次改写已有文件前备份为 `hooks.json.before-shifu`。
- 额度显示：Claude 的额度从它交给 statusLine 的数据里取（你自己的 statusLine 照常显示）；Codex 和 CodeBuddy 的额度读本机会话记录；Kimi 的额度复用 Kimi Code 本机有效登录令牌查询，不另存该令牌；Antigravity 通过当前登录的 `agy` 读取用量。
- Shifu 用自己的 tmux socket（`shifu-app`，命令行版是 `shifu`），不碰你原来的 tmux。

## 已知问题

- 只在 macOS（Apple Silicon）上用过。Linux 和 Windows 没试过，通知、拖放这些用的是 macOS 的接口。
- 某个项目目录里的 `git` 命令卡住时（比如在等 macOS 的「访问桌面文件夹」授权），整个窗口会跟着卡住，直到 git 返回。原因已经找到：读分支名时拿着锁跑 git；还没修。
- 终端里折成三行以上的长路径，点后面几行打不开。原因没查清。
- Kimi Code 登录过期时不能刷新额度；会保留上次结果，运行 Kimi 更新登录后再刷新。
- 手机外网连接需要另行配置私有网络。点餐入口需要本机 `~/.shifu/app/waimai.json` 提供有效的绝对路径 `tool_script`，并在 `127.0.0.1:3816` 运行兼容服务；登录、地址与购买权限由该服务管理。
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

## 许可证

[MIT](LICENSE) © Yutai Lin。第三方代码与素材的来源见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

早期 SHIFU 流程插件保留在 [`process-plugin-final`](https://github.com/Longado/shifu/tree/process-plugin-final)。
