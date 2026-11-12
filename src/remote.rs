//! The remote gateway is installed separately so the desktop and hooks do not load a web server.

use crate::agent::Res;
use std::path::PathBuf;
use std::process::Command;

pub fn run(args: &[String]) -> Res<()> {
    crate::bridge::default_to_app();
    let binary = std::env::var_os("SHIFU_REMOTE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".shifu/remote/bin/shifu-remote")
        });
    if !binary.is_file() {
        return Err(
            "手机远程入口还没安装。请在 Shifu 项目中运行 bash scripts/install-remote.sh。".into(),
        );
    }
    use std::os::unix::process::CommandExt;
    Err(format!(
        "启动远程入口失败: {}",
        Command::new(binary).args(args).exec()
    ))
}
