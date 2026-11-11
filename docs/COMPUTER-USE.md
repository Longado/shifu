# Shifu Computer Use

让 Shifu 里的 Claude Code、Codex、Kimi 等通过 `shifu computer` 查看和操作本机窗口。支持 macOS 14+，运行不依赖 Orca。首次使用需要为 **Shifu Computer Use** 授权辅助功能和屏幕录制。

## 安装与权限

在仓库根目录运行：

```bash
cargo install --path . --locked --force
bash native/computer-use-macos/build-app.sh
ditto 'target/computer-use/Shifu Computer Use.app' "$HOME/Applications/Shifu Computer Use.app"
shifu computer permissions
```

构建脚本沿用项目配置里的签名身份，也可设置 `SHIFU_SIGNING_IDENTITY`。保持 bundle ID `dev.longado.shifu.computer-use` 和签名身份稳定。系统授权由本人操作，不代点；不要重置或借用 Orca 的授权。已安装的 Shifu 桌面界面不需要重启。

## 看窗口、操作、回读

优先用应用已有的 API/CLI。必须操作界面时，先列应用和窗口，选定目标，再使用刚返回的控件索引：

```bash
shifu computer capabilities --json
shifu computer list-apps --json
shifu computer list-windows --app com.apple.TextEdit --json
shifu computer get-app-state --app com.apple.TextEdit --window-id 123 --json
# N 来自刚读到的 treeText，不是控件总数。
shifu computer click --app com.apple.TextEdit --window-id 123 --element-index N --json
shifu computer set-value --app com.apple.TextEdit --element-index N --value '文字' --json
```

其余操作见 `shifu computer help`：输入/粘贴、单键/快捷键、滚动（`--pages`）、拖动，以及控件提供的次级动作。坐标是所选窗口的局部坐标；截图像素须除以返回的 `scale`。文字可用 `--text-stdin` / `--value-stdin` 从标准输入传入，避免进入命令历史。

- 结果是 JSON，包含控件树、窗口、动作和验证信息。截图保存到 `~/.shifu/computer/screenshots/`，响应只返回路径，不把整张图片的 base64 塞进上下文；无需截图时加 `--no-screenshot`。
- 每次 UI 改变后读取返回的新状态再操作。索引会过期，不能猜测或沿用其他对话的索引。默认使用 `SHIFU_ID` 隔离操作上下文；在外部终端并发操作时各自指定 `--session <名字>`。
- 默认后台输入；只有本人允许接管前台时才加 `--restore-window`。原生弹出选项优先 `set-value`，不把后台点击失败当作成功。
- `ok: true` 表示请求执行成功，动作是否达成要看 `action.verification` 和实际回读。`unverified`、超时、断线均不能宣称完成；先观察再决定下一步，不能盲目重试可能发送/删除的操作。
- 不自动接管授权框、提交表单、发送消息、购买或删除。调用者仍受当前任务授权约束。辅助程序保留上游对敏感系统应用的拦截。

## 筛选、等待和结果核对

```bash
# 只显示包含“保存”的前五个控件，原控件索引不变。
shifu computer get-app-state --app com.apple.TextEdit --match 保存 --limit 5 --no-screenshot
# 等待真实正文；只在允许暂时切换前台时加 foreground-fallback。
shifu computer get-app-state --app com.apple.Safari --wait-for 'text We provide a web service' --timeout-ms 5000 --foreground-fallback --match 'We provide'
# 操作后核对新路径出现、旧路径消失。路径必须是绝对路径。
shifu computer press-key --app com.apple.finder --window-id 123 --key Return --expect-path /tmp/after.txt --expect-missing-path /tmp/before.txt --timeout-ms 3000
```

`--match` 不区分大小写；`--wait-for` 是控件行中区分大小写的文字片段，不匹配窗口标题或末尾的焦点摘要。可用 `heading ...`、`text ...` 等更具体的片段等待正文。筛选在等待判断之后进行，返回 `snapshot.view` 的匹配/显示/省略数量；`elementCount` 仍是完整窗口数量，原生树自身的截断信息另行保留。

等待默认 5 秒，可用 `--timeout-ms` 设为 0–30000 毫秒。只重复观察，不重做点击、输入等动作；窗口出现后固定其 ID。原生读取本身也需要时间，因此总耗时可能超过等待预算。权限和传输错误立即返回。`--foreground-fallback` 仅用于带条件的 `get-app-state`，在等待预算过半后尝试一次前台读取，并恢复原前台应用；不会恢复应用内部的选择或滚动位置，不用于自动重试动作。

动作的 `verification.scope: element` 仅表示控件层面的核对；`taskCompletion: not_checked` 不宣称业务任务完成。显式提供的条件另记在 `result.postcondition`，超时令 `ok: false`，但保留原动作回执。文件条件只核对路径存在性，不证明内容、保存成功或变化由本次动作造成。

**目前限制：** Finder 的文件行可返回 AXSetValue 成功，但文件未必改名；进入改名状态后，编辑框也可能不出现在辅助功能树。文件改名优先用文件接口，并核对旧、新路径。Safari 后台可能只返回工具栏；需要正文时显式等待正文，必要时使用前台读取回退。本工具没有浏览器 DOM 通道。

## 进程与数据

首次命令按需启动一个 `shifu computer _serve` 本地转接进程和一个签名的原生辅助程序。前者维持同一条原生连接，保留跨命令的控件快照；闲置 5 分钟退出，或运行 `shifu computer stop`。不注册开机启动，不修改休眠设置。

本地 socket、运行日志、截图在 `~/.shifu/computer`，目录仅本人可访问。原生连接使用临时随机凭据并校验连接者是启动它的父进程，临时凭据不会出现在返回值中；不读取 Claude、Codex、Kimi 等账号凭据。

开发验证可通过 `SHIFU_COMPUTER_HOME` 和 `SHIFU_COMPUTER_HELPER` 指向隔离目录和候选辅助程序。不要用测试清理脚本终止正式版或其他应用。

## 来源与适配范围

原生实现来自 [stablyai/orca](https://github.com/stablyai/orca/tree/d1466f6017c4bb6006b77eb46f2fc0f461ef021e/native/computer-use-macos)，固定提交 `d1466f6017c4bb6006b77eb46f2fc0f461ef021e`（2026-11-10）。保留 `native/computer-use-macos/LICENSE` 中的 MIT 许可与 Lovecast 版权；辅助程序包也包含该许可。Swift 包及模块名保留上游名字以便比较，但运行不连接 Orca。

保留上游 AX、截图、后台输入、窗口/控件身份验证、输入取消和快照缓存机制。Shifu 的适配是：独立 CLI/本地转接进程、自己的签名应用与权限入口、改为校验自己的父进程、截图落文件、中文使用说明。没有移植 Orca 的 Electron 桌面、账号管理、远程运行或浏览器。

## 验证入口

```bash
cargo test
cargo clippy -- -D warnings
swift test --package-path native/computer-use-macos
```

`native/computer-use-macos/Fixture.swift` 是无外部副作用的 AppKit 测试窗口：计数按钮和文字框，用于真实窗口的点击、输入、截图及回读。用 `swiftc -swift-version 6` 编译；只操作该测试窗口，不接管其他工作窗口。
