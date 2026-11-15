//! 每日分析：侧栏里钉在项目经理下面的常驻 agent。它在自己的文件夹里开（`<home>/daily`），那里的 CLAUDE.md／AGENTS.md
//! 写着职责。可手动请求分析；定时触发由用户自行配置。

use crate::config::Config;

pub const TASK_NAME: &str = "每日分析";

const BRIEF: &str = "# 每日分析

你常驻在 Shifu，任务名「每日分析」。用户说「开始分析 <日期>」时，汇总当天各项目的推进、阻碍和需要决定的事情；平时也回答追问。用中文，先给结论。

1. 先用 `shifu status --json` 查看当前会话，再只读相关项目的 HANDOFF.md、CHANGELOG.md 和日期对应的记录。当前状态与当天发生的事情分开写。
2. 只有用户配置并授权了本地安全审阅、云机花费或 Agent 用量报告时才读取；缺少来源时说明未配置，不编造数字，不调用不存在的私人脚本。估算与实际账单分开。
3. 回复先列需要用户决定或处理的事情，再列各项目推进与阻碍；数字只用实际读到的来源。报告默认留在本对话，只有用户指定输出目录才保存文件。
4. 不改项目代码、数据库、云资源或凭据，不给其它会话传话，不批准或撤销授权，不对外发送或推送。

本版本不安装定时任务。需要每日定时分析时，由用户另行配置调度器调用 `shifu tell`；定时授权不能代替上述数据访问权限。
";

/// The folder exists and its instructions are current. Returns the folder as tasks store it (canonical).
pub fn ensure_dir(cfg: &Config) -> Result<String, String> {
    crate::pm::ensure_role_dir(cfg, "daily", BRIEF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_gets_its_own_folder_with_instructions_next_to_the_project_manager() {
        let home = std::env::temp_dir().join(format!("shifu-daily-test-{}", std::process::id()));
        let cfg = Config { home: home.clone(), socket: "s".into(), shifu_bin: "shifu".into(), user_statusline: None };
        let daily = ensure_dir(&cfg).unwrap();
        let pm = crate::pm::ensure_dir(&cfg).unwrap();
        assert_ne!(daily, pm);
        let brief = std::fs::read_to_string(std::path::Path::new(&daily).join("CLAUDE.md")).unwrap();
        assert!(brief.contains("开始分析 <日期>"));
        assert_eq!(brief, std::fs::read_to_string(std::path::Path::new(&daily).join("AGENTS.md")).unwrap());
        std::fs::remove_dir_all(home).unwrap();
    }
}
