use shifu::{bridge, commands, computer, hook, sidebar};
use std::process::ExitCode;

const USAGE: &str = "shifu — 多 agent 终端工作台

用法:
  shifu                        打开工作台（没有会话就建一个）
  shifu new <agent> [目录] [-- 额外参数...]
                               开一个 agent：claude、codex 或任意命令名
                               --focus 建好后切到它
  shifu ls                     列出所有 agent
  shifu jump                   跳到等你最久的 agent
  shifu status [--json]        Shifu 应用里每个对话在干什么、等了多久、最后一句
  shifu tell <对话> [--from 谁] <话>
                               往停着的对话发消息；加 --queue --id <ID> 可向原会话排队追加
  shifu inbox <对话> <消息ID> [--cancel]
                               查看追加消息回执，或取消尚未送入的消息
  shifu stepfun                用 Step Plan 订阅开始或继续 StepFun 会话
  shifu help                   显示这段
  shifu computer help          看窗口、点击、输入（macOS，不依赖 Orca）
  shifu browser help           浏览器 DOM、标签页、截图（Playwright）
  shifu remote help            从手机连接桌面应用里的现有会话

快捷键（在 Shifu 的 tmux 里，先按 Ctrl-b）:
  j  跳到等你的 agent    s  光标回侧栏    n  新建 agent";

fn dispatch(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None => commands::cmd_open(),
        Some("new") => commands::cmd_new(&args[1..]),
        Some("ls") => commands::cmd_ls(),
        Some("jump") => commands::cmd_jump(),
        Some("computer") => computer::run(&args[1..]),
        Some("browser") => shifu::browser::run(&args[1..]),
        Some("remote") => shifu::remote::run(&args[1..]),
        Some("stepfun") => shifu::stepfun::run(&args[1..]),
        Some("tally") => commands::cmd_tally(),
        Some("act") => commands::cmd_act(&args[1..]),
        Some("status") => {
            bridge::default_to_app();
            bridge::cmd_status(&args[1..])
        }
        Some("tell") => {
            bridge::default_to_app();
            bridge::cmd_tell(&args[1..])
        }
        Some("inbox") => {
            bridge::default_to_app();
            bridge::cmd_inbox(&args[1..])
        }
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
        hook::run(args.get(1).map(String::as_str).unwrap_or(""), args.get(2).map(String::as_str).unwrap_or(""));
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
