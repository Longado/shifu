# 轻量后台托管

[回到 Shifu](../README.md) · [English](#english)

让主控在你离开后查看一至两个现有 Codex 会话的完整回复和必要交付文件，在已批准目标内安排下一步，到期结束。入口是源码中的 `scripts/autopilot.py`，需要 Python 3.9+、已安装的 Shifu CLI 和已登录的 Codex CLI，不另装 Python 依赖。当前预发布下载包不含此入口，桌面版没有新增托管按钮。

## 准备本轮配置

先由主控核对目标会话、当前用户授权和允许修改的范围，再在仓库外的私有目录保存 JSON。下面是字段模板，所有占位内容都需替换；不要复用上一轮的截止时间或授权锚点。

```json
{
  "run_id": "reviewed-run",
  "deadline": "YYYY-MM-DDTHH:MM:SS+08:00",
  "shifu_home": "/absolute/path/to/shifu-home",
  "shifu_socket": "shifu-app",
  "state_path": "/absolute/path/to/private-run/state.json",
  "log_path": "/absolute/path/to/private-run/events.jsonl",
  "stop_path": "/absolute/path/to/private-run/STOP",
  "agents": [{
    "name": "exact-internal-agent-name",
    "session_id": "pinned-session-id",
    "authorized_user": {"offset": 0, "sha256": "REPLACE_WITH_REVIEWED_USER_CONTENT_SHA256"},
    "cwd": "/absolute/path/to/project",
    "goal": "本轮已批准的具体开发目标",
    "allowed_paths": ["/absolute/path/to/project"],
    "preserve": ["保留他人改动；不安装或重开正式应用"],
    "artifacts": ["/absolute/path/to/project/HANDOFF.md"]
  }]
}
```

- `name / session_id / cwd` 必须匹配 `shifu_home/agents/<name>.json` 中的现有 Codex 会话。桌面版通常使用 `~/.shifu/app` 和 `shifu-app`；命令行版通常使用 `~/.shifu` 和 `shifu`。按实际实例填写。
- `authorized_user` 绑定已审阅的用户任务记录。模块中的 `latest_user_turn(transcript_path, session_id)` 可读取最新完整 user 的字节偏移和内容指纹；主控必须确认该条确实授权本轮任务，不能把暂停或人工接手当成新授权。配置不会自动改绑。
- `allowed_paths` 限定后续派发的修改位置，`artifacts` 指定必要只读文本材料；这是任务范围约束，不是操作系统沙箱。保留事项随派发传给项目 Agent。
- 可选 `shifu_bin / codex_bin / transcripts_dir` 指定命令和会话目录。默认会话目录为当前 `CODEX_HOME` 下的 `sessions`，未设置时为 `~/.codex/sessions`。不修改登录或全局配置。

## 启动、查看、停止

在仓库根目录执行，配置路径换成本轮已审阅的文件：

```sh
python3 scripts/autopilot.py --config /absolute/path/to/reviewed-config.json --start
python3 scripts/autopilot.py --config /absolute/path/to/reviewed-config.json --status
python3 scripts/autopilot.py --config /absolute/path/to/reviewed-config.json --stop
```

`--start` 在后台登记成功后返回，关闭主控终端仍继续；没有安装常驻系统服务。`--status` 只读运行状态、目标动作、次数、耗时与 Codex 实际报告的 token，不调用模型。`--stop` 和截止时间结束本轮控制器及其判断子进程，项目 Agent 和 App 继续运行。截止时间阻止新的派发，不会强行终止已交给项目 Agent 的任务。

`--dry-run --once` 会调用 Codex 判断并记录结果，使用独立 `.dry-run` 状态和日志，但不发送任务；它仍会把必要会话和文件内容交给当前 Codex 模型，并非离线检查。dry-run 与真运行共用配置中的 STOP，勿同时作为两位主控运行。

## 执行与停止边界

- 只向配置中的一至两个现有 Codex Agent 派发，不创建项目 Agent。只在 `idle / needs_reply` 时接续，不回答批准框。判断器只输出受限 JSON，不执行开发工具；开发继续由原项目 Agent 完成。
- 读取完整近期回复和必要 UTF-8 文本文件；最后回复里、允许范围内的本地 Markdown 文件链接也会读取。图片、PDF、网页和 App 内部需要主控提供另行审阅的结果。
- 每0.5秒检查文件元信息，状态异常或运行中的兜底复查默认60秒。输入不变不重复调用模型或写日志；两条线的判断和发送独立推进。
- 新回复复用未变文件的完整内容和指纹；路径、设备、inode、大小、mtime 或 ctime 变化会失效。只保留当前证据缓存，重启重新读取。发送前仍复核状态、身份、人工输入及文件。
- 人工输入会持久停止该目标。判断失败或发送结果未知不自动重试；重启也保留停止和未知记录。先查真实会话及状态记录，再由主控决定新一轮，不删除旧记录绕过保护。CLI 回执只代表传话命令返回，不代表任务完成。
- 默认完整读取预算为：对话尾部8MiB、单文件64KiB、最多16份文件、判断上下文160,000字节；超限停止并报告，不截短代替审阅。可选配置为 `recent_messages`（默认6）、`max_transcript_bytes / max_artifact_bytes / max_context_bytes`。判断超时默认90秒（最大300秒），命令超时15秒，可用 `judge_timeout_seconds / command_timeout_seconds` 调整。

`--status` 的 `metrics` 包含 `artifact_reads / artifact_cache_hits / artifact_bytes_read`：实际文件读取次数、复用次数和读入字节数。它们用于观察文件处理开销；模型仍收到完整证据，不能据此推算 token 或金额节省。`usage` 中缓存输入是输入的子集，不重复相加，缺报显示 null。

状态和日志包含会话依据、派发全文及回执，按0600保存，新目录0700。应保留在私有目录，不提交 Git。判断使用当前 Codex 登录和模型，必要内容会发送给该模型服务；脚本不代管凭据、不切换模型、不自动部署。

## English

The source-only `scripts/autopilot.py` follows up on one or two existing Codex agents within reviewed goals and a deadline. It needs Python 3.9+, Shifu CLI and a signed-in Codex CLI; no Python packages are required. Existing preview downloads do not include this script, and there is no new desktop button.

Save the JSON template above outside the repository in a private run directory, replacing every placeholder. Pin the exact internal agent name, session ID and working directory from the existing Shifu agent record. `authorized_user` must reference a user task the controller has reviewed and confirmed as authorization. The module's `latest_user_turn(transcript_path, session_id)` returns the latest complete user's offset and content hash; a pause or human takeover is not authorization. Set allowed edit paths, required text artifacts, preservation instructions and a fresh deadline. These constrain dispatch scope; they are not an OS sandbox.

Use the start/status/stop commands above with that reviewed configuration. Start returns once the detached process registers. Status is read-only and makes no model call. Stop and expiry end the controller and its own judgment children, leaving project agents and the app running. Expiry prevents new dispatches; it does not forcibly terminate work already assigned to a project agent. No system service is installed.

`--dry-run --once` uses separate `.dry-run` state/log files and never dispatches, but it still calls the configured Codex model with necessary conversation and artifact content. It shares the configuration's STOP marker with a live run; do not use both as simultaneous controllers.

Only `idle` and `needs_reply` targets can receive follow-ups. Approval prompts are left for a person. The judge returns constrained JSON with development tools disabled; the original project agent performs the work. Human input persistently halts that target. Failed judgments and uncertain sends are not retried, including after restart. Inspect the original conversation and durable records before reviewing a new run; do not erase records to bypass a halt. A CLI receipt does not prove project completion.

The controller reads complete recent replies and required UTF-8 artifacts, including local Markdown links inside the target's allowed paths. Images, PDFs, web pages and app internals require separately reviewed input. Defaults: an 8MiB transcript tail, 64KiB per artifact, up to 16 artifacts and 160,000 context bytes; oversized input stops rather than being truncated. Optional keys are `recent_messages` (6), `max_transcript_bytes`, `max_artifact_bytes`, `max_context_bytes`, `poll_seconds` (60), `judge_timeout_seconds` (90, maximum 300) and `command_timeout_seconds` (15). `shifu_bin`, `codex_bin` and `transcripts_dir` can override executable locations and the transcript directory.

File metadata is checked every 0.5 seconds; idle unchanged input does not repeatedly invoke CLIs/models or append logs. Two targets progress independently. New replies reuse complete unchanged artifacts, invalidating on path/device/inode/size/mtime/ctime changes and dropping obsolete delivery links. Restart rereads files. Identity, state, human input and evidence are still checked before dispatch.

Status metrics `artifact_reads`, `artifact_cache_hits` and `artifact_bytes_read` measure actual file I/O. They do not imply smaller model prompts, faster inference or billing savings. Reported cached input tokens are a subset of input tokens; missing usage is null. State and logs contain conversation evidence and messages: keep them private (0600 files, 0700 new directories). Necessary input goes to the current Codex model service. The script does not change login, model, global configuration or application installation.
