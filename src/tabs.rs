//! 标签栏（tmux 状态栏）：页名、每页的状态圆点、右侧「在等你」计数。

/// 一页的名字：1 个 agent 写名字，2 个用 ` + ` 连起来，更多写「第一个名字 +N」。没有 agent 的页是 `home`。
pub fn page_name(names: &[String]) -> String {
    match names {
        [] => "home".to_string(),
        [one] => one.clone(),
        [a, b] => format!("{a} + {b}"),
        [first, rest @ ..] => format!("{first} +{}", rest.len()),
    }
}

/// 一个 agent 窗格的圆点。`#{@shifu_status}` 是 `shifu hook` 写在窗格上的选项；
/// 没有这个选项的窗格（侧栏、home 的 shell）什么都不画。
/// 格式里的逗号只能是 `#{?}` 的分隔符，所以颜色一次只写一个属性。
fn status_dots() -> String {
    let by_status = |status: &str, mark: &str, otherwise: &str| format!("#{{?#{{==:#{{@shifu_status}},{status}}},{mark},{otherwise}}}");
    let dot = by_status(
        "needs_approval",
        "#[fg=yellow]●",
        &by_status(
            "needs_reply",
            "#[fg=brightblue]●",
            &by_status("running", "#[fg=green]●", &by_status("exited", "#[fg=colour240]×", "#[fg=colour245]○")),
        ),
    );
    format!("#{{P:#{{?#{{@shifu_status}},#{{?pane_dead,#[fg=colour240]×,{dot}}},}}}}")
}

/// `window-status-format`：这一页每个 agent 窗格一个圆点，后面是页名。
pub fn window_status_format() -> String {
    format!("{}#[fg=colour244] #W", status_dots())
}

/// `window-status-current-format`：同上，当前页的页名加粗变亮。
pub fn window_status_current_format() -> String {
    format!("{}#[fg=colour255,bold] #W", status_dots())
}

/// `shifu tally` 的输出：等批准和等回复各自的数量，带 tmux 颜色标记；都没有就是空。
pub fn tally_text(approval: usize, reply: usize) -> String {
    let parts: Vec<String> = [(approval, "yellow", "等你批准"), (reply, "brightblue", "等你回复")]
        .into_iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, colour, label)| format!("#[fg={colour}]{label} {count}#[default]"))
        .collect();
    parts.join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn page_name_one_two_many() {
        assert_eq!(page_name(&names(&["claude-a"])), "claude-a");
        assert_eq!(page_name(&names(&["claude-a", "codex-b"])), "claude-a + codex-b");
        assert_eq!(page_name(&names(&["claude-a", "codex-b", "sh-c"])), "claude-a +2");
        assert_eq!(page_name(&names(&["a", "b", "c", "d", "e"])), "a +4");
    }

    #[test]
    fn page_without_agents_is_home() {
        assert_eq!(page_name(&[]), "home");
    }

    /// 在格式串里找 `#{==:#{@shifu_status},<状态>},<分支>` 的分支文字。
    fn branch_after<'a>(format: &'a str, status: &str) -> &'a str {
        let marker = format!("#{{==:#{{@shifu_status}},{status}}},");
        let start = format.find(&marker).unwrap_or_else(|| panic!("格式里没有 {status} 分支")) + marker.len();
        format[start..].split(',').next().unwrap()
    }

    #[test]
    fn every_status_has_its_colour_branch() {
        for format in [window_status_format(), window_status_current_format()] {
            assert_eq!(branch_after(&format, "needs_approval"), "#[fg=yellow]●");
            assert_eq!(branch_after(&format, "needs_reply"), "#[fg=brightblue]●");
            assert_eq!(branch_after(&format, "running"), "#[fg=green]●");
            assert_eq!(branch_after(&format, "exited"), "#[fg=colour240]×");
            // 启动中和空闲没有单独分支，落在默认的灰色空心圆；窗格已死排在最前面
            assert!(format.contains("#{?pane_dead,#[fg=colour240]×,"), "{format}");
            assert!(format.contains("#[fg=colour245]○"), "{format}");
            assert!(format.starts_with("#{P:"), "{format}");
        }
    }

    #[test]
    fn braces_are_balanced() {
        for format in [window_status_format(), window_status_current_format()] {
            let open = format.matches("#{").count();
            let close = format.matches('}').count();
            assert_eq!(open, close, "{format}");
        }
    }

    #[test]
    fn current_page_name_is_bold_and_bright() {
        assert!(window_status_current_format().ends_with("#[fg=colour255,bold] #W"));
        assert!(window_status_format().ends_with("#[fg=colour244] #W"));
    }

    #[test]
    fn tally_cases() {
        assert_eq!(tally_text(0, 0), "");
        assert_eq!(tally_text(2, 0), "#[fg=yellow]等你批准 2#[default]");
        assert_eq!(tally_text(0, 3), "#[fg=brightblue]等你回复 3#[default]");
        assert_eq!(tally_text(1, 4), "#[fg=yellow]等你批准 1#[default]  #[fg=brightblue]等你回复 4#[default]");
    }
}
