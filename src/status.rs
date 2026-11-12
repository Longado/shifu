//! agent 状态：转换规则、排序分组、显示文字。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Starting,
    Idle,
    Running,
    NeedsApproval,
    NeedsReply,
    Exited,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Starting => "启动中",
            Status::Idle => "空闲",
            Status::Running => "运行中",
            Status::NeedsApproval => "等你批准",
            Status::NeedsReply => "等你回复",
            Status::Exited => "已退出",
        }
    }

    /// 内部值的名字（和 JSON、窗格选项 `@shifu_status` 里的写法一致）。
    pub fn name(self) -> &'static str {
        match self {
            Status::Starting => "starting",
            Status::Idle => "idle",
            Status::Running => "running",
            Status::NeedsApproval => "needs_approval",
            Status::NeedsReply => "needs_reply",
            Status::Exited => "exited",
        }
    }

    /// 排序组：数字越小越靠上。
    pub fn rank(self) -> u8 {
        match self {
            Status::NeedsApproval => 0,
            Status::NeedsReply => 1,
            Status::Running => 2,
            Status::Starting | Status::Idle => 3,
            Status::Exited => 4,
        }
    }

    pub fn is_waiting(self) -> bool {
        matches!(self, Status::NeedsApproval | Status::NeedsReply)
    }
}

/// 状态转换：返回新状态，不变就返回 `current`。
pub fn next_status(current: Status, event: &str, tool_name: Option<&str>) -> Status {
    use Status::*;
    let asks_user = matches!(tool_name, Some("AskUserQuestion" | "ask_question"));
    match (current, event) {
        (Starting | Exited, "SessionStart") => Idle,
        (_, "UserPromptSubmit" | "PreInvocation") => Running,
        (_, "PermissionRequest") => NeedsApproval,
        (_, "PreToolUse") if tool_name == Some("ask_permission") => NeedsApproval,
        (_, "PreToolUse") if asks_user => NeedsReply,
        (NeedsApproval, "PostToolUse" | "PermissionResult") => Running,
        (_, "PostToolUse") if asks_user => Running,
        // Fresh activity can follow a stale SessionEnd too. A dead/missing pane still overrides this reported
        // status in Agent::effective_status; sub-agent events are filtered by hook::apply_event.
        (NeedsReply | Idle | Exited, "PreToolUse" | "PostToolUse") => Running,
        // StopFailure: Kimi's turn ended on an error (rate limit, network); it waits for you all the same.
        (_, "Stop" | "StopFailure" | "Interrupt") => NeedsReply,
        (_, "SessionEnd") => Exited,
        _ => current,
    }
}

/// 「刚刚」「N分」「N时」「N天」。
pub fn format_age(secs: u64) -> String {
    match secs {
        0..=59 => "刚刚".to_string(),
        60..=3599 => format!("{}分", secs / 60),
        3600..=86399 => format!("{}时", secs / 3600),
        _ => format!("{}天", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::Status::*;
    use super::*;

    /// 逐步走一段事件序列，每一步之后核对状态。
    fn walk(start: Status, steps: &[(&str, Option<&str>, Status)]) {
        let mut cur = start;
        for (i, (event, tool, want)) in steps.iter().enumerate() {
            cur = next_status(cur, event, *tool);
            assert_eq!(cur, *want, "第 {} 步 {} 之后", i + 1, event);
        }
    }

    #[test]
    fn single_rules() {
        assert_eq!(next_status(Starting, "SessionStart", None), Idle);
        assert_eq!(next_status(Running, "SessionStart", None), Running);
        assert_eq!(next_status(NeedsReply, "SessionStart", None), NeedsReply);
        assert_eq!(next_status(Idle, "UserPromptSubmit", None), Running);
        assert_eq!(next_status(Idle, "PermissionRequest", None), NeedsApproval);
        assert_eq!(next_status(Running, "PreToolUse", Some("AskUserQuestion")), NeedsReply);
        assert_eq!(next_status(Running, "PreToolUse", Some("Bash")), Running);
        assert_eq!(next_status(Idle, "PreToolUse", Some("Bash")), Running);
        assert_eq!(next_status(NeedsApproval, "PostToolUse", Some("Bash")), Running);
        // Kimi Code: approved or rejected, it carries on.
        assert_eq!(next_status(NeedsApproval, "PermissionResult", Some("Bash")), Running);
        assert_eq!(next_status(NeedsReply, "PermissionResult", Some("Bash")), NeedsReply);
        assert_eq!(next_status(NeedsReply, "PostToolUse", Some("AskUserQuestion")), Running);
        assert_eq!(next_status(Running, "Stop", None), NeedsReply);
        assert_eq!(next_status(Running, "Interrupt", None), NeedsReply);
        assert_eq!(next_status(NeedsReply, "Interrupt", None), NeedsReply);
        assert_eq!(next_status(Running, "SessionEnd", None), Exited);
        assert_eq!(next_status(Running, "Notification", None), Running);
        assert_eq!(next_status(NeedsApproval, "Whatever", None), NeedsApproval);
    }

    #[test]
    fn a_tool_starting_after_stop_means_it_is_working_again() {
        // Woken without a prompt (a background job finished, a message from another session): no UserPromptSubmit,
        // but its own tools still report. Sub-agents' tools never get here (see hook::apply_event).
        // Starting counts, not only finishing: a long build or a sub-agent run would otherwise show "waiting" throughout.
        assert_eq!(next_status(NeedsReply, "PreToolUse", Some("Bash")), Running);
        assert_eq!(next_status(NeedsApproval, "PreToolUse", Some("Bash")), NeedsApproval);
        assert_eq!(next_status(NeedsReply, "PostToolUse", Some("Bash")), Running);
        assert_eq!(next_status(Idle, "PostToolUse", Some("Read")), Running);
    }

    #[test]
    fn fresh_activity_after_session_end_recovers_the_reported_state() {
        for (event, tool, expected) in [
            ("UserPromptSubmit", None, Running),
            ("PreToolUse", Some("Bash"), Running),
            ("PostToolUse", Some("Read"), Running),
            ("PreToolUse", Some("AskUserQuestion"), NeedsReply),
            ("PermissionRequest", None, NeedsApproval),
            ("Stop", None, NeedsReply),
            ("Interrupt", None, NeedsReply),
            ("SessionStart", None, Idle),
        ] {
            assert_eq!(next_status(Exited, event, tool), expected, "{event}");
        }
        assert_eq!(next_status(Exited, "SessionEnd", None), Exited);
        assert_eq!(next_status(Exited, "Unknown", None), Exited);
    }

    #[test]
    fn claude_manual_sequence() {
        walk(
            Starting,
            &[
                ("SessionStart", None, Idle),
                ("UserPromptSubmit", None, Running),
                ("PreToolUse", Some("Bash"), Running),
                ("PermissionRequest", Some("Bash"), NeedsApproval),
                ("PostToolUse", Some("Bash"), Running),
                ("Stop", None, NeedsReply),
            ],
        );
    }

    #[test]
    fn claude_auto_sequence() {
        walk(
            Starting,
            &[
                ("UserPromptSubmit", None, Running),
                ("PreToolUse", Some("Read"), Running),
                ("PostToolUse", Some("Read"), Running),
                ("PreToolUse", Some("Edit"), Running),
                ("PostToolUse", Some("Edit"), Running),
                ("Stop", None, NeedsReply),
            ],
        );
    }

    #[test]
    fn codex_approval_sequence() {
        walk(
            Starting,
            &[
                ("SessionStart", None, Idle),
                ("UserPromptSubmit", None, Running),
                ("PreToolUse", Some("Bash"), Running),
                ("PermissionRequest", Some("Bash"), NeedsApproval),
                ("PostToolUse", Some("Bash"), Running),
                ("PreToolUse", Some("Bash"), Running),
                ("PostToolUse", Some("Bash"), Running),
                ("Stop", None, NeedsReply),
            ],
        );
    }

    #[test]
    fn codex_interrupt_and_quit() {
        walk(
            Idle,
            &[
                ("UserPromptSubmit", None, Running),
                ("PreToolUse", Some("Bash"), Running),
                ("Interrupt", None, NeedsReply),
                ("Interrupt", None, NeedsReply),
                ("SessionEnd", None, Exited),
            ],
        );
    }

    #[test]
    fn ask_user_question_cycle() {
        walk(
            Running,
            &[
                ("PreToolUse", Some("AskUserQuestion"), NeedsReply),
                ("PostToolUse", Some("AskUserQuestion"), Running),
            ],
        );
    }

    #[test]
    fn rank_order() {
        let mut all = [Exited, Idle, Running, NeedsReply, Starting, NeedsApproval];
        all.sort_by_key(|s| s.rank());
        assert_eq!(all[0], NeedsApproval);
        assert_eq!(all[1], NeedsReply);
        assert_eq!(all[2], Running);
        assert_eq!(all[5], Exited);
        assert_eq!(Idle.rank(), Starting.rank());
    }

    #[test]
    fn age_format() {
        assert_eq!(format_age(0), "刚刚");
        assert_eq!(format_age(59), "刚刚");
        assert_eq!(format_age(60), "1分");
        assert_eq!(format_age(3599), "59分");
        assert_eq!(format_age(3600), "1时");
        assert_eq!(format_age(86399), "23时");
        assert_eq!(format_age(86400), "1天");
        assert_eq!(format_age(86400 * 5 + 99), "5天");
    }

    #[test]
    fn name_matches_serde() {
        for s in [Starting, Idle, Running, NeedsApproval, NeedsReply, Exited] {
            assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{}\"", s.name()));
        }
    }

    #[test]
    fn serde_names() {
        assert_eq!(serde_json::to_string(&NeedsApproval).unwrap(), "\"needs_approval\"");
        assert_eq!(serde_json::from_str::<Status>("\"needs_reply\"").unwrap(), NeedsReply);
    }
}
