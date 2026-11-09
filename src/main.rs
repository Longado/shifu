use shifu::{commands, hook, sidebar};
use std::process::ExitCode;

const USAGE: &str = "shifu — 多 agent 终端工作台

用法:
  shifu                        打开工作台（没有会话就建一个）
  shifu new <agent> [目录] [-- 额外参数...]
                               开一个 agent：claude、codex 或任意命令名
                               --focus 建好后切到它
  shifu ls                     列出所有 agent
  shifu jump                   跳到等你最久的 agent
  shifu help                   显示这段

快捷键（在 Shifu 的 tmux 里，先按 Ctrl-b）:
  j  跳到等你的 agent    s  光标回侧栏    n  新建 agent";

fn dispatch(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None => commands::cmd_open(),
        Some("new") => commands::cmd_new(&args[1..]),
        Some("ls") => commands::cmd_ls(),
        Some("jump") => commands::cmd_jump(),
        Some("tally") => commands::cmd_tally(),
        Some("act") => commands::cmd_act(&args[1..]),
        Some("sidebar") => sidebar::run(),
        Some("help" | "-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("不认识的命令 {other:?}。运行 shifu help 看用法。")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // hook 必须永远 exit 0 且不输出，单独走，不进统一的错误打印
    if args.first().map(String::as_str) == Some("hook") {
        hook::run(args.get(2).map(String::as_str).unwrap_or(""));
        return ExitCode::SUCCESS;
    }
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("shifu: {e}");
            ExitCode::FAILURE
        }
    }
}
