# Shifu Computer Use

让 Shifu 里的 Claude Code、Codex、Kimi 等通过 `shifu computer` 查看和操作本机窗口。支持 macOS 14+，运行不依赖 Orca。首次使用需要为 **Shifu Computer Use** 授权辅助功能和屏幕录制。

## 统一使用指引

Claude Code、Codex、Kimi 等调用者可先读取同一份指南：

```bash
shifu computer guide
shifu computer guide --json
```

指南不启动辅助程序，也不需要系统权限；JSON 返回 `version` 和与纯文本相同的 `guide`。内容说明如何选窗口、使用新控件索引、执行单次动作、核对结果以及处理失败。新版本的项目经理指引包含这个入口。

Shifu 新建的 Claude、Codex 会话在首次启动时自动收到简短指南入口；Kimi 在该会话首次提交提示时收到一次。使用各家已有 hook 追加上下文，不替换系统提示、不修改项目指令或全局设置，也不向现有/恢复会话补发消息。只有已登记且类型匹配的主会话会收到入口。拿到入口并不代表模型已经执行指南或打开截图。

截图仍返回本地路径。调用 Agent 需要使用自己的看图工具打开 `result.screenshot.path`；文件存在或拿到路径均不代表模型已经看到图片，本轮未增加自动图片反馈。

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

动作回读可返回增量控件树：`snapshot.treeDiff:true` 表示 `treeText` 只列 `~` 变化行、`-` 删除行或 `No change`，并保留窗口头和焦点摘要；`baselineSnapshotId` 指向可重建的前次状态。未列出的控件及索引沿用基线。没有 `treeDiff` 时按完整树读取；`get-app-state` 始终完整，动作加 `--full-tree` 也可强制完整。出现新增、移动、结构/几何变化、截断或无可靠基线时自动回退完整树。

内部缓存始终完整。文字/文件条件和筛选前强制完整观察；不会因增量树省略文字而误判文字消失。`--match`/`--limit` 筛选结果不能作为完整基线，之后的下一动作返回完整树。等待多轮时仍只执行一次动作。

- 结果是 JSON，包含控件树、窗口、动作和验证信息。截图保存到 `~/.shifu/computer/screenshots/`，响应只返回路径，不把整张图片的 base64 塞进上下文；无需截图时加 `--no-screenshot`。
- 每次 UI 改变后读取返回的新状态再操作。索引会过期，不能猜测或沿用其他对话的索引。默认使用 `SHIFU_ID` 隔离操作上下文；在外部终端并发操作时各自指定 `--session <名字>`。
- 默认后台输入；只有本人允许接管前台时才加 `--restore-window`。原生弹出选项优先 `set-value`，不把后台点击失败当作成功。
- `ok: true` 表示请求执行成功，动作是否达成要看 `action.verification` 和实际回读。`unverified`、超时、断线均不能宣称完成；先观察再决定下一步，不能盲目重试可能发送/删除的操作。
- 动作已返回、后续读取失败时，返回 `ok: false / observation_failed`，保留 `result.action` 和实际目标窗口；`result.observationError`（原生读取）或 `result.postcondition.state`（等待核对）说明失败发生在观察阶段。此时不要重复动作，先重新读取目标。
- 返回 JSON 的失败响应还包含 `result.recovery`：`nextStep` 为 `observe`、`check_permissions` 或 `stop`，`automaticRetry` 始终为 false。`actionReported` 仅说明是否有动作回执，为 false 时不能推断动作未发生。原始错误、快照和动作保留；恢复建议不会自行执行输入、切前台或重做动作。首次传输错误仍直接报告结果未知。
- 不自动接管授权框、提交表单、发送消息、购买或删除。调用者仍受当前任务授权约束。辅助程序保留上游对敏感系统应用的拦截。

## 筛选、等待和结果核对

```bash
# 只显示包含“保存”的前五个控件，原控件索引不变。
shifu computer get-app-state --app com.apple.TextEdit --match 保存 --limit 5 --no-screenshot
# 等待真实正文；只在允许暂时切换前台时加 foreground-fallback。
shifu computer get-app-state --app com.apple.Safari --wait-for 'text We provide a web service' --timeout-ms 5000 --foreground-fallback --match 'We provide'
# 等待当前窗口中的“正在保存”消失；也可与 --wait-for 一起要求结果出现。
shifu computer get-app-state --app com.apple.TextEdit --wait-for-gone 正在保存 --timeout-ms 5000 --no-screenshot
# 操作后核对新路径出现、旧路径消失。路径必须是绝对路径。
shifu computer press-key --app com.apple.finder --window-id 123 --key Return --expect-path /tmp/after.txt --expect-missing-path /tmp/before.txt --timeout-ms 3000
```

`--match` 不区分大小写；`--wait-for`、`--wait-for-gone` 是控件行中区分大小写的文字片段，不匹配窗口标题或末尾的焦点摘要。可用 `heading ...`、`text ...` 等更具体的片段等待正文。消失条件要求控件树非空且未截断；它只证明当前可读控件里没有该文字，后台缺正文时应同时等待明确的结果文字，不能据此推断保存或业务完成。筛选在等待判断之后进行，返回 `snapshot.view` 的匹配/显示/省略数量；`elementCount` 仍是完整窗口数量，原生树自身的截断信息另行保留。

等待默认 5 秒，可用 `--timeout-ms` 设为 0–30000 毫秒。只重复观察，不重做点击、输入等动作；窗口出现后固定其 ID，按序号操作后的回读也固定到实际接收动作的窗口，防止窗口重排后读错。原生读取本身也需要时间，因此总耗时可能超过等待预算。权限和传输错误立即返回。`--foreground-fallback` 仅用于带条件的 `get-app-state`，在等待预算过半后尝试一次前台读取，并恢复原前台应用；未达到条件时继续后台观察到预算结束，不再切前台。原焦点窗口可读取时也恢复该窗口，不会恢复页面内部的选择或滚动位置，不用于自动重试动作。

动作的 `verification.scope: element` 仅表示控件层面的核对；`taskCompletion: not_checked` 不宣称业务任务完成。显式提供的条件另记在 `result.postcondition`，超时令 `ok: false`，但保留原动作回执。文件条件只核对路径存在性，不证明内容、保存成功或变化由本次动作造成。

显式指定 `--window-id` 或 `--window-index` 时，文字条件只核对实际接收动作的窗口；该窗口关闭后，即使返回了其他窗口的快照，也会报 `observation_failed`，不会用其他窗口已有的文字判成功。仅文件条件仍可在输入窗口关闭后核对。若要查看应用随后出现的窗口，应另行读取并选择新目标；未显式指定窗口的文字条件沿用按应用观察的范围。

## 弹出的输入窗口与多窗口操作

Finder 等应用可能把临时编辑框作为独立的小窗口暴露，而不把它放在原窗口的控件树里。现在这些焦点输入窗口会被列出；`snapshot.focusedInputWindowId` 给出其 ID。未指定窗口时，`get-app-state` 优先读取这个输入窗口；指定了 ID 或索引时仍只读指定目标。

需要验证界面改名时：先选中测试文件并按 Return 进入编辑，读取当前输入窗口及控件索引，向该控件 `set-value`，最后向**输入窗口**发送 Return，并用 `--expect-path` / `--expect-missing-path` 核对文件结果。不要把列表中的文件名控件赋值当作提交。仅需处理文件时，仍优先用现有文件接口。

`--restore-window` 会激活指定的窗口；窗口索引在激活前解析，避免窗口顺序变化后选错。已有焦点的输入框不会被重复打开应用的动作打断。输入框提交后即使关闭，动作回执的 `action.targetWindowId` 仍记录实际接收操作的窗口；返回的 `snapshot.window.id` 可以是操作后的主窗口。

**目前限制：** Safari 后台有时只返回工具栏；需要正文时显式等待正文，必要时使用前台读取回退。此轮修复了目标窗口选择与恢复原窗口，没有增加浏览器 DOM 通道，也不保证所有应用的自定义控件可操作。

回读的 `snapshot.foreground` 包含 `state`（`target_app`、`other_app`、`unknown`）、`targetAppPid` 和可空的 `frontmostAppPid`，只描述该次回读采样时的前台状态；前台回退读取结束后仍会恢复原应用。前台按键派发前同时检查目标窗口焦点和目标应用是否在前台，读取不到或不匹配时拒绝派发。它不能阻止按键之后用户或其他应用切换前台。Finder 临时改名框可能随这种切换消失，此时应先观察、重新选择目标，不自动重发 Return 或继续向旧输入框赋值。

## 文件选择和上传

默认后台操作仍可能触发应用自己的弹窗；当前没有强制静默策略或浏览器上传接口。

原生选择框及其“前往文件夹”路径框可作为独立 sheet 暴露。按 `list-windows` 返回的真实窗口 ID 读取、操作；即使小面板位于大网页范围内，也不会用网页树冒充它。Firefox 只在焦点接口暴露、未列入普通窗口数组的面板也会参与匹配。明确身份不符、无法唯一匹配时拒绝，不按包含面积猜窗口。

`--restore-window` 复用同一组面板候选；显式目标找不到时不改选父窗口。共享桌面仍可能被其他应用切走，焦点检查失败时先观察文件框及路径框，不能去掉保护或把后台快捷键回执当成成功。实际 Firefox 流程中后台快捷键曾未生效，前台发键后也须重新列窗口确认路径框出现。

标准 macOS 流程：

1. 核对要上传的本地文件及站点；打开选择框后重新列窗口。若按钮只返回 `suspected_noop`，先观察窗口及附件状态；必要时从新截图确认位置，在允许前台操作的范围内切换输入方式，不重复盲点。
2. 对选择框发送 `hotkey --key CmdOrCtrl+Shift+G --restore-window`，发现新路径 sheet 后读取聚焦文本框，用 `set-value --value-stdin` 输入完整绝对路径，并核对值。
3. 向路径 sheet 发送一次 Return。等待它关闭，重读原选择框；确认目标文件名在唯一选中的文件行里，Open 可用，再对该按钮执行一次点击。
4. 回到网页核对实际附件名称、大小及上传完成/错误状态；自动解析后仍核姓名、邮箱、任职、学历及日期。原生选文件、网页上传、最终提交是不同结果，不能只凭窗口关闭或拿到回执宣称上传成功。

可复跑的本地流程：编译 `FilePickerFixture.swift`，设置较短的隔离 `SHIFU_COMPUTER_HOME`（例如独立的 `/tmp/shifu-picker-test`，避免 Unix socket 路径长度限制）后运行 `verify-file-picker.py NEW_OUTPUT COMPILED_FIXTURE`。它只选择自己创建的中文/空格路径文件，校验文件名及单次选择，再停止自己的 fixture/broker。`FilePickerFixture.html` 可在现有 Firefox 中检查浏览器附件读回，无表单或网络上传。

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

需要复查真实窗口行为时，可手动运行：

```bash
python3 native/computer-use-macos/verify-real-tasks.py /tmp/shifu-computer-check-unique
```

该检查只创建自己的 Finder 测试文件和两个本机 Safari 页面，会短暂切换前台，结束后关闭自己的 Finder 窗口、按唯一完整 URL 关闭测试标签并恢复原应用；不修改既有文件。Safari 不按整个窗口关闭，避免影响后来加进同一窗口的其他标签。目录必须尚不存在，结果与测试文件保留在该目录。可通过 `SHIFU_CLI`、`SHIFU_COMPUTER_HOME`、`SHIFU_COMPUTER_HELPER` 指向候选版本。

`verify-observation-tasks.py <新结果目录> <编译后的Fixture>` 专门检查窗口重排后回读、文字消失条件、关闭最后窗口后仍保留动作回执。需设置上述三个变量，使用独立的 computer home；只针对测试进程 PID，结束后停止自己的转接进程和测试窗口。

完整任务可重复运行并汇总：

```bash
python3 native/computer-use-macos/evaluate-tasks.py /tmp/shifu-tasks-new --runs 3 --cli /absolute/path/to/shifu
```

输出目录必须尚不存在。评估器使用独立的短路径 computer home，逐轮执行 Finder 改名和 Safari 多窗口任务；一项失败后仍执行另一项。`report.json` 分别记录 `passed/failed/not_run`、失败阶段、原始错误、耗时、证据和清理结果。每轮最多90秒；清理未完成或无法确认时保留未知状态。`owned-windows.json` 记录 Finder 窗口；Safari 在创建前写入 `owned-pages.json`，即使标题未出现或子进程超时，外层仍按登记 URL 清理。`pageCleanup.remaining` 为空只证明这些 URL 已不存在；用户把测试标签导航到别处时，不再追着关闭新页面。

报告标为 `scripted_ui_tasks`：这里测量脚本驱动的界面任务可靠性，未包含模型决策，不能称为 Agent 自主任务成功率。没有持续监测的人工介入和前台打断次数保留为 unknown；单按钮、文字等待等机制用例不计作额外完整任务。

比较两个执行器的共同能力，可运行 `compare-providers.py OUTPUT --fixture ABS --shifu ABS --orca ABS --runs 3`。两边使用同一编译的单窗 Fixture、交替执行顺序，完成相同的读取、计数点击、中文赋值和完整回读；只操作自建 PID，无截图或前台接管。记录完整观察和全部响应的控件树字节数、耗时及原始失败，不把字节数当模型 token，也不把小任务对照推广为复杂应用成功率。
