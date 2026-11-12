//! A remote view attaches to an existing tmux session; it never starts an agent.

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::BorrowedFd,
};
use tokio::sync::mpsc;

pub struct Target {
    pub session: String,
    pub cols: u16,
    pub rows: u16,
}
pub enum Output {
    Data(Vec<u8>),
    Exit,
}
pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: File,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

pub fn open(target: Target) -> super::Result<(Terminal, mpsc::Receiver<Output>)> {
    let pair = native_pty_system()
        .openpty(PtySize {
            cols: target.cols,
            rows: target.rows,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut command = CommandBuilder::new(shifu::tmux::find_tmux_bin());
    command.args([
        "-L",
        &shifu::tmux::Tmux::from_env().socket,
        "attach-session",
        "-E",
        "-f",
        "ignore-size",
        "-t",
        &target.session,
    ]);
    command.env_remove("TMUX");
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("LANG", "en_US.UTF-8");
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    // portable-pty's take_writer() sends EOF on Drop; that can quit the original agent.
    let fd = pair.master.as_raw_fd().ok_or("PTY 缺少文件描述符")?;
    let writer = File::from(
        unsafe { BorrowedFd::borrow_raw(fd) }
            .try_clone_to_owned()
            .map_err(|e| e.to_string())?,
    );
    let terminal = Terminal {
        master: pair.master,
        writer,
        killer: child.clone_killer(),
    };
    let (tx, rx) = mpsc::channel(16);
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0
                || tx
                    .blocking_send(Output::Data(buffer[..n].to_vec()))
                    .is_err()
            {
                break;
            }
        }
        let _ = child.wait();
        let _ = tx.blocking_send(Output::Exit);
    });
    Ok((terminal, rx))
}

impl Terminal {
    pub fn write(&mut self, data: &[u8]) -> super::Result<()> {
        self.writer
            .write_all(data)
            .and_then(|_| self.writer.flush())
            .map_err(|e| format!("输入未确认，请查看原终端后再决定是否重发：{e}"))
    }
    pub fn resize(&self, cols: u16, rows: u16) -> super::Result<()> {
        self.master
            .resize(PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.killer.kill();
    }
}
