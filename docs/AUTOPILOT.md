# 轻量后台托管

[回到 Shifu](../README.md) · [English](#english)

让主控在你离开后查看一至两个现有 Codex 会话的完整回复和必要交付文件，在已批准目标内安排下一步，到期结束。入口是源码中的 `scripts/autopilot.py`，需要 Python 3.9+、已安装的 Shifu CLI 和已登录的 Codex CLI，不另装 Python 依赖。预发布包的 scripts/ 目录也提供该入口，桌面版没有新增托管按钮。

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

## 回访原总控及状态

可选 `supervisor` 对象使用与目标相同的 `name / session_id / cwd / authorized_user` 字段，固定原总控，不能同时列为目标。目标受阻或全部完成时，状态会交回该总控；忙时等待空闲后通知。通知回执未知会持久阻断，不自动重发，不猜测另一个会话。目标或总控的新人工输入都会触发相应的停止保护；配置变更应停旧轮、保留记录、核对新授权后开始新轮。

每条目标可增加 `read_paths`，授权读取范围外的候选/回执，不扩大修改权限。图片和 PDF 只提供路径、大小、指纹，不能据此认定看过内容。`--status` 分开显示进程驻留状态和 `activity_status`（工作、等待或完成/受阻）；全部目标完成或受阻并处理完通知后提前退出，不空挂到截止。

## 追加输入与成果进度（0.3.7 预发布）

这两项已包含在 [0.3.7 预发布包](https://github.com/Nalu-Lab/shifu/releases/tag/remote-v0.3.7-preview.1)的配套 App、CLI 和脚本中；0.3.6 及更早的包不含这些功能。安装不会自动开启托管或修改正在进行的任务目标。

向原有 Codex/Claude 会话追加输入时，明确使用队列；普通 `tell` 仍只发给停下的会话：

```sh
shifu tell exact-agent-name --queue --id batch-01-note --expires-at 1800000000 '已批准任务的补充要求'
shifu inbox exact-agent-name batch-01-note
shifu inbox exact-agent-name batch-01-note --cancel
```

替换示例截止秒数，消息 ID 在该会话内保持稳定。队列固定原生会话、目录和终端，重用相同 ID/内容返回原记录，不能用同一 ID 改内容。桌面 App 在原终端有可识别的空输入框时粘贴并提交；已有草稿、陌生界面、批准窗口会等待，不清空输入、不重开会话。当前适配 Codex `›` 和 Claude `❯` 输入框；版本改变或不识别时不会强行发送。

回执依次区分 `queued`（排队）、`delivering`（尝试中）、`written`（已写入终端）和 `received`（原生用户消息或 UserPromptSubmit 确认接收）。`received` 也不代表任务已完成。发送不确定或中断成为 `unknown`，不会自动重发；应检查原终端。只有尚未发送的 `queued` 能取消。可选 `--stop-file /absolute/path/to/STOP` 和 `--expires-at` 阻止尚未提交的输入继续发送；已经写入的内容不会撤回。

在已有 `supervisor` 配置旁加 `"queue_supervisor": true`，可以把成果停滞通知排进正在工作的原总控会话。保持默认 false 则仍等总控空闲。控制器停止/到期会停止尚未提交的本轮通知，实际确认接收前不会记作已送达。

每个目标可选配置成果文件和阈值，例如：

```json
"progress": {
  "path": "/absolute/path/to/project/progress.json",
  "target": 3,
  "stalled_after_seconds": 1800
}
```

目标 owner 审核实际成果后，更新该文件；使用稳定成果 ID、独立且不改写的回执文件和该文件的真实 SHA256：

```json
{
  "batch": "batch-01",
  "results": [{
    "id": "stable-result-id",
    "evidence": "/absolute/path/to/project/receipts/result-01.json",
    "sha256": "REPLACE_WITH_ACTUAL_FILE_SHA256"
  }]
}
```

成果文件及回执必须位于已审阅的 `allowed_paths` 或 `read_paths` 内。计数依据当前批次的完整有效回执；仅写 `completed: 3` 不被接受。同一成果 ID 或相同回执字节不能跨批次再计入，换名字或复制旧回执也不会刷新新增成果时间。文件缺失、变更、重复或超出读取预算时显示“成果待核”，不能当作 0 份正常成果。脚本核文件、指纹和去重；回执是否代表业务上的真正成功，仍需 owner/总控对照外部服务核实，脚本不会替你查询该服务。

App 的原 Task 下方显示“本轮 1/3”；超过阈值仍未新增则显示“需调整”，通知原总控核差额、换路径或补输入。单纯运行、回复、更新时间或开启空批次都不算新增成果。运行中和空闲中的目标都会继续计时；停滞通知本身不会结束托管，未达成果目标的完成自述也不能提前结束。未配置 `progress` 时保留原流程。截止后保留实际数量并标明托管结束；需要跨批次持续执行时，由原总控按本轮授权推进，目标不会自动重置。

## 执行与停止边界

以下默认接续规则保持不变；可选的原总控输入队列见上一节。

- 只向配置中的一至两个现有 Codex Agent 派发，不创建项目 Agent。只在 `idle / needs_reply` 时接续，不回答批准框。判断器只输出受限 JSON，不执行开发工具；开发继续由原项目 Agent 完成。
- 读取完整近期回复和必要 UTF-8 文本文件；允许修改或只读范围内的交付链接也会读取。图片和 PDF 仅提供元数据，内容仍需另行审阅。
- 每0.5秒检查文件元信息，状态异常或运行中的兜底复查默认60秒。输入不变不重复调用模型或写日志；两条线的判断和发送独立推进。
- 新回复复用未变文件的完整内容和指纹；路径、设备、inode、大小、mtime 或 ctime 变化会失效。只保留当前证据缓存，重启重新读取。发送前仍复核状态、身份、人工输入及文件。
- 人工输入会持久停止该目标。判断失败或发送结果未知不自动重试；重启也保留停止和未知记录。先查真实会话及状态记录，再由主控决定新一轮，不删除旧记录绕过保护。CLI 回执只代表传话命令返回，不代表任务完成。
- 默认完整读取预算为：对话尾部8MiB、单文件64KiB、最多16份文件、判断上下文160,000字节；超限停止并报告，不截短代替审阅。可选配置为 `recent_messages`（默认6）、`max_transcript_bytes / max_artifact_bytes / max_context_bytes`。判断超时默认90秒（最大300秒），命令超时15秒，可用 `judge_timeout_seconds / command_timeout_seconds` 调整。

`--status` 的 `metrics` 包含 `artifact_reads / artifact_cache_hits / artifact_bytes_read`：实际文件读取次数、复用次数和读入字节数。它们用于观察文件处理开销；模型仍收到完整证据，不能据此推算 token 或金额节省。`judgment_seconds` 也计入人工接手、停止或到期前已经等待的判断耗时；这些记录标记 `cancelled`，未收到的 token 用量保留 null，不补造金额。`usage` 中缓存输入是输入的子集，不重复相加，缺报显示 null。

状态和日志包含会话依据、派发全文及回执，按0600保存，新目录0700。应保留在私有目录，不提交 Git。判断使用当前 Codex 登录和模型，必要内容会发送给该模型服务；脚本不代管凭据、不切换模型、不自动部署。

## English

The independent `scripts/autopilot.py` follows up on one or two existing Codex agents within reviewed goals and a deadline. It needs Python 3.9+, Shifu CLI and a signed-in Codex CLI; no Python packages are required. The preview package also includes the script under scripts/; there is no new desktop button.

Save the JSON template above outside the repository in a private run directory, replacing every placeholder. Pin the exact internal agent name, session ID and working directory from the existing Shifu agent record. `authorized_user` must reference a user task the controller has reviewed and confirmed as authorization. The module's `latest_user_turn(transcript_path, session_id)` returns the latest complete user's offset and content hash; a pause or human takeover is not authorization. Set allowed edit paths, required text artifacts, preservation instructions and a fresh deadline. These constrain dispatch scope; they are not an OS sandbox.

Use the start/status/stop commands above with that reviewed configuration. Start returns once the detached process registers. Status is read-only and makes no model call. Stop and expiry end the controller and its own judgment children, leaving project agents and the app running. Expiry prevents new dispatches; it does not forcibly terminate work already assigned to a project agent. No system service is installed.

`--dry-run --once` uses separate `.dry-run` state/log files and never dispatches, but it still calls the configured Codex model with necessary conversation and artifact content. It shares the configuration's STOP marker with a live run; do not use both as simultaneous controllers.

Only `idle` and `needs_reply` targets can receive follow-ups. Approval prompts are left for a person. The judge returns constrained JSON with development tools disabled; the original project agent performs the work. Human input persistently halts that target. Failed judgments and uncertain sends are not retried, including after restart. Inspect the original conversation and durable records before reviewing a new run; do not erase records to bypass a halt. A CLI receipt does not prove project completion.

The controller reads complete recent replies and required UTF-8 artifacts, including local Markdown links inside the target's allowed paths. Images, PDFs, web pages and app internals require separately reviewed input. Defaults: an 8MiB transcript tail, 64KiB per artifact, up to 16 artifacts and 160,000 context bytes; oversized input stops rather than being truncated. Optional keys are `recent_messages` (6), `max_transcript_bytes`, `max_artifact_bytes`, `max_context_bytes`, `poll_seconds` (60), `judge_timeout_seconds` (90, maximum 300) and `command_timeout_seconds` (15). `shifu_bin`, `codex_bin` and `transcripts_dir` can override executable locations and the transcript directory.

File metadata is checked every 0.5 seconds; idle unchanged input does not repeatedly invoke CLIs/models or append logs. Two targets progress independently. New replies reuse complete unchanged artifacts, invalidating on path/device/inode/size/mtime/ctime changes and dropping obsolete delivery links. Restart rereads files. Identity, state, human input and evidence are still checked before dispatch.

Status metrics `artifact_reads`, `artifact_cache_hits` and `artifact_bytes_read` measure actual file I/O. They do not imply smaller model prompts, faster inference or billing savings. `judgment_seconds` includes time spent waiting before human takeover, stop or expiry cancels a judgment. These entries are marked `cancelled`; unreported token usage remains null and no cost is inferred. Reported cached input tokens are a subset of input tokens; missing usage is null. State and logs contain conversation evidence and messages: keep them private (0600 files, 0700 new directories). Necessary input goes to the current Codex model service. The script does not change login, model, global configuration or application installation.

Optional `supervisor` pins the original controller with the same identity and reviewed-authorization fields as a target. Blocked/completed reports wait for that controller to become available, then notify it once. Unknown receipts remain blocked; new human input preserves takeover protection. Optional per-target `read_paths` permits read-only artifact directories without broadening edits. Images/PDFs expose metadata, not reviewed content. Status distinguishes process residency from activity; completed/blocked runs exit after notification handling instead of idling until their deadline.

### Queued input and outcome progress (0.3.7 preview)

These additions are included in the matching app, CLI and script in the [0.3.7 preview package](https://github.com/Nalu-Lab/shifu/releases/tag/remote-v0.3.7-preview.1). Packages 0.3.6 and earlier do not include them. Installation does not start a run or change an active task goal.

The queue commands above pin the original Codex/Claude session, directory and pane. Ordinary `tell` retains its idle-only behavior. The app waits for a recognized empty native composer (`›` for Codex, `❯` for Claude), preserving drafts and approval prompts. Unrecognized versions/screens wait rather than forcing input. Stable message IDs make identical retries idempotent; conflicting content is rejected. Receipt phases distinguish queued, delivering, written and native received. Written bytes do not prove receipt, and receipt does not prove task completion. Ambiguous/interrupted sends become unknown and are never automatically resent. Only unsent queued entries can be cancelled. Optional absolute `--stop-file` and Unix-seconds `--expires-at` prevent pending submissions, without retracting input already submitted.

With an existing pinned supervisor, optional `queue_supervisor: true` also queues outcome-stall notices while that original controller is working. The default remains false. Stop/expiry blocks pending notices; they are not marked delivered until native acceptance is confirmed.

Optional per-agent `progress` uses the path/target/stall threshold and report format above. The owner reviews each actual outcome and supplies a stable ID, immutable receipt path and its real SHA256. Reports and receipts must lie inside reviewed edit/read paths and fit the artifact budget. Counts alone are rejected. The same result ID or receipt bytes cannot earn credit in another batch; relabeling/copying old receipts cannot refresh the progress clock. Missing, changed or duplicate proof is invalid, not a healthy zero. File/hash checks do not independently verify business success with an external service; that remains an owner/controller responsibility.

The existing Task displays actual batch outcomes (for example, 1/3), asks for adjustment after the configured stall interval and retains the shortfall at cutoff. Running/idle state, replies, file timestamps and empty batch changes are not new outcomes. Stalled notices do not end an active run, and a completion claim below the receipt target cannot end it early. Omit progress to retain the original workflow. Recurring batches still require the original controller to act within the reviewed authorization; targets are not automatically reset.
