//! `shifu hook <kind> <事件>`：agent 的 hook 回调。永远 exit 0，不写 stderr。
//! 默认 stdout 静默；三家新会话的首次启动可收到一次指南入口。

use crate::agent::{self, Agent, Res, now_secs};
use crate::status::{Status, asks_user, next_status};
use crate::tmux::{find_tmux_bin, run_command};
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

const LAST_MESSAGE_CHARS: usize = 300;
const WAITING_FOR_CHARS: usize = 200;

/// hook 送来的 JSON 里用得到的字段。
#[derive(Debug, Default, PartialEq)]
pub struct HookInput {
    pub tool_name: Option<String>,
    pub session_id: Option<String>,
    /// Why a session starts: Claude says `startup`, `resume`, `clear` or `compact`.
    pub source: Option<String>,
    pub last_message: Option<String>,
    pub prompt: Option<String>,
    /// What the tool call is about: the question it asks the user, else its command or the file it touches.
    pub tool_detail: Option<String>,
    /// The event came from a sub-agent's tool (Claude marks those with `agent_id`), not from the agent itself.
    /// Kimi Code sends `agent_id: "main"` for the agent itself.
    pub from_sub_agent: bool,
    /// Antigravity can emit Stop while its background tasks are still running.
    pub background_work: bool,
}

pub fn parse_input(raw: &str) -> HookInput {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return HookInput::default();
    };
    let field = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    HookInput {
        tool_name: field("tool_name").or_else(|| v["toolCall"]["name"].as_str().map(str::to_string)),
        session_id: field("session_id").or_else(|| field("conversationId")),
        source: field("source"),
        last_message: field("last_assistant_message"),
        prompt: field("prompt"),
        tool_detail: tool_detail(&v["tool_input"]),
        from_sub_agent: field("agent_id").is_some_and(|id| id != "main"),
        background_work: v.get("fullyIdle").and_then(Value::as_bool) == Some(false),
    }
}

fn tool_detail(input: &Value) -> Option<String> {
    let questions = input["questions"].as_array().map_or(0, Vec::len);
    let text = if let Some(question) = input["questions"][0]["question"].as_str().or_else(|| input["question"].as_str()) {
        if questions > 1 { format!("共 {questions} 问：{}", question.trim()) } else { question.trim().to_string() }
    } else if let Some(command) = input["command"].as_str() {
        command.trim().lines().next().unwrap_or_default().trim().to_string()
    } else if let Some(words) = input["command"].as_array() {
        words.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")
    } else {
        let path = input["file_path"].as_str()?;
        std::path::Path::new(path).file_name()?.to_str()?.to_string()
    };
    (!text.is_empty()).then(|| text.chars().take(WAITING_FOR_CHARS).collect())
}

/// A line that points at something instead of saying something: a Codex directive (`::codex-file-citation{…}`), a
/// lone link, URL or path, or the label above such a list ("Sources:"). Replies often end on these; what the agent
/// wants is said before them.
fn points_elsewhere(line: &str) -> bool {
    let lone = !line.contains(char::is_whitespace);
    let directive = line.starts_with(':') && line.ends_with('}') && line.trim_start_matches(':').starts_with(|c: char| c.is_ascii_alphabetic());
    let link = line.starts_with('[') && line.ends_with(')') && line.matches("](").count() == 1;
    let address = lone && ["http://", "https://", "/", "~/"].iter().any(|start| line.starts_with(start));
    directive || link || address || line.ends_with([':', '：'])
}

/// The reply's last line that says something, without its Markdown marks: where an agent says what it wants next.
/// Commands and output inside code fences are not it. A long line keeps its end, since that is where the question is.
fn closing_line(message: &str) -> Option<String> {
    let mut fenced = false;
    let prose: Vec<&str> = message
        .lines()
        .filter(|line| {
            let fence = line.trim_start().starts_with("```");
            fenced ^= fence;
            !fence && !fenced
        })
        .collect();
    prose.into_iter().rev().find_map(|line| {
        let line = line.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '>' | '#' | '-' | '*' | '+'));
        let numbered = line.trim_start_matches(|c: char| c.is_ascii_digit());
        let line = match numbered.strip_prefix(['.', ')', '、']) {
            Some(rest) if numbered.len() < line.len() && (rest.starts_with(char::is_whitespace) || numbered.starts_with('、')) => rest,
            _ => line,
        };
        let plain = line.replace("**", "").replace("__", "").replace('`', "");
        let plain = plain.trim();
        if plain.is_empty() || points_elsewhere(plain) {
            return None;
        }
        let chars: Vec<char> = plain.chars().collect();
        Some(chars[chars.len().saturating_sub(WAITING_FOR_CHARS)..].iter().collect())
    })
}

/// 事件作用到 agent 上，返回新值。状态变了才更新 `status_since`。
pub fn apply_event(agent: &Agent, event: &str, input: &HookInput, now: u64) -> Agent {
    // A sub-agent working in the background says nothing about whether the agent itself waits for you.
    // Kimi Code's /new ends the old session after the new one started: that end is not the agent's.
    let old_session_ended = event == "SessionEnd"
        && matches!((&input.session_id, &agent.session_id), (Some(ended), Some(current)) if ended != current);
    if old_session_ended {
        return agent.clone();
    }
    let status = if input.from_sub_agent { agent.status }
        else if event == "Stop" && input.background_work { Status::Running }
        else { next_status(agent.status, event, input.tool_name.as_deref()) };
    let mut next = agent.clone();
    if status != agent.status {
        next.status = status;
        next.status_since = now;
    }
    if input.session_id.is_some() {
        next.session_id = input.session_id.clone();
    }
    if event == "Stop"
        && let Some(msg) = &input.last_message
    {
        next.last_message = Some(msg.chars().take(LAST_MESSAGE_CHARS).collect());
    }
    let tool = input.tool_name.as_deref();
    next.waiting_for = if input.from_sub_agent {
        agent.waiting_for.clone()
    } else if !status.is_waiting() {
        None
    } else if event == "PermissionRequest" {
        match (tool, &input.tool_detail) {
            (Some(tool), Some(detail)) => Some(format!("{tool}：{detail}")),
            (tool, detail) => tool.map(str::to_string).or_else(|| detail.clone()),
        }
    } else if event == "PreToolUse" && asks_user(tool) {
        // An explicit question tool can itself quote the question. Preserve its
        // request context when only this one-line detail reaches the sidebar.
        input.tool_detail.as_ref().map(|detail| {
            if detail.starts_with(['“', '「', '『', '‘', '"'])
                || (detail.starts_with("共 ") && detail.split_once('：').is_some_and(|(_, question)| question.starts_with(['“', '「', '『', '‘', '"'])))
            {
                format!("请回答{detail}")
            } else {
                detail.clone()
            }
        })
    } else if event == "Stop" {
        input.last_message.as_deref().and_then(closing_line)
    } else if status != agent.status {
        None
    } else {
        agent.waiting_for.clone()
    };
    next
}

pub fn run(kind: &str, event: &str) {
    // 错误只记本地日志；不能污染 CLI 的 hook 协议输出。
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(|| handle(kind, event));
    let error = match outcome {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e,
        Err(_) => "hook 内部 panic".to_string(),
    };
    log_error(event, &error);
}

fn handle(kind: &str, event: &str) -> Res<()> {
    let Some(name) = std::env::var("SHIFU_ID").ok().filter(|n| !n.is_empty()) else {
        return Ok(());
    };
    if !agent::is_safe_name(&name) {
        return Err(format!("SHIFU_ID 不是合法名字: {name:?}"));
    }
    let mut raw = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut raw) {
        log_error(event, &format!("读 stdin 失败: {e}"));
    }
    let input = parse_input(&raw);
    let lock = agent::lock()?;
    let Some(agent) = agent::load(&name)? else {
        return Ok(());
    };
    // An agy subprocess launched by another CLI must not overwrite that parent's conversation ID/status.
    if kind == "antigravity" && agent.kind != crate::agent::Kind::Antigravity { return Ok(()); }
    if kind == "stepfun" && agent.kind != crate::agent::Kind::StepFun { return Ok(()); }
    if kind == "qwen" && (agent.kind != crate::agent::Kind::Qwen || input.from_sub_agent) { return Ok(()); }
    let updated = apply_event(&agent, event, &input, now_secs());
    agent::save(&updated)?;
    drop(lock);
    if event == "UserPromptSubmit" && !input.from_sub_agent {
        if let Some(prompt) = input.prompt.as_deref() {
            crate::inbox::acknowledge(&agent::shifu_home(), &updated, prompt)?;
        }
    }
    if updated.status != agent.status {
        announce_status(updated.status)?;
    }
    if let Some(context) = guide_context(kind, event, &name, &agent, &input)? {
        writeln!(std::io::stdout().lock(), "{context}").map_err(|e| format!("写指南上下文失败: {e}"))?;
    }
    Ok(())
}

fn guide_context(kind: &str, event: &str, name: &str, registered: &Agent, input: &HookInput) -> Res<Option<String>> {
    if !matches!(kind, "claude" | "codex" | "kimi")
        || kind != registered.kind.as_str()
        || input.from_sub_agent
    {
        return Ok(None);
    }
    let Some(session) = input.session_id.as_deref().filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let home = agent::shifu_home();
    // A brand-new conversation gets the computer guide and its task. A started one gets only its task again after
    // /clear, compaction or resume, which drop what it was told; any other start under its name (a CLI run inside
    // the conversation) gets nothing.
    let fresh = registered.session_id.is_none();
    let restarted = matches!(input.source.as_deref(), Some("clear" | "compact" | "resume"));
    if event == "SessionStart" && !fresh && !(restarted && kind != "kimi") {
        return Ok(None);
    }
    if event != "SessionStart" && registered.session_id.as_deref() != Some(session) {
        return Ok(None);
    }
    let pending = home.join("computer-guide-pending").join(name);
    if kind == "kimi" {
        // Kimi ignores SessionStart output; its first prompt hook accepts plain context.
        if event == "SessionStart" {
            std::fs::create_dir_all(pending.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&pending, session).map_err(|e| e.to_string())?;
            return Ok(None);
        }
        if event != "UserPromptSubmit" { return Ok(None); }
        match std::fs::read_to_string(&pending) {
            Ok(expected) if expected == session => std::fs::remove_file(&pending).map_err(|e| e.to_string())?,
            Ok(_) => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        }
    } else if event != "SessionStart" {
        return Ok(None);
    }
    let task = task_note(&home, name).unwrap_or_else(|e| {
        log_error(event, &e);
        None
    });
    let guide = if fresh || kind == "kimi" { Some(computer_note()?) } else { None };
    let context = match (guide, task) {
        (Some(g), Some(t)) => format!("{g}\n\n{t}"),
        (Some(only), None) | (None, Some(only)) => only,
        (None, None) => return Ok(None),
    };
    if kind == "kimi" { return Ok(Some(context)); }
    Ok(Some(serde_json::json!({"hookSpecificOutput": {
        "hookEventName":"SessionStart", "additionalContext":context
    }}).to_string()))
}

fn computer_note() -> Res<String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let executable = executable.to_str().ok_or("Shifu 程序路径不是 UTF-8")?;
    let command = format!("{} computer guide", crate::launch::sh_quote(executable));
    let browser = format!("{} browser guide", crate::launch::sh_quote(executable));
    Ok(format!(
        "优先用已有 API/CLI；网页先读 {browser}，本机窗口先读 {command}。Shifu 自带后台截图、点击、输入和滚动，不依赖 Orca；普通后台保持鼠标位置，应用仍可能改变焦点或弹窗。--background-only 仅允许读取和文本框赋值，不得去掉限制重试。截图路径须打开查看；不扩大前台接管、发送或交易授权。"
    ))
}

#[derive(serde::Deserialize)]
struct TaskList {
    #[serde(default)]
    tasks: Vec<TaskEntry>,
}

#[derive(serde::Deserialize)]
struct TaskEntry {
    id: String,
    name: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    agents: Vec<String>,
}

/// The task this conversation belongs to (the app's `tasks.json`) and what to do with the task's goal file
/// (`goals/<id>.md`, the app's `taskgoals.rs`). Told at every start, so a conversation never works without it.
fn task_note(home: &std::path::Path, name: &str) -> Res<Option<String>> {
    let Some(text) = crate::app_session::read_optional_text(&home.join("tasks.json"))
        .map_err(|e| format!("读 tasks.json 失败: {e}"))? else { return Ok(None) };
    let list: TaskList = serde_json::from_str(&text).map_err(|e| format!("tasks.json 解析失败: {e}"))?;
    let Some(task) = list.tasks.into_iter().find(|t| t.agents.iter().any(|a| a == name)) else {
        return Ok(None);
    };
    // 项目经理 and 每日分析 have their own instructions in their folders and no goal file; the progress page leaves
    // their tasks out too.
    if ["pm", "daily"].iter().any(|role| std::path::Path::new(&task.project) == home.join(role)) {
        return Ok(None);
    }
    let file = home.join("goals").join(format!("{}.md", task.id));
    let (task_name, path) = (task.name, file.display());
    let goal = match crate::app_session::read_optional_text(&file) {
        Ok(text) => text.unwrap_or_default(),
        Err(e) => return Ok(Some(format!(
            "你在 Shifu 里属于任务「{task_name}」，你的名字是 `{name}`。任务文件 {path} 暂时无法读取（{e}）。保留原文件，继续处理用户当前交代的工作。"
        ))),
    };
    Ok(Some(if written(&goal) {
        format!(
            "你在 Shifu 里属于任务「{task_name}」，你的名字是 `{name}`。这个任务的终点、重点和清单在 {path}，动手前先读。做完清单里的一步，把那一行的 `- [ ]` 改成 `- [x]`；行末标了别的 `@名字` 的不要做，没标的等我交给你再做。别的对话需要知道的发现和决定，在「# 记录」下面追加一行 `- 日期 内容`。要改终点或重点，先跟我说。"
        )
    } else {
        format!(
            "你在 Shifu 里属于任务「{task_name}」，你的名字是 `{name}`。这个任务的终点、重点和清单还没写，文件是 {path}（可能还不存在）。等你从对话里弄清这个任务要做成什么样，就起草它：「# 终点」一两句写做完是什么样子，「# 重点」写现在最该推的一件事，「# 清单」每步一行 `- [ ] `，「# 记录」先留空。起草前再读一次这个文件，别的对话已经写了就照它做，不要覆盖。写完告诉我，我来确认；不要为了起草打断我交代的事。"
        )
    }))
}

/// Same rule as the app's `taskgoals::written`: a goal file counts once it says more than its headings.
fn written(text: &str) -> bool {
    text.lines().map(str::trim).any(|l| !l.is_empty() && !l.starts_with('#'))
}

/// 状态真的变了才写窗格选项 `@shifu_status`，标签栏靠它画圆点。
/// 用环境里 `TMUX` 指的那个服务（`-S <socket 路径>`）；没有 TMUX 或 TMUX_PANE 就跳过。
/// 这是 hook 唯一会调的 tmux 命令。
fn announce_status(status: Status) -> Res<()> {
    let var = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let (Some(tmux_var), Some(pane)) = (var("TMUX"), var("TMUX_PANE")) else {
        return Ok(());
    };
    let socket = tmux_var.split(',').next().unwrap_or_default();
    let mut cmd = Command::new(find_tmux_bin());
    cmd.args(["-S", socket, "set-option", "-p", "-t", &pane, "@shifu_status", status.name()]).stdin(Stdio::null());
    run_command(cmd).map(|_| ())
}

fn log_error(event: &str, message: &str) {
    let path = agent::shifu_home().join("hook-errors.log");
    let _ = std::fs::create_dir_all(agent::shifu_home());
    // 写日志本身失败时没有别的地方可报，只能放弃
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {event}: {}", now_secs(), message.replace('\n', " "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Kind;
    use crate::status::Status;

    fn agent(status: Status) -> Agent {
        Agent {
            name: "claude-p".to_string(),
            kind: Kind::Claude,
            command: "claude".to_string(),
            cwd: "/tmp".to_string(),
            created_at: 100,
            status,
            status_since: 100,
            window_id: Some("@1".to_string()),
            pane_id: Some("%1".to_string()),
            session_id: None,
            last_message: None,
            waiting_for: None,
            model: None,
            effort: None,
            label: None,
        }
    }

    #[test]
    fn parse_input_reads_fields() {
        let i = parse_input(r#"{"tool_name":"Bash","session_id":"abc","last_assistant_message":"好了","x":1}"#);
        assert_eq!(i.tool_name.as_deref(), Some("Bash"));
        assert_eq!(i.session_id.as_deref(), Some("abc"));
        assert_eq!(i.last_message.as_deref(), Some("好了"));
        assert_eq!(parse_input(""), HookInput::default());
        assert_eq!(parse_input("not json"), HookInput::default());
        assert_eq!(parse_input(r#"{"tool_name":5}"#), HookInput::default());
    }

    #[test]
    fn since_changes_only_with_status() {
        let a = agent(Status::Running);
        let same = apply_event(&a, "PreToolUse", &HookInput::default(), 500);
        assert_eq!(same.status_since, 100);
        let changed = apply_event(&a, "Stop", &HookInput::default(), 500);
        assert_eq!(changed.status, Status::NeedsReply);
        assert_eq!(changed.status_since, 500);
        assert_eq!(a.status_since, 100, "原值不被改");
    }

    #[test]
    fn stop_records_last_message_truncated_to_300_chars() {
        let long: String = "字".repeat(400);
        let input = HookInput { last_message: Some(long), ..Default::default() };
        let out = apply_event(&agent(Status::Running), "Stop", &input, 5);
        assert_eq!(out.last_message.as_ref().unwrap().chars().count(), 300);
        let other = apply_event(&agent(Status::Running), "UserPromptSubmit", &input, 5);
        assert_eq!(other.last_message, None);
    }

    #[test]
    fn a_waiting_agent_records_what_it_waits_for() {
        let asks = parse_input(r#"{"tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"用哪个数据库？"},{"question":"要不要迁移？"}]}}"#);
        let asked = apply_event(&agent(Status::Running), "PreToolUse", &asks, 5);
        assert_eq!(asked.waiting_for.as_deref(), Some("共 2 问：用哪个数据库？"));
        let answered = apply_event(&asked, "PostToolUse", &asks, 6);
        assert_eq!((answered.status, answered.waiting_for), (Status::Running, None));

        let shell = parse_input(r#"{"tool_name":"Bash","tool_input":{"command":"npm publish \\\n  --tag next"}}"#);
        let approval = apply_event(&agent(Status::Running), "PermissionRequest", &shell, 5);
        assert_eq!(approval.waiting_for.as_deref(), Some("Bash：npm publish \\"));
        let edit = parse_input(r#"{"tool_name":"Edit","tool_input":{"file_path":"/repo/app/config.toml"}}"#);
        assert_eq!(apply_event(&agent(Status::Running), "PermissionRequest", &edit, 5).waiting_for.as_deref(), Some("Edit：config.toml"));
        let codex = parse_input(r#"{"tool_name":"shell","tool_input":{"command":["bash","-lc","git push"]}}"#);
        assert_eq!(apply_event(&agent(Status::Running), "PermissionRequest", &codex, 5).waiting_for.as_deref(), Some("shell：bash -lc git push"));
        let bare = parse_input(r#"{"tool_name":"Bash"}"#);
        assert_eq!(apply_event(&agent(Status::Running), "PermissionRequest", &bare, 5).waiting_for.as_deref(), Some("Bash"));
        assert_eq!(apply_event(&approval, "PostToolUse", &shell, 6).waiting_for, None);
    }

    #[test]
    fn quoted_examples_and_explicit_question_tools_keep_their_context() {
        let reply = format!("{}\n可以这样问管理员：\n“设备 A 的使用编号是多少？”", "说明。".repeat(200));
        let input = HookInput { last_message: Some(reply), ..HookInput::default() };
        let stopped = apply_event(&agent(Status::Running), "Stop", &input, 5);
        assert_eq!(stopped.status, Status::NeedsReply);
        assert_eq!(stopped.waiting_for.as_deref(), Some("“设备 A 的使用编号是多少？”"));
        let input = HookInput { tool_name: Some("AskUserQuestion".into()), tool_detail: Some("“设备 A 的编号是多少？”".into()), ..HookInput::default() };
        let asked = apply_event(&agent(Status::Running), "PreToolUse", &input, 5);
        assert_eq!(asked.status, Status::NeedsReply);
        assert_eq!(asked.waiting_for.as_deref(), Some("请回答“设备 A 的编号是多少？”"));
        let input = HookInput { tool_detail: Some("共 2 问：“编号是多少？”".into()), ..input };
        let asked = apply_event(&agent(Status::Running), "PreToolUse", &input, 5);
        assert_eq!(asked.waiting_for.as_deref(), Some("请回答共 2 问：“编号是多少？”"));
    }

    #[test]
    fn a_finished_turn_keeps_its_closing_line_even_when_the_reply_is_long() {
        let reply = format!("{}\n\n- 改好了三处\n\n**要我现在合并吗？**\n", "前文。".repeat(200));
        let input = HookInput { last_message: Some(reply), ..Default::default() };
        let stopped = apply_event(&agent(Status::Running), "Stop", &input, 5);
        assert_eq!(stopped.waiting_for.as_deref(), Some("要我现在合并吗？"));
        let list = HookInput { last_message: Some("做完了：\n1. 第一步\n> - `cargo test` 通过".into()), ..Default::default() };
        assert_eq!(apply_event(&agent(Status::Running), "Stop", &list, 5).waiting_for.as_deref(), Some("cargo test 通过"));
        let long = HookInput { last_message: Some(format!("{}结尾？", "长".repeat(400))), ..Default::default() };
        let kept = apply_event(&agent(Status::Running), "Stop", &long, 5).waiting_for.unwrap();
        assert!(kept.chars().count() == 200 && kept.ends_with("结尾？"), "{kept}");
        let fenced = HookInput { last_message: Some("装好了，重启后生效。\n\n要看的话跑这条：\n```sh\nopen -a Preview '/tmp/a.pdf'\n```\n---\n".into()), ..Default::default() };
        assert_eq!(apply_event(&agent(Status::Running), "Stop", &fenced, 5).waiting_for.as_deref(), Some("装好了，重启后生效。"));
        // Replies often end on citations, sources or file paths; the closing line is the sentence before them.
        let closing = |reply: &str| {
            let input = HookInput { last_message: Some(reply.into()), ..Default::default() };
            apply_event(&agent(Status::Running), "Stop", &input, 5).waiting_for
        };
        assert_eq!(
            closing("表格已更新，当前 4/5 位。\n\n::codex-file-citation{path=\"/Users/example/out/进度.xlsx\" artifact_kind=\"workbook\"}").as_deref(),
            Some("表格已更新，当前 4/5 位。")
        );
        assert_eq!(
            closing("数据都核过了，要继续写第二节吗？\n\nSources:\n- [月活（某站）](https://example.com/a)\n- https://example.com/b").as_deref(),
            Some("数据都核过了，要继续写第二节吗？")
        );
        assert_eq!(closing("对照写好了，先看结论一节。\n\n文件在这里：\n/Users/example/项目/对照与借鉴.md\n~/notes/b.md").as_deref(), Some("对照写好了，先看结论一节。"));
        assert_eq!(closing("见 [设计稿](https://example.com/d) 和 /tmp/a.png 两处").as_deref(), Some("见 [设计稿](https://example.com/d) 和 /tmp/a.png 两处"));
        assert_eq!(closing("来源：\n- https://example.com/a"), None);
        let version = HookInput { last_message: Some("3.2 版已发布".into()), ..Default::default() };
        assert_eq!(apply_event(&agent(Status::Running), "Stop", &version, 5).waiting_for.as_deref(), Some("3.2 版已发布"));
        // The next prompt, a sub-agent's events and background work leave nothing stale behind.
        assert_eq!(apply_event(&stopped, "UserPromptSubmit", &HookInput::default(), 6).waiting_for, None);
        let sub = parse_input(r#"{"tool_name":"Bash","agent_id":"a80d08fb6413ebdaf"}"#);
        assert_eq!(apply_event(&stopped, "PostToolUse", &sub, 6).waiting_for.as_deref(), Some("要我现在合并吗？"));
        let busy = HookInput { last_message: Some("还在跑".into()), background_work: true, ..Default::default() };
        assert_eq!(apply_event(&agent(Status::Running), "Stop", &busy, 5).waiting_for, None);
        assert_eq!(apply_event(&agent(Status::Running), "Stop", &HookInput::default(), 5).waiting_for, None);
    }

    #[test]
    fn a_sub_agents_tools_do_not_wake_a_session_that_waits() {
        let sub = parse_input(r#"{"tool_name":"Bash","agent_id":"a80d08fb6413ebdaf","agent_type":"general-purpose"}"#);
        assert_eq!(apply_event(&agent(Status::NeedsReply), "PostToolUse", &sub, 5).status, Status::NeedsReply);
        let own = parse_input(r#"{"tool_name":"Bash"}"#);
        assert_eq!(apply_event(&agent(Status::NeedsReply), "PostToolUse", &own, 5).status, Status::Running);
        // Kimi Code names its own agent "main" when it stops to ask (11-09: Shifu kept showing running).
        let kimi_asks = parse_input(r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash","agent_id":"main"}"#);
        assert_eq!(apply_event(&agent(Status::Running), "PermissionRequest", &kimi_asks, 5).status, Status::NeedsApproval);
    }

    #[test]
    fn an_old_session_ending_after_a_new_one_started_is_ignored() {
        // Kimi Code's /new: SessionStart for the new session, then SessionEnd for the old one (11-09).
        let started = apply_event(&agent(Status::Idle), "SessionStart", &HookInput { session_id: Some("new".into()), ..Default::default() }, 5);
        let old_end = HookInput { session_id: Some("old".into()), ..Default::default() };
        let after = apply_event(&started, "SessionEnd", &old_end, 6);
        assert_eq!((after.status, after.session_id.as_deref()), (Status::Idle, Some("new")));
        let own_end = HookInput { session_id: Some("new".into()), ..Default::default() };
        assert_eq!(apply_event(&started, "SessionEnd", &own_end, 7).status, Status::Exited);
        assert_eq!(apply_event(&agent(Status::Idle), "SessionEnd", &old_end, 7).status, Status::Exited);
    }

    #[test]
    fn activity_recovers_a_false_exit_but_a_dead_pane_still_shows_exited() {
        use crate::agent::PaneInfo;
        use std::collections::HashMap;
        let mut original = agent(Status::Running);
        original.session_id = Some("same-conversation".into());
        original.label = Some("双人 IM".into());
        let ended = apply_event(&original, "SessionEnd", &HookInput::default(), 200);
        let own = parse_input(r#"{"tool_name":"exec_command","session_id":"same-conversation"}"#);
        let active = apply_event(&ended, "PostToolUse", &own, 300);
        assert_eq!(active.status, Status::Running);
        assert_eq!(active.status_since, 300);
        assert_eq!(active.session_id, original.session_id);
        assert_eq!(active.label, original.label);
        let live = HashMap::from([("%1".into(), PaneInfo { dead: false, window_id: "@1".into(), index: 0 })]);
        assert_eq!(active.effective_status(&live), Status::Running);
        let dead = HashMap::from([("%1".into(), PaneInfo { dead: true, window_id: "@1".into(), index: 0 })]);
        assert_eq!(active.effective_status(&dead), Status::Exited);
        assert_eq!(active.effective_status(&HashMap::new()), Status::Exited);
        let sub = parse_input(r#"{"tool_name":"Bash","agent_id":"child"}"#);
        assert_eq!(apply_event(&ended, "PostToolUse", &sub, 300).status, Status::Exited);
        assert_eq!(apply_event(&active, "Stop", &HookInput::default(), 400).status, Status::NeedsReply);
    }

    #[test]
    fn session_id_recorded() {
        let input = HookInput { session_id: Some("s1".to_string()), ..Default::default() };
        assert_eq!(apply_event(&agent(Status::Starting), "SessionStart", &input, 5).session_id.as_deref(), Some("s1"));
    }
}
