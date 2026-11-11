//! `shifu hook <kind> <事件>`：agent 的 hook 回调。永远 exit 0，永远不写 stdout/stderr。

use crate::agent::{self, Agent, Res, now_secs};
use crate::status::{Status, next_status};
use crate::tmux::{find_tmux_bin, run_command};
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

const LAST_MESSAGE_CHARS: usize = 300;

/// hook 送来的 JSON 里用得到的字段。
#[derive(Debug, Default, PartialEq)]
pub struct HookInput {
    pub tool_name: Option<String>,
    pub session_id: Option<String>,
    pub last_message: Option<String>,
    /// The event came from a sub-agent's tool (Claude marks those with `agent_id`), not from the agent itself.
    /// Kimi Code sends `agent_id: "main"` for the agent itself.
    pub from_sub_agent: bool,
}

pub fn parse_input(raw: &str) -> HookInput {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return HookInput::default();
    };
    let field = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    HookInput {
        tool_name: field("tool_name"),
        session_id: field("session_id"),
        last_message: field("last_assistant_message"),
        from_sub_agent: field("agent_id").is_some_and(|id| id != "main"),
    }
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
    let status =
        if input.from_sub_agent { agent.status } else { next_status(agent.status, event, input.tool_name.as_deref()) };
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
    next
}

pub fn run(event: &str) {
    // 任何情况都不能把东西写到 stdout/stderr，panic 也一样
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(|| handle(event));
    let error = match outcome {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e,
        Err(_) => "hook 内部 panic".to_string(),
    };
    log_error(event, &error);
}

fn handle(event: &str) -> Res<()> {
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
    let Some(agent) = agent::load(&name)? else {
        return Ok(());
    };
    let updated = apply_event(&agent, event, &input, now_secs());
    agent::save(&updated)?;
    if updated.status != agent.status {
        announce_status(updated.status)?;
    }
    Ok(())
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
