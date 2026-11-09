//! 启动命令：shell 引号、Claude 的 hooks 文件、Codex 的 `-c` 覆盖。

use crate::agent::Kind;
use serde_json::{Value, json};

/// shell 单引号转义。
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 路径里没有特殊字符就原样用，否则加引号。
fn sh_word(s: &str) -> String {
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+:@=,".contains(c));
    if plain { s.to_string() } else { sh_quote(s) }
}

/// tmux 双引号字符串（配置文件里用）。
pub fn tmux_dq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// TOML 基本字符串（带引号）。
fn toml_basic(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// hook 命令行：`<SHIFU> hook <kind> <事件>`。
fn hook_command(shifu: &str, kind: &str, event: &str) -> String {
    format!("{} hook {kind} {event}", sh_word(shifu))
}

const CLAUDE_EVENTS: [&str; 7] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];

/// 顺序和文本必须固定：Codex 的信任记录按它们算。
const CODEX_EVENTS: [&str; 7] = [
    "SessionStart",
    "UserPromptSubmit",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
    "Interrupt",
    "SessionEnd",
];

pub fn claude_hooks_json(shifu: &str) -> Value {
    hooks_json(shifu, "claude")
}

/// CodeBuddy takes Claude's hook events and settings format; only the label in the command differs.
pub fn codebuddy_hooks_json(shifu: &str) -> Value {
    hooks_json(shifu, "codebuddy")
}

fn hooks_json(shifu: &str, kind: &str) -> Value {
    let mut hooks = serde_json::Map::new();
    for event in CLAUDE_EVENTS {
        let group = json!({
            "hooks": [{"type": "command", "command": hook_command(shifu, kind, event)}]
        });
        hooks.insert(event.to_string(), json!([group]));
    }
    json!({ "hooks": Value::Object(hooks) })
}

/// Kimi's events have Claude's names but no PermissionRequest (Shifu starts it with --yolo, so it never asks);
/// the tool events only matter for AskUserQuestion, so only that tool calls back. StopFailure: a turn ended on an error.
const KIMI_EVENTS: [&str; 7] = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "StopFailure", "SessionEnd"];

/// Kimi reads hooks only from its config file, and it refuses `/model` unless that is the default
/// `~/.kimi/config.toml` (a copy through `--config-file` blocked it. So Shifu's hooks live in the
/// user's own file: `user` (may be empty) with any earlier Shifu hooks replaced by the current ones, their own
/// hooks kept. `None` when the file already has exactly these. Outside Shifu the hooks do nothing (no SHIFU_ID).
pub fn kimi_config(user: &str, shifu: &str) -> Result<Option<String>, String> {
    let mut table: toml::Table = user.parse().map_err(|e| format!("Kimi 的 config.toml 读不懂: {e}"))?;
    let existing = match table.remove("hooks") {
        Some(toml::Value::Array(own)) => own,
        _ => Vec::new(),
    };
    let is_shifus = |h: &toml::Value| h.get("command").and_then(toml::Value::as_str).is_some_and(|c| c.contains(" hook kimi "));
    let (old, mut hooks): (Vec<toml::Value>, Vec<toml::Value>) = existing.into_iter().partition(is_shifus);
    let ours = kimi_hooks(shifu);
    // Kimi writes its defaults back when it saves the file (`matcher = ""`, `timeout = 30`), so compare what matters.
    let key = |h: &toml::Value| ["event", "command", "matcher"].map(|k| h.get(k).and_then(toml::Value::as_str).unwrap_or("").to_string());
    if old.iter().map(key).eq(ours.iter().map(key)) {
        return Ok(None);
    }
    hooks.extend(ours);
    table.insert("hooks".into(), hooks.into());
    toml::to_string(&table).map(Some).map_err(|e| e.to_string())
}

fn kimi_hooks(shifu: &str) -> Vec<toml::Value> {
    let mut hooks = Vec::new();
    for event in KIMI_EVENTS {
        let mut hook = toml::Table::new();
        hook.insert("event".into(), event.into());
        hook.insert("command".into(), hook_command(shifu, "kimi", event).into());
        if event.ends_with("ToolUse") {
            hook.insert("matcher".into(), "^AskUserQuestion$".into());
        }
        hooks.push(hook.into());
    }
    hooks
}

/// `kimi-cli` rather than `kimi`: uv installs both, and `kimi` may be renamed away (`kimi-legacy`).
const KIMI_PROGRAM: &str = "kimi-cli";

/// 每个事件一条 `-c` 的值：`hooks.Stop=[{hooks=[{type="command",command="..."}]}]`。
pub fn codex_overrides(shifu: &str) -> Vec<String> {
    CODEX_EVENTS
        .iter()
        .map(|event| {
            let command = toml_basic(&hook_command(shifu, "codex", event));
            format!("hooks.{event}=[{{hooks=[{{type=\"command\",command={command}}}]}}]")
        })
        .collect()
}

/// 每条覆盖一对 `-c '<值>'`，已做 shell 转义；最后是不问批准、不进沙箱（和 Orca 一样默认 bypass）。
/// 只加在 hook 覆盖后面，覆盖本身的顺序和文本不变，Codex 已记下的 hooks 信任不受影响。
fn codex_flags(shifu: &str) -> Vec<String> {
    let mut flags: Vec<String> = codex_overrides(shifu).iter().flat_map(|o| ["-c".to_string(), sh_quote(o)]).collect();
    flags.push(CODEX_BYPASS.to_string());
    flags
}

/// Claude、Codex、CodeBuddy 都像 Orca 那样默认 bypass 启动。CodeBuddy 用和 Claude 一样的参数；
/// 它的 --help 写明高风险、极高风险的操作仍会问。
const CLAUDE_BYPASS: &str = "--dangerously-skip-permissions";
const CODEX_BYPASS: &str = "--dangerously-bypass-approvals-and-sandbox";
const KIMI_YOLO: &str = "--yolo";

/// 交给 tmux（再交给 shell）的完整启动命令。
/// `claude_settings` is the settings file `write_claude_hooks` wrote for this kind (Kimi takes none).
pub fn agent_command(kind: Kind, command: &str, shifu: &str, claude_settings: &str, extra: &[String]) -> String {
    let program = if kind == Kind::Kimi { KIMI_PROGRAM } else { command };
    let mut words: Vec<String> = vec![sh_quote(program)];
    match kind {
        Kind::Claude | Kind::CodeBuddy => {
            words.push("--settings".to_string());
            words.push(sh_quote(claude_settings));
            words.push(CLAUDE_BYPASS.to_string());
        }
        Kind::Codex => words.extend(codex_flags(shifu)),
        Kind::Kimi => words.push(KIMI_YOLO.to_string()),
        Kind::Other => {}
    }
    words.extend(extra.iter().map(|a| sh_quote(a)));
    words.join(" ")
}

/// 选的模型和思考档位换成各家命令行的参数（未加引号，交给 `agent_command` 的 extra 或 `resume_command`）。
/// 没选就不加，用命令行自己的默认。
pub fn model_flags(kind: Kind, model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let mut words = Vec::new();
    match kind {
        Kind::Codex => {
            if let Some(m) = model {
                words.extend(["-m".to_string(), m.to_string()]);
            }
            if let Some(e) = effort {
                words.extend(["-c".to_string(), format!("model_reasoning_effort=\"{e}\"")]);
            }
        }
        Kind::Claude | Kind::CodeBuddy => {
            if let Some(m) = model {
                words.extend(["--model".to_string(), m.to_string()]);
            }
            if let Some(e) = effort {
                words.extend(["--effort".to_string(), e.to_string()]);
            }
        }
        // Kimi has no effort levels, only thinking on or off (models.rs offers these two when the model can think).
        Kind::Kimi => {
            if let Some(m) = model {
                words.extend(["--model".to_string(), m.to_string()]);
            }
            match effort {
                Some("thinking") => words.push("--thinking".to_string()),
                Some("no-thinking") => words.push("--no-thinking".to_string()),
                _ => {}
            }
        }
        Kind::Other => {}
    }
    words
}

/// 恢复已退出的会话：Claude 用 `--resume <id>`（没有 id 用 `--continue`），
/// Codex 用 `resume` 子命令（没有 id 用 `--last`），其他命令原样重开。
/// `pick` 是 `model_flags` 的结果，恢复时照原样再带上。
pub fn resume_command(kind: Kind, command: &str, shifu: &str, claude_settings: &str, session_id: Option<&str>, pick: &[String]) -> String {
    match (kind, session_id) {
        (Kind::Claude | Kind::CodeBuddy | Kind::Kimi, id) => {
            let mut extra = pick.to_vec();
            match id {
                Some(id) => extra.extend(["--resume".to_string(), id.to_string()]),
                None => extra.push("--continue".to_string()),
            }
            agent_command(kind, command, shifu, claude_settings, &extra)
        }
        (Kind::Codex, id) => {
            let mut words = vec![sh_quote(command), sh_quote("resume")];
            words.extend(codex_flags(shifu));
            words.extend(pick.iter().map(|w| sh_quote(w)));
            words.push(sh_quote(id.unwrap_or("--last")));
            words.join(" ")
        }
        (Kind::Other, _) => agent_command(kind, command, shifu, claude_settings, &[]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_quote_handles_quotes_and_spaces() {
        assert_eq!(sh_quote("abc"), "'abc'");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote("中文 目录"), "'中文 目录'");
    }

    #[test]
    fn sh_word_quotes_only_when_needed() {
        assert_eq!(sh_word("/Users/e/.cargo/bin/shifu"), "/Users/e/.cargo/bin/shifu");
        assert_eq!(sh_word("/Users/my dir/shifu"), "'/Users/my dir/shifu'");
    }

    #[test]
    fn tmux_dq_escapes() {
        assert_eq!(tmux_dq("a\"b\\c$d"), "\"a\\\"b\\\\c\\$d\"");
    }

    #[test]
    fn codex_overrides_exact_text_and_order() {
        let got = codex_overrides("/opt/shifu");
        let want = [
            r#"hooks.SessionStart=[{hooks=[{type="command",command="/opt/shifu hook codex SessionStart"}]}]"#,
            r#"hooks.UserPromptSubmit=[{hooks=[{type="command",command="/opt/shifu hook codex UserPromptSubmit"}]}]"#,
            r#"hooks.PermissionRequest=[{hooks=[{type="command",command="/opt/shifu hook codex PermissionRequest"}]}]"#,
            r#"hooks.PostToolUse=[{hooks=[{type="command",command="/opt/shifu hook codex PostToolUse"}]}]"#,
            r#"hooks.Stop=[{hooks=[{type="command",command="/opt/shifu hook codex Stop"}]}]"#,
            r#"hooks.Interrupt=[{hooks=[{type="command",command="/opt/shifu hook codex Interrupt"}]}]"#,
            r#"hooks.SessionEnd=[{hooks=[{type="command",command="/opt/shifu hook codex SessionEnd"}]}]"#,
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn codex_override_escapes_backslash_and_quote_in_toml() {
        // 路径里带空格 → 先 shell 加引号；带 " 和 \ → TOML 再转义
        let got = codex_overrides("/x/a\"b\\c d/shifu");
        // shell 层: '/x/a"b\c d/shifu' hook codex Stop
        // TOML 层: " → \"  \ → \\
        let want_stop = r#"hooks.Stop=[{hooks=[{type="command",command="'/x/a\"b\\c d/shifu' hook codex Stop"}]}]"#;
        assert_eq!(got[4], want_stop);
    }

    #[test]
    fn codex_toml_value_roundtrips_for_tricky_command() {
        // 取出 command="..." 里的内容，按 TOML 基本字符串规则手工还原，应得到原命令
        let got = codex_overrides("/x/a\"b\\c d/shifu");
        let s = &got[0];
        let start = s.find("command=\"").unwrap() + "command=\"".len();
        let end = s.rfind("\"}]}]").unwrap();
        let mut decoded = String::new();
        let mut chars = s[start..end].chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                decoded.push(chars.next().unwrap());
            } else {
                decoded.push(c);
            }
        }
        assert_eq!(decoded, "'/x/a\"b\\c d/shifu' hook codex SessionStart");
    }

    #[test]
    fn claude_hooks_json_structure() {
        let v = claude_hooks_json("/opt/shifu");
        let hooks = v["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), 7);
        for ev in ["SessionStart", "UserPromptSubmit", "PreToolUse", "PermissionRequest", "PostToolUse", "Stop", "SessionEnd"] {
            let groups = hooks[ev].as_array().unwrap_or_else(|| panic!("缺 {ev}"));
            assert_eq!(groups.len(), 1, "{ev}");
            let inner = groups[0]["hooks"].as_array().unwrap();
            assert_eq!(inner.len(), 1);
            assert_eq!(inner[0]["type"], "command");
            assert_eq!(inner[0]["command"], format!("/opt/shifu hook claude {ev}"));
            // Every tool calls back, not only AskUserQuestion: a tool starting is how a woken agent shows it works.
            assert!(groups[0].get("matcher").is_none(), "{ev} 不应有 matcher");
        }
        assert!(hooks.get("Interrupt").is_none());
    }

    #[test]
    fn claude_command_has_settings() {
        let c = agent_command(Kind::Claude, "claude", "/opt/shifu", "/h/claude-hooks.json", &["--resume".to_string()]);
        assert_eq!(c, "'claude' --settings '/h/claude-hooks.json' --dangerously-skip-permissions '--resume'");
    }

    #[test]
    fn codex_command_has_seven_c_flags_in_order() {
        let c = agent_command(Kind::Codex, "codex", "/opt/shifu", "", &[]);
        assert!(c.starts_with("'codex' -c 'hooks.SessionStart="));
        assert_eq!(c.matches(" -c ").count(), 7);
        assert!(c.ends_with("hook codex SessionEnd\"}]}]' --dangerously-bypass-approvals-and-sandbox"));
        assert!(!c.contains("bypass-hook-trust"));
    }

    #[test]
    fn other_command_is_plain_with_quoted_args() {
        let c = agent_command(Kind::Other, "sh", "/opt/shifu", "", &["-c".to_string(), "sleep 600".to_string()]);
        assert_eq!(c, "'sh' '-c' 'sleep 600'");
    }

    #[test]
    fn claude_resume_with_and_without_session_id() {
        let with = resume_command(Kind::Claude, "claude", "/opt/shifu", "/h/claude-hooks.json", Some("abc-123"), &[]);
        assert_eq!(with, "'claude' --settings '/h/claude-hooks.json' --dangerously-skip-permissions '--resume' 'abc-123'");
        let without = resume_command(Kind::Claude, "claude", "/opt/shifu", "/h/claude-hooks.json", None, &[]);
        assert_eq!(without, "'claude' --settings '/h/claude-hooks.json' --dangerously-skip-permissions '--continue'");
    }

    #[test]
    fn codebuddy_runs_like_claude_with_its_own_hooks_label() {
        let c = agent_command(Kind::CodeBuddy, "codebuddy", "/opt/shifu", "/h/codebuddy-hooks.json", &[]);
        assert_eq!(c, "'codebuddy' --settings '/h/codebuddy-hooks.json' --dangerously-skip-permissions");
        let with = resume_command(Kind::CodeBuddy, "codebuddy", "/opt/shifu", "/h/codebuddy-hooks.json", Some("s-1"), &[]);
        assert_eq!(with, "'codebuddy' --settings '/h/codebuddy-hooks.json' --dangerously-skip-permissions '--resume' 's-1'");
        let hooks = codebuddy_hooks_json("/opt/shifu").to_string();
        assert!(hooks.contains("hook codebuddy PermissionRequest"));
        assert!(!hooks.contains("hook claude"));
        assert_eq!(Kind::of_command("codebuddy"), Kind::CodeBuddy);
        assert_eq!(serde_json::to_value(Kind::CodeBuddy).unwrap(), "codebuddy");
    }

    #[test]
    fn kimi_config_keeps_the_users_settings_and_hooks_and_adds_shifus() {
        let user = r#"default_model = "kimi-code/k3"
hooks = [{ event = "Stop", command = "say done" }]

[models."kimi-code/k3"]
provider = "managed:kimi-code"
model = "k3"
"#;
        let text = kimi_config(user, "/opt/shifu").unwrap().unwrap();
        let v: toml::Table = text.parse().unwrap();
        assert_eq!(v["default_model"].as_str(), Some("kimi-code/k3"));
        assert_eq!(v["models"]["kimi-code/k3"]["model"].as_str(), Some("k3"));
        let hooks = v["hooks"].as_array().unwrap();
        assert_eq!(hooks.len(), 8);
        assert_eq!(hooks[0]["command"].as_str(), Some("say done"));
        let ours: Vec<&str> = hooks[1..].iter().map(|h| h["event"].as_str().unwrap()).collect();
        assert_eq!(ours, KIMI_EVENTS);
        assert_eq!(hooks[1]["command"].as_str(), Some("/opt/shifu hook kimi SessionStart"));
        assert!(hooks[1].get("matcher").is_none());
        assert_eq!(hooks[3]["matcher"].as_str(), Some("^AskUserQuestion$"));
        assert_eq!(kimi_config("", "/opt/shifu").unwrap().unwrap().parse::<toml::Table>().unwrap()["hooks"].as_array().unwrap().len(), 7);
        assert!(kimi_config("not = [toml", "/opt/shifu").is_err());
        // Already in place: nothing to write. A moved shifu: its old hooks are replaced, the user's stays.
        assert_eq!(kimi_config(&text, "/opt/shifu").unwrap(), None);
        let saved_by_kimi = text.replace("event = \"SessionStart\"", "event = \"SessionStart\"\nmatcher = \"\"\ntimeout = 30");
        assert_ne!(saved_by_kimi, text);
        assert_eq!(kimi_config(&saved_by_kimi, "/opt/shifu").unwrap(), None);
        let moved: toml::Table = kimi_config(&text, "/new/shifu").unwrap().unwrap().parse().unwrap();
        let hooks = moved["hooks"].as_array().unwrap();
        assert_eq!(hooks.len(), 8);
        assert_eq!(hooks[0]["command"].as_str(), Some("say done"));
        assert_eq!(hooks[1]["command"].as_str(), Some("/new/shifu hook kimi SessionStart"));
    }

    #[test]
    fn kimi_starts_on_its_own_config_in_yolo_and_resumes_by_session() {
        let c = agent_command(Kind::Kimi, "kimi", "/opt/shifu", "/h/kimi.toml", &model_flags(Kind::Kimi, Some("kimi-code/k3"), Some("no-thinking")));
        assert_eq!(c, "'kimi-cli' --yolo '--model' 'kimi-code/k3' '--no-thinking'");
        let r = resume_command(Kind::Kimi, "kimi", "/opt/shifu", "/h/kimi.toml", Some("1377-abc"), &[]);
        assert_eq!(r, "'kimi-cli' --yolo '--resume' '1377-abc'");
        let last = resume_command(Kind::Kimi, "kimi", "/opt/shifu", "/h/kimi.toml", None, &[]);
        assert!(last.ends_with("--yolo '--continue'"), "{last}");
        assert_eq!(model_flags(Kind::Kimi, None, Some("thinking")), ["--thinking"]);
        assert_eq!(Kind::of_command("kimi-cli"), Kind::Kimi);
    }

    #[test]
    fn picked_model_and_effort_reach_each_cli_and_survive_resume() {
        let claude = model_flags(Kind::Claude, Some("claude-sonnet-5-5"), Some("high"));
        assert_eq!(claude, ["--model", "claude-sonnet-5-5", "--effort", "high"]);
        let c = resume_command(Kind::Claude, "claude", "/opt/shifu", "/h/c.json", Some("abc"), &claude);
        assert!(c.ends_with("'--model' 'claude-sonnet-5-5' '--effort' 'high' '--resume' 'abc'"), "{c}");
        let codex = model_flags(Kind::Codex, Some("gpt-6-luna"), Some("low"));
        assert_eq!(codex, ["-m", "gpt-6-luna", "-c", "model_reasoning_effort=\"low\""]);
        let r = resume_command(Kind::Codex, "codex", "/opt/shifu", "", Some("019a"), &codex);
        assert!(r.ends_with("'-m' 'gpt-6-luna' '-c' 'model_reasoning_effort=\"low\"' '019a'"), "{r}");
        assert!(model_flags(Kind::Claude, None, None).is_empty());
        assert!(model_flags(Kind::Other, Some("x"), Some("y")).is_empty());
    }

    #[test]
    fn codex_resume_keeps_the_seven_overrides_in_order() {
        let fresh = agent_command(Kind::Codex, "codex", "/opt/shifu", "", &[]);
        let overrides = fresh.strip_prefix("'codex' ").unwrap();
        let with = resume_command(Kind::Codex, "codex", "/opt/shifu", "", Some("019a-xyz"), &[]);
        assert_eq!(with, format!("'codex' 'resume' {overrides} '019a-xyz'"));
        let without = resume_command(Kind::Codex, "codex", "/opt/shifu", "", None, &[]);
        assert_eq!(without, format!("'codex' 'resume' {overrides} '--last'"));
        assert!(with.contains(" --dangerously-bypass-approvals-and-sandbox '019a-xyz'"));
        assert!(!with.contains("bypass-hook-trust"));
    }

    #[test]
    fn other_resume_reopens_plain_command() {
        assert_eq!(resume_command(Kind::Other, "gemini", "/opt/shifu", "", Some("ignored"), &[]), "'gemini'");
    }
}
