//! 对话的话题名：Claude、Codex 把对话主题写进终端标题，tmux 记在窗格标题里；这里把它清理成能显示的名字。
//! 桌面应用的侧栏和命令行的 `shifu status` 共用。

use crate::tmux::Tmux;
use std::collections::HashMap;
use std::process::Command;

fn is_status_glyph(c: char) -> bool {
    // Claude puts ✳ ✶ ✻ ✽ (dingbats) or a braille spinner in front of the topic; Codex and others use shapes or dots.
    matches!(c, '\u{00B7}' | '\u{2022}' | '*' | '\u{2600}'..='\u{27BF}' | '\u{25A0}'..='\u{25FF}' | '\u{2800}'..='\u{28FF}')
}

/// The topic out of a tmux pane title: status symbols in front and ` | <folder>` behind are dropped. A title that
/// is then empty or only the program name, the folder name or the machine's host name (the shell's default)
/// says nothing, so it is `None`.
pub fn clean_title(raw: &str, dir_name: &str, hostname: &str) -> Option<String> {
    let t = raw.trim_start_matches(|c: char| c.is_whitespace() || is_status_glyph(c)).trim_end();
    let t = t.strip_suffix(&format!(" | {dir_name}")).unwrap_or(t).trim();
    let host_short = hostname.split('.').next().unwrap_or(hostname);
    let empty_names = ["", "Claude Code", "Codex", dir_name, hostname, host_short];
    if empty_names.iter().any(|n| n.eq_ignore_ascii_case(t)) { None } else { Some(t.to_string()) }
}

/// `pane_id -> raw title` from `list-panes -a -F "#{pane_id} #{pane_title}"`.
pub fn parse_titles(out: &str) -> HashMap<String, String> {
    out.lines().filter_map(|l| l.split_once(' ')).map(|(id, title)| (id.to_string(), title.to_string())).collect()
}

pub fn pane_titles(tmux: &Tmux) -> HashMap<String, String> {
    parse_titles(&tmux.run(["list-panes", "-a", "-F", "#{pane_id} #{pane_title}"]).unwrap_or_default())
}

/// The machine's host name; the shell sets the pane title to it, so it must not count as a topic.
pub fn hostname() -> &'static str {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        let out = Command::new("hostname").output().ok().filter(|o| o.status.success());
        out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_titles_are_cleaned_to_the_topic() {
        let host = "developer-host.local";
        let c = |raw: &str, dir: &str| clean_title(raw, dir, host);
        assert_eq!(c("✳ 赌石模拟器云端同步与DeepSeek接入", "Workspace").as_deref(), Some("赌石模拟器云端同步与DeepSeek接入"));
        assert_eq!(c("检查采购总表 | Workspace", "Workspace").as_deref(), Some("检查采购总表"));
        assert_eq!(c("⠂ 修仙模拟器", "Workspace").as_deref(), Some("修仙模拟器"));
        assert_eq!(c("⠐ 修仙模拟器 | Workspace", "Workspace").as_deref(), Some("修仙模拟器"));
        // a bracket or slash is part of the name, not a status symbol
        assert_eq!(c("(草稿) notes", "w").as_deref(), Some("(草稿) notes"));
        assert_eq!(c("/tmp/a.png", "w").as_deref(), Some("/tmp/a.png"));
    }

    #[test]
    fn titles_that_say_nothing_are_none() {
        let host = "developer-host.local";
        for raw in ["✳ Claude Code", "claude code", "Codex", "Workspace", "workspace", host, "developer-host", "", "  ✳ ", "Claude Code | Workspace"] {
            assert_eq!(clean_title(raw, "Workspace", host), None, "{raw:?}");
        }
    }

    #[test]
    fn titles_are_read_per_pane_even_with_spaces_and_chinese() {
        let got = parse_titles("%1 ✳ 修仙 模拟器\n%2 host\n%3 \nbad");
        assert_eq!(got["%1"], "✳ 修仙 模拟器");
        assert_eq!(got["%2"], "host");
        assert_eq!(got["%3"], "");
        assert_eq!(got.len(), 3);
    }
}
