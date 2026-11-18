# StepFun Step Plan

[中文](#中文) / [English](#english)

## 中文

新建任务和添加会话可选 StepFun。使用 Step Plan 订阅接口 `https://api.stepfun.ai/step_plan/v1`，默认 Step 3.7 Flash，也可选 Step 3.5 Flash 和 Step 5 Preview；不会回退到按量接口。模型权限取决于你的套餐。StepFun 会话支持运行/回复状态及退出后继续准确的原对话。

Step 5 Preview 的上下文窗口为 500000 token，两个 Flash 模型保持 128000；输出上限仍为 8192。更新到包含此修复的 CLI 后，退出并继续原会话才能载入新窗口。扩展由 CLI 每次启动时生成，手改生成文件不会保留。

需要 Node.js 20.6+，并为此入口单独准备 Pi 0.73.1（不会替换你已有的 `pi` / `step`）：

```sh
npm install --prefix "$HOME/.local/share/shifu/stepfun" --no-audit --no-fund --ignore-scripts @mariozechner/pi-coding-agent@0.73.1
```

密钥从 `STEP_API_KEY` 环境变量读取。由你信任的本地密钥管理工具注入环境后，可运行 `shifu stepfun`；不要把密钥放在命令参数、仓库或会话文件里。桌面启动器在变量缺失时调用 `$HOME/.local/bin/hk run 'StepFun · Shifu' -- ...`，因此使用桌面入口还需你自己的兼容密钥库设置；hk 不随本项目分发，也不自动安装。使用这个入口前请自行完成密钥库配置，条目环境变量名为 `STEP_API_KEY`。没有密钥库时仍可通过环境变量使用独立 CLI。

GUI 和 CLI 应成对更新。配置和会话分别保存在当前 `SHIFU_HOME/stepfun`；默认禁用 Pi 的扩展/技能/主题自动发现，只加载 Shifu 的状态扩展，项目 AGENTS.md / CLAUDE.md 照常读取。内置读写文件和命令工具采用 Pi 自身行为。订阅额度、价格显示、历史全文搜索、每日用量和自动驾驶尚未适配；Pi 显示的美元成本不能代表套餐余额。

## English

Select StepFun when creating a task or adding a session. It uses the Step Plan subscription endpoint `https://api.stepfun.ai/step_plan/v1`, defaults to Step 3.7 Flash and also offers Step 3.5 Flash and Step 5 Preview, subject to your plan. There is no pay-as-you-go fallback. Session status and exact-session resume are supported.

Step 5 Preview uses a 500,000-token context window; both Flash models retain 128,000 tokens. The output limit remains 8,192 tokens. After installing a CLI containing this fix, exit and resume the session to load the new window. The CLI regenerates the extension on every launch, so manual edits to the generated file do not persist.

Install Node.js 20.6+ and prepare the separate Pi 0.73.1 runtime using the command above. It does not replace existing `pi` or `step` commands. Inject `STEP_API_KEY` through a trusted local secret manager before running `shifu stepfun`; never put a key in command arguments or repository files. When that environment variable is absent, the desktop launcher calls `$HOME/.local/bin/hk run 'StepFun · Shifu' -- ...`, requiring your own compatible vault setup. hk is not distributed or installed by Shifu. Standalone CLI use supports environment injection without hk.

Update the app and CLI together. The isolated runtime state and sessions live under `SHIFU_HOME/stepfun`. Automatic Pi resource discovery is disabled; project AGENTS.md / CLAUDE.md are still read. Pi controls its built-in file and command tools. Subscription quota, pricing, transcript search, daily usage and autopilot are not integrated. Pi's dollar-cost field does not represent the subscription balance.
