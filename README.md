<p align="center"><img src="app/src-tauri/icons/128x128@2x.png" width="112" alt="Shifu"></p>

<h1 align="center">Shifu</h1>
<p align="center"><strong>同时开十几个编程 agent，一眼看出谁在干活、谁在等你。</strong></p>
<p align="center">给同时跑 Claude Code、Codex、Kimi、CodeBuddy 的人用的 macOS 桌面工作台。</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue" alt="MIT"></a>
  <img src="https://img.shields.io/badge/tested%20on-macOS-black" alt="tested on macOS">
  <img src="https://img.shields.io/badge/Rust%20%2B%20Tauri%202-orange" alt="Rust + Tauri 2">
  <img src="https://img.shields.io/badge/status-Alpha-orange" alt="Alpha">
</p>

<p align="center">
  <a href="README.en.md">English</a> ·
  <a href="docs/ARCHITECTURE.md">架构</a> ·
  <a href="#快速开始">快速开始</a> ·
  <a href="#一起做">一起做</a>
</p>

agent 一多，终端标签就开始一个个翻：这个跑完了没有，那个是不是卡在等我批准。Shifu 把它们都放进同一个窗口，左边一栏直接写着「运行中」「等你批准」「在问你」「做完了」，点一下就跳过去。

**每个 agent 用它自己原本的界面** · **关掉窗口 agent 照跑** · **状态来自 agent 自己的 hooks，不靠猜屏幕**

## 11 月 11 日更新

- 电脑操作支持独立小输入窗口；按窗口 ID 或索引切换时会激活指定目标，临时前台读取后恢复原焦点窗口。附带可手动运行的 Finder/Safari 真实任务检查。

- Antigravity 额度监测：通过当前已登录的 `agy`（1.1.11+）读取[官方 `/usage` 报告](https://antigravity.google/docs/cli/commands/usage/)，底栏分 Gemini 与 Claude/GPT 显示已用周额度、5小时额度和重置时间，每5分钟刷新；失败会标明旧数据或不可用。无需 Shifu 保存账号凭据。

- 独立的 macOS 电脑操作：`shifu computer` 可读取窗口、截图、点击、输入、滚动和拖动，运行不依赖 Orca。需要给 Shifu 自有辅助程序授予系统权限；[安装与参数说明](docs/COMPUTER-USE.md)。
- 控件可以按文字筛选、限制返回数量，等待目标文字出现；Safari 可显式选择短暂前台读取后切回。文件条件未满足会返回失败，控件赋值成功不等于文件任务完成。
- 项目经理可以选择 agent，查看当前对话状态并通过本地协作命令传话；搜索可查当前对话及其内容，侧栏可隐藏，支持上下分屏。
- 底栏显示额度与进程占用，杯子按钮控制保持运行或保持屏幕开启；退出后释放保持唤醒。文件标签支持 HTML、音视频预览。

Finder 的独立改名输入框现已可读取、输入并提交；操作结果仍应核对文件路径。浏览器后台可能只有工具栏，等待文字时应选正文中的明确片段；目前没有 DOM 操作通道。

## 是什么

Shifu 是一个跑在 tmux 上的 agent 工作台，分两层：

- **`shifu` 命令行**：在一个专用的 tmux 服务里开 agent，接收各家 agent 的 hooks，把状态写进 `~/.shifu`。单独用它也行，tmux 里自带一列侧栏。
- **Shifu 桌面版**（`app/`，Tauri 2）：每个 agent 是一个终端标签，可以分屏、拖动，按「任务」分组；左边栏显示状态、在等你的排在最上面；左下角显示各家的额度用了多少。

agent 进程归 tmux 管，桌面版只是挂上去看。关掉、重启桌面版都不会打断正在跑的 agent。设计细节见 [架构说明](docs/ARCHITECTURE.md)。

> **早期版本。** 目前只在作者自己的 Mac（Apple Silicon、Homebrew tmux）上天天用过，没有现成的安装包，需要自己编译。**Shifu 默认用免确认模式启动 agent**（Claude 的 `--dangerously-skip-permissions`、Codex 的 `--dangerously-bypass-approvals-and-sandbox`、Kimi 的 `--yolo`），它们改文件、跑命令都不会先问你。不想这样，请先改 `src/launch.rs` 再编译。还没解决的问题见[已知问题](#已知问题)。

<p align="center"><img src="docs/images/shifu-main.png" width="900" alt="Shifu 主界面"></p>
<p align="center"><sub>界面截图，agent 和额度都是模拟数据，终端内容为空。</sub></p>

## 它怎么工作

| 你…… | 它会…… |
|---|---|
| 新建一个任务，选 Claude / Codex / Kimi / CodeBuddy、目录和模型 | 在专用 tmux 服务里起这个 agent，hooks 只挂在这次启动上 |
| 让 agent 去干活 | 左边那行变成转圈的「运行中」 |
| agent 停下来要你批准 | 变成闪烁的琥珀色点，同时进入顶部「在等你」 |
| agent 回了一句话、等你回复 | 变成蓝色环「在问你」；窗口不在前台时发系统通知，点通知直接跳到它 |
| 按 Cmd+J | 跳到等你最久的那个 agent |
| 把标签或左边的行拖到另一个任务上 | 这个 agent 改归那个任务，标签跟着过去 |
| 点终端里的路径或网址 | 网址用浏览器打开；路径可以在 Shifu 里预览、用默认程序打开或在 Finder 中显示 |
| 关掉 Shifu 再打开 | agent 都还在，每个任务的分屏布局原样恢复 |

## 可以试试这些

| 试试 | 会看到 |
|---|---|
| 开一个 Claude，让它写个文件，再切去别的标签 | 左边那行从「运行中」变成「在问你」，顶部「在等你」多一行 |
| 一个任务里点右上角「+」，再加一个 Codex | 那个任务从一行展开成标题加两行，两个 agent 可以并排 |
| 把一个标签拖到右边缘 | 分成左右两屏 |
| 把截图拖进终端 | 截图存一份副本，路径自动打进输入框 |
| Cmd+, | 外观设置：几套配色，也可以自己调颜色 |

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
| 点标签上的 × | 结束这个 agent；还在干活的会先问一次 |

## 隐私

- Shifu 没有服务器，也不收集任何数据。状态、任务、布局都存在本机 `~/.shifu/app`（命令行版在 `~/.shifu`）。
- Claude Code、Codex、CodeBuddy 的 hooks 通过启动参数挂在 Shifu 开的那个进程上，**不改**你的 `~/.claude/settings.json`、`~/.codex/config.toml`。
- **Kimi 例外**：Kimi 只认自己的配置文件，Shifu 会把 7 条 hook 写进 `~/.kimi/config.toml`，你自己的 hook 原样保留，第一次写之前备份成 `config.toml.before-shifu`。在 Shifu 外面开的 Kimi 也会调用这些 hook，但它们发现不是 Shifu 开的就什么都不做。
- 额度显示：Claude 的额度从它交给 statusLine 的数据里取（你自己的 statusLine 照常显示）；Codex 和 CodeBuddy 的额度读本机会话记录；Kimi 的额度用 Kimi 自己的登录状态向 Kimi 服务器查询。Shifu 不读、也不存任何令牌。
- Shifu 用自己的 tmux socket（`shifu-app`，命令行版是 `shifu`），不碰你原来的 tmux。

## 已知问题

- 只在 macOS（Apple Silicon）上用过。Linux 和 Windows 没试过，通知、拖放这些用的是 macOS 的接口。
- 某个项目目录里的 `git` 命令卡住时（比如在等 macOS 的「访问桌面文件夹」授权），整个窗口会跟着卡住，直到 git 返回。原因已经找到：读分支名时拿着锁跑 git；还没修。
- 终端里折成三行以上的长路径，点后面几行打不开。原因没查清。
- 用了 Kimi 内部的登录模块来查额度。Kimi 换代或改名后，左下角的 Kimi 额度可能停在「没在做事」。
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

## 关于这个仓库

这个仓库以前放的是 SHIFU 流程插件（按任务风险调整 AI 编程 agent 的流程深度）。那一版保存在 tag [`process-plugin-final`](https://github.com/Longado/shifu/tree/process-plugin-final)，从这次提交开始，仓库换成了这个工作台。

## 许可证

[MIT](LICENSE) © Yutai Lin。底部 Claude 小人的像素画法来自 Anthropic 的 claude-desktop-buddy（MIT），见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
