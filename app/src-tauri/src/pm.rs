//! 项目经理：侧栏最上面常驻的那个 Claude。它在自己的文件夹里开（`<home>/pm`），那里的 CLAUDE.md（Codex、Kimi
//! 读 AGENTS.md）写着它的职责和两条命令（`shifu status`、`shifu tell`），所以重开、续接、/clear 之后都还记得。

use crate::config::Config;
use std::path::Path;

pub const TASK_NAME: &str = "项目经理";

fn brief(shifu: &str) -> String {
    format!(
        "# 你是 用户 的项目经理

你常驻在 Shifu 里。用户 同时开着很多个 agent 对话（Claude、Codex、CodeBuddy、Kimi），你替他盯全局，回答他「现在怎么样」「哪些在等我」「某件事做到哪了」，需要时替他给某个对话传话。用中文，说人话，先给结论。

## 看全局

- 回答之前先跑 `{shifu} status`。它按「等你批准 / 等你回复 / 在干活 / 启动中 / 空闲 / 已结束」列出每个对话：显示名、任务、哪家、在这个状态多久、方括号里的内部名、最后一句。
- 要细看某个对话做到哪：`{shifu} status --json` 里有每个对话的工作目录（cwd），去读那里的 HANDOFF.md、CHANGELOG.md 或最近的 git log。
- 汇报先说要他马上处理的（等批准、等回复），每个一句话说清是什么事、卡在哪；再说在干活的；空闲和已结束的只在他问时说。用显示名，不报内部名。

## 传话

- 只在 用户 明确让你传话时才发：`{shifu} tell <对话> --from 项目经理 <要说的话>`。<对话> 写方括号里的内部名最稳。
- 发之前跟他确认发给谁、原话是什么；他说「直接发」就不用再确认。
- 只能发给停着的对话（等你回复、空闲）。在干活或等批准的会被拒绝，告诉他稍后再发，不要绕过。

## 不做

- 不结束、不新开、不改别的对话，不替他批准任何东西。
- 不改各个项目的代码和文件，除非他明确让你改。
"
    )
}

/// The folder exists and its instructions are current (rewritten when the text or the `shifu` path changed).
/// Returns the folder as tasks store it (canonical).
pub fn ensure_dir(cfg: &Config) -> Result<String, String> {
    let dir = cfg.home.join("pm");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {} failed: {e}", dir.display()))?;
    let text = brief(&cfg.shifu_bin);
    for name in ["CLAUDE.md", "AGENTS.md"] {
        write_if_changed(&dir.join(name), &text)?;
    }
    let real = dir.canonicalize().map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(real.to_string_lossy().into_owned())
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(());
    }
    std::fs::write(path, text).map_err(|e| format!("write {} failed: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_names_the_installed_shifu_for_both_commands() {
        let b = brief("/Users/e/.cargo/bin/shifu");
        assert!(b.contains("`/Users/e/.cargo/bin/shifu status`"));
        assert!(b.contains("`/Users/e/.cargo/bin/shifu tell <对话> --from 项目经理"));
    }
}
