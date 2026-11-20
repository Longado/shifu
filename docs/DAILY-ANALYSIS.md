# 每日分析的自动触发

侧栏先打开「每日分析」并选择 Agent。它应已完成登录和目录信任确认。自动触发使用现有会话，不创建新会话，也不处理批准窗口。

`shifu status` / `shifu tell` 能识别新 Codex 已停在空白欢迎输入框的情况；首次输入前没有 SessionStart 事件也可接收第一条指令。识别仅适用于已观察到的英文初始界面，其他界面继续保留“启动中”。桌面侧栏仍由原有状态事件更新。

定时入口是 `scripts/daily-analysis.py`，只依赖 Python 标准库。报告职责与数据源由每日分析目录（`~/.shifu/app/daily`）的 CLAUDE.md、AGENTS.md 配置，可以直接改：改过的文件 Shifu 不再覆盖，删掉它下次启动恢复默认；项目经理（`~/.shifu/app/pm`）相同。脚本只安排执行。

```sh
python3 scripts/daily-analysis.py \
  --reports /absolute/path/to/daily-reports \
  --first-day 2026-11-14 --dry-run
```

- 每天台北时间 09:10 起分析前一天；之前仍可补更早的日期。
- 从 `--first-day` 开始检查缺失的 `YYYY-MM-DD.md`，按日期顺序每次发送一天。空文件不算报告。
- 一旦有同目录、同任务的会话正在执行，就等待下一次检查，不向另一个会话重复派发。
- 未就绪、需要确认、会话不存在时记录原因并等待，不自动新建或恢复会话。
- 指令成功发送只算已投递；会话停下却没有报告时，至少间隔 30 分钟重试，每个日期最多三次。失败日期不会永远挡住后面的日期。
- 发送结果不明时暂停，先查看原会话和 `~/.shifu/daily-analysis-state.json`；不要直接重发。

用 launchd 每 5 分钟调用一次；保留 `StartCalendarInterval` 的 09:10 触发，并加 `StartInterval=300` 与 `RunAtLoad=true`。`ProgramArguments` 应使用 Python、脚本、报告目录的绝对路径，并同时提供 `--reports` 和 `--first-day`。先用同一 launchd 环境的 `--dry-run` 确认它可以读取报告目录；macOS 的桌面/文稿权限可能与交互终端不同。日志可沿用 `~/.shifu/daily-analysis-kick.log`。定时配置中不要加 `--through`。

一次性提前补到昨天时可加 `--through YYYY-MM-DD`；不允许选择今天或未来。`--dry-run` 只读状态，不发消息、不写状态或通知。

## English

Open **每日分析** in the sidebar once and select an agent. Complete its login and folder trust steps. The scheduler uses that existing conversation; it does not create or resume sessions or answer approval prompts.

The CLI recognizes a fresh Codex instance at its observed empty English welcome composer, even before its first SessionStart event. Unknown startup screens remain blocked. Desktop status continues to follow the existing hooks. The role's duties live in `~/.shifu/app/daily/CLAUDE.md` and `AGENTS.md` (the project manager's in `~/.shifu/app/pm`); edit them freely: Shifu leaves an edited file alone, and deleting it restores the default at the next start.

Run `scripts/daily-analysis.py` with an absolute `--reports` directory and `--first-day`. Configure launchd for 09:10 plus a 300-second interval and `RunAtLoad`. Verify access with `--dry-run` under launchd itself. Yesterday becomes due at 09:10 Asia/Taipei; older gaps are eligible earlier. The script sends one missing date at a time, waits while a matching conversation is running, and retries missing reports at least 30 minutes apart, up to three deliveries per date. Uncertain delivery is held for inspection. Nonempty dated report files record production of an artifact, not review or acceptance of its contents.

Use `--through YYYY-MM-DD` only for a manual catch-up through a past date. Keep it out of the recurring schedule. State defaults to `~/.shifu/daily-analysis-state.json`; no report content is stored there.
