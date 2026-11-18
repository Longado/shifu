# Qwen Code / Bailian Token Plan

[中文](#中文) / [English](#english)

## 中文

当前源码在新建任务和添加会话中提供 Qwen Code，支持模型选择、运行状态和继续原会话。已有下载包尚未包含此入口；从源码使用时应配套更新 App 和 CLI。

先安装并准备自己的 Qwen Code CLI；本次集成使用 0.27.4。Shifu 不分发 Qwen 程序或订阅密钥。入口固定使用百炼 Token Plan 地址 `https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`，不回退到按量接口，也不允许用额外启动参数替换鉴权或地址。

密钥按以下顺序读取：

1. 当前进程的 `DASHSCOPE_API_KEY` 环境变量。
2. 本人准备的 `~/.config/secrets/dashscope.env` 中的 `DASHSCOPE_API_KEY=...`。可带 `export ` 和成对的单/双引号；这是纯文本配置，只读取该赋值，不执行 shell 内容。文件由本人管理，建议权限为 `600`。
3. 环境和文件均无密钥时，调用兼容本地密钥库 `$HOME/.local/bin/hk run 'Qwen · Shifu' -- ...`。hk 不随 Shifu 分发；没有该工具时使用前两种方式。

文件入口用于桌面/tmux 启动没有继承交互式 shell 环境的情况。Shifu 不创建该密钥文件，也不会把密钥值写进生成的模型设置、启动参数或仓库。

完成设置后可运行：

```sh
shifu qwen
shifu qwen --model qwen3.8-flash
```

也可在「＋」选择 Qwen Code。默认选 `auto`；内置目录另外列出 Qwen4 Max、Qwen4 Max Preview、Qwen3.8 Max、Qwen3.8 Flash，实际可用性由账号和套餐决定，不表示每项都已做在线调用验证。恢复时沿用准确会话 ID 和已选模型。

此入口使用 `--approval-mode yolo`，文件与命令不逐项请求批准。Qwen 配置、会话和状态 hooks 使用当前 `SHIFU_HOME/qwen` 的独立目录；默认关闭自动更新、遥测和使用统计。项目 AGENTS.md、CLAUDE.md、QWEN.md 照常读取。订阅额度、历史全文搜索、每日用量统计和专用自动驾驶尚未适配。

## English

The current source adds Qwen Code to the new-task and add-session pickers, including model selection, lifecycle status and exact-session resume. Existing release packages do not include it yet; update the app and CLI together when building from source.

Provide your own Qwen Code CLI and subscription. This integration uses Qwen Code 0.27.4 and pins the Bailian Token Plan endpoint `https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`. It does not fall back to a pay-as-you-go endpoint or accept credential/base-URL overrides in extra launch arguments.

Credentials are read in order from the process's `DASHSCOPE_API_KEY`, then an optional user-managed `~/.config/secrets/dashscope.env`, then a compatible `$HOME/.local/bin/hk run 'Qwen · Shifu' -- ...` launcher. The env file accepts `DASHSCOPE_API_KEY=...`, optionally prefixed by `export ` and enclosed in matching quotes. Its contents are parsed, never executed. The file is plaintext, should be mode `600`, and is not created by Shifu. hk is not bundled; use environment or file configuration if you do not have it.

The file option covers desktop/tmux launches that do not inherit interactive shell configuration. The key value is not written into generated model settings, command arguments or the repository. After setup, use the commands above or select Qwen Code in the “+” picker.

The default model is `auto`. The embedded catalog also lists Qwen4 Max, Qwen4 Max Preview, Qwen3.8 Max and Qwen3.8 Flash; access depends on the account and plan, and the list is not a claim that every model has been exercised online. Resume keeps the exact conversation and chosen model.

The launcher uses `--approval-mode yolo`, allowing file and command tools without individual approval. Configuration, sessions and hooks use an isolated `SHIFU_HOME/qwen` directory. Automatic updates, telemetry and usage statistics default to off; project AGENTS.md, CLAUDE.md and QWEN.md remain available. Subscription quota, transcript search, daily usage statistics and dedicated autopilot are not integrated.
