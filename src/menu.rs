//! 侧栏弹出的 tmux 菜单（`display-menu`）：新建 agent、agent 右键菜单，以及「最近用过的目录」。

use crate::agent::{Res, shifu_home};
use crate::launch::{sh_quote, tmux_dq};
use crate::tmux::new_prompt_template;

pub const RECENT_MAX: usize = 6;

/// 菜单里的一行。名字为空是分隔线。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub name: String,
    pub key: String,
    pub command: String,
}

/// 目录能放进菜单：含 `'` `"` `$` `;` 的会让命令转义出错，`#` `\` 和换行会被 tmux 当成格式或转义，一律不列。
pub fn menu_safe(dir: &str) -> bool {
    !dir.is_empty() && !dir.contains(['\'', '"', '$', ';', '#', '\\', '\n'])
}

/// 把目录放到最前，去重，最多 `RECENT_MAX` 个。
pub fn push_recent(list: &[String], dir: &str) -> Vec<String> {
    std::iter::once(dir.to_string()).chain(list.iter().filter(|d| *d != dir).cloned()).take(RECENT_MAX).collect()
}

fn recent_path() -> std::path::PathBuf {
    shifu_home().join("recent.json")
}

/// 文件不存在就是空；内容坏了报错，由调用方决定怎么提示。
pub fn load_recent() -> Res<Vec<String>> {
    let path = recent_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{} 格式不对: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("读 {} 失败: {e}", path.display())),
    }
}

/// `shifu new` 成功后调用。原子写入，和 agent 文件一样。
pub fn record_recent(dir: &str) -> Res<()> {
    let list = push_recent(&load_recent().unwrap_or_default(), dir);
    let path = recent_path();
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(&list).map_err(|e| format!("序列化失败: {e}"))?;
    std::fs::write(&tmp, text).map_err(|e| format!("写 {} 失败: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("改名到 {} 失败: {e}", path.display()))
}

fn base_name(dir: &str) -> &str {
    dir.rsplit('/').find(|p| !p.is_empty()).unwrap_or(dir)
}

fn item(name: String, key: &str, command: String) -> MenuItem {
    MenuItem { name, key: key.to_string(), command }
}

fn run_shell(shell_command: &str) -> String {
    format!("run-shell {}", tmux_dq(shell_command))
}

fn new_in(shifu: &str, kind: &str, dir: &str) -> String {
    run_shell(&format!("{} new {kind} {} --focus", sh_quote(shifu), sh_quote(dir)))
}

/// 「新建 agent」菜单：当前目录的 Claude、Codex，最近用过的其他目录的 Claude，最后是「其他目录…」。
pub fn new_menu(shifu: &str, base_dir: &str, recent: &[String]) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if menu_safe(base_dir) {
        let name = base_name(base_dir);
        items.push(item(format!("Claude · {name}"), "c", new_in(shifu, "claude", base_dir)));
        items.push(item(format!("Codex · {name}"), "x", new_in(shifu, "codex", base_dir)));
    }
    let others = recent.iter().filter(|d| d.as_str() != base_dir && menu_safe(d));
    for (n, dir) in others.enumerate() {
        items.push(item(format!("Claude · {}", base_name(dir)), &(n + 1).to_string(), new_in(shifu, "claude", dir)));
    }
    let initial = if menu_safe(base_dir) { base_dir } else { "" };
    let prompt = format!(
        "command-prompt -p {} -I {} {}",
        tmux_dq("agent:,目录:"),
        tmux_dq(&format!("claude,{initial}")),
        tmux_dq(&new_prompt_template(shifu))
    );
    items.push(item("其他目录…".to_string(), "o", prompt));
    items
}

/// agent 的右键菜单。恢复会话只对已退出的显示。
/// `page` 是打开菜单时侧栏所在的页（窗口 id）：菜单项经 run-shell 执行时拿不到可靠的「当前页」，
/// 实测会落到主页上，所以把页直接写进命令。
pub fn agent_menu(shifu: &str, name: &str, exited: bool, page: &str) -> Vec<MenuItem> {
    let act = |action: &str| run_shell(&format!("{} act {action} {name} {page}", sh_quote(shifu)));
    let mut items = vec![
        item("跳过去".to_string(), "j", act("jump")),
        item("并排到右边".to_string(), "v", act("right")),
        item("并排到下面".to_string(), "b", act("below")),
        item("单独成页".to_string(), "u", act("own")),
        item("平均排列".to_string(), "=", act("even")),
        item("在 Finder 打开".to_string(), "o", act("finder")),
    ];
    if exited {
        items.push(item("恢复会话".to_string(), "R", act("resume")));
    }
    items.push(item(String::new(), "", String::new()));
    items.push(item("关闭".to_string(), "X", act("close")));
    items
}

/// `tmux display-menu` 的整条参数。`x`、`y` 是终端里的坐标，或 tmux 认的 `P`、`C` 这类位置词。
pub fn menu_args(title: &str, x: &str, y: &str, target_pane: &str, items: &[MenuItem]) -> Vec<String> {
    let head = ["display-menu", "-T", title, "-x", x, "-y", y, "-t", target_pane];
    // 空名字是分隔线，tmux 不会再读它后面的 key 和 command
    let rows = items.iter().flat_map(|i| if i.name.is_empty() { vec![String::new()] } else { vec![i.name.clone(), i.key.clone(), i.command.clone()] });
    head.iter().map(|s| s.to_string()).chain(rows).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn names(items: &[MenuItem]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn dangerous_directories_are_not_menu_safe() {
        for bad in ["/x/it's", "/x/a\"b", "/x/$HOME", "/x/a;b", "/x/#{x}", "/x/a\\b", "/x/a\nb", ""] {
            assert!(!menu_safe(bad), "{bad:?}");
        }
        for good in ["/x/中文 目录", "/Users/e/proj-1.2_x", "/a b/c(d)"] {
            assert!(menu_safe(good), "{good:?}");
        }
    }

    #[test]
    fn recent_moves_to_front_and_dedupes() {
        assert_eq!(push_recent(&dirs(&["/a", "/b", "/c"]), "/b"), dirs(&["/b", "/a", "/c"]));
        assert_eq!(push_recent(&[], "/a"), dirs(&["/a"]));
    }

    #[test]
    fn recent_is_capped_at_six() {
        let mut list = Vec::new();
        for i in 1..=8 {
            list = push_recent(&list, &format!("/d{i}"));
        }
        assert_eq!(list, dirs(&["/d8", "/d7", "/d6", "/d5", "/d4", "/d3"]));
    }

    #[test]
    fn new_menu_lists_base_then_recent_then_other() {
        let recent = dirs(&["/w/proj", "/w/other", "/w/third"]);
        let items = new_menu("/p/shifu", "/w/proj", &recent);
        assert_eq!(
            names(&items),
            ["Claude · proj", "Codex · proj", "Claude · other", "Claude · third", "其他目录…"]
        );
        assert_eq!(items[0].command, "run-shell \"'/p/shifu' new claude '/w/proj' --focus\"");
        assert_eq!(items[1].command, "run-shell \"'/p/shifu' new codex '/w/proj' --focus\"");
        assert_eq!(items[2].command, "run-shell \"'/p/shifu' new claude '/w/other' --focus\"");
    }

    #[test]
    fn new_menu_keeps_chinese_and_space_but_drops_dangerous() {
        let recent = dirs(&["/w/中文 目录", "/w/it's", "/w/a$b", "/w/a;b", "/w/q\"r", "/w/ok"]);
        let items = new_menu("/p/shifu", "/w/base", &recent);
        assert_eq!(
            names(&items),
            ["Claude · base", "Codex · base", "Claude · 中文 目录", "Claude · ok", "其他目录…"]
        );
        assert_eq!(items[2].command, "run-shell \"'/p/shifu' new claude '/w/中文 目录' --focus\"");
    }

    #[test]
    fn new_menu_with_dangerous_base_has_no_base_rows() {
        let items = new_menu("/p/shifu", "/w/it's", &dirs(&["/w/ok"]));
        assert_eq!(names(&items), ["Claude · ok", "其他目录…"]);
        // 输入框的初始目录也不带危险的路径
        assert!(items.last().unwrap().command.contains("-I \"claude,\""));
    }

    #[test]
    fn other_directory_row_opens_the_v1_prompt() {
        let items = new_menu("/p/shifu", "/w/proj", &[]);
        let last = items.last().unwrap();
        assert_eq!(last.name, "其他目录…");
        assert_eq!(
            last.command,
            "command-prompt -p \"agent:,目录:\" -I \"claude,/w/proj\" \"run-shell \\\"'/p/shifu' new '%1' '%2' --focus\\\"\""
        );
    }

    #[test]
    fn agent_menu_items_and_actions() {
        let running = agent_menu("/p/shifu", "claude-a", false, "@7");
        assert_eq!(
            names(&running),
            ["跳过去", "并排到右边", "并排到下面", "单独成页", "平均排列", "在 Finder 打开", "", "关闭"]
        );
        assert_eq!(running[0].command, "run-shell \"'/p/shifu' act jump claude-a @7\"");
        assert_eq!(running[1].command, "run-shell \"'/p/shifu' act right claude-a @7\"");
        assert_eq!(running[2].command, "run-shell \"'/p/shifu' act below claude-a @7\"");
        assert_eq!(running[3].command, "run-shell \"'/p/shifu' act own claude-a @7\"");
        assert_eq!(running[4].command, "run-shell \"'/p/shifu' act even claude-a @7\"");
        assert_eq!(running[5].command, "run-shell \"'/p/shifu' act finder claude-a @7\"");
        assert_eq!(running.last().unwrap().command, "run-shell \"'/p/shifu' act close claude-a @7\"");
    }

    #[test]
    fn resume_only_shown_for_exited() {
        let exited = agent_menu("/p/shifu", "claude-a", true, "@7");
        assert!(names(&exited).contains(&"恢复会话"));
        let item = exited.iter().find(|i| i.name == "恢复会话").unwrap();
        assert_eq!(item.command, "run-shell \"'/p/shifu' act resume claude-a @7\"");
        assert!(!names(&agent_menu("/p/shifu", "claude-a", false, "@7")).contains(&"恢复会话"));
    }

    #[test]
    fn menu_args_shape() {
        let items = vec![MenuItem { name: "甲".into(), key: "a".into(), command: "run-shell \"x\"".into() }];
        let args = menu_args("新建 agent", "5", "7", "%3", &items);
        assert_eq!(args, ["display-menu", "-T", "新建 agent", "-x", "5", "-y", "7", "-t", "%3", "甲", "a", "run-shell \"x\""]);
    }

    #[test]
    fn separator_is_a_single_empty_argument() {
        // tmux 遇到空名字就当分隔线，不再读 key 和 command
        let items = vec![
            MenuItem { name: "甲".into(), key: "a".into(), command: "c1".into() },
            MenuItem { name: String::new(), key: String::new(), command: String::new() },
            MenuItem { name: "乙".into(), key: "b".into(), command: "c2".into() },
        ];
        let args = menu_args("t", "0", "0", "%1", &items);
        assert_eq!(args[9..], ["甲", "a", "c1", "", "乙", "b", "c2"]);
    }
}
