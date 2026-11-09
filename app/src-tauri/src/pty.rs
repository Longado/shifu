use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::BorrowedFd;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
struct PtyData {
    id: u32,
    data: String,
}

#[derive(Clone, Serialize)]
struct PtyExit {
    id: u32,
    code: Option<u32>,
}

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: File,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

/// All live PTYs, keyed by the id handed to the frontend.
#[derive(Default)]
pub struct Ptys {
    next_id: AtomicU32,
    sessions: Arc<Mutex<HashMap<u32, Session>>>,
}

impl Ptys {
    pub fn spawn(
        &self,
        app: AppHandle,
        program: &str,
        args: &[String],
        size: PtySize,
        cwd: Option<&str>,
        env: &[(String, String)],
    ) -> Result<u32, String> {
        let pair = native_pty_system().openpty(size).map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        // A GUI app launched from Finder has no locale; without UTF-8 tmux mangles Chinese.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("LANG", "en_US.UTF-8");
        // tmux refuses to nest when TMUX is set (the app may be started from inside tmux).
        cmd.env_remove("TMUX");
        for (k, v) in env {
            cmd.env(k, v);
        }

        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        // Keeping our copy of the slave open would stop the reader from ever seeing EOF.
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = input_writer(&*pair.master)?;

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.sessions.lock().unwrap().insert(
            id,
            Session { master: pair.master, writer, killer: child.clone_killer() },
        );

        let sessions = Arc::clone(&self.sessions);
        std::thread::spawn(move || {
            pump_output(&app, id, reader);
            let code = child.wait().ok().map(|s| s.exit_code());
            sessions.lock().unwrap().remove(&id);
            let _ = app.emit("pty-exit", PtyExit { id, code });
        });
        Ok(id)
    }

    pub fn write(&self, id: u32, data: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let s = sessions.get_mut(&id).ok_or("no such pty")?;
        s.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        s.writer.flush().map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: u32, size: PtySize) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("no such pty")?;
        s.master.resize(size).map_err(|e| e.to_string())
    }

    pub fn kill(&self, id: u32) -> Result<(), String> {
        let mut s = self.sessions.lock().unwrap().remove(&id).ok_or("no such pty")?;
        s.killer.kill().map_err(|e| e.to_string())
    }
}

/// A writer onto the PTY master that does NOT send EOF when dropped.
///
/// `MasterPty::take_writer()` returns a writer whose `Drop` types `"\n"` + the EOF character into the PTY so the
/// child sees end of input. When we close a panel the `tmux attach` client may still be alive for a moment, reads
/// that Enter + Ctrl-D as keystrokes and forwards them to the agent's pane (Ctrl-D twice quits Claude Code).
/// A plain duplicate of the master descriptor writes the same bytes and has no such `Drop`.
fn input_writer(master: &dyn MasterPty) -> Result<File, String> {
    let fd = master.as_raw_fd().ok_or("pty has no file descriptor")?;
    // SAFETY: `fd` belongs to `master`, which outlives this call; the duplicate is owned by the returned File.
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned().map_err(|e| e.to_string())?;
    Ok(File::from(owned))
}

fn pump_output(app: &AppHandle, id: u32, mut reader: Box<dyn Read + Send>) {
    let mut chunker = Utf8Chunker::default();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let data = chunker.push(&buf[..n]);
        if !data.is_empty() {
            let _ = app.emit("pty-data", PtyData { id, data });
        }
    }
    let data = chunker.finish();
    if !data.is_empty() {
        let _ = app.emit("pty-data", PtyData { id, data });
    }
}

/// Turns a byte stream read in arbitrary chunks into Strings without splitting a
/// multi-byte character: an incomplete trailing sequence is held back until the
/// next chunk completes it. Bytes that can never be valid UTF-8 become U+FFFD.
#[derive(Default)]
struct Utf8Chunker {
    pending: Vec<u8>,
}

impl Utf8Chunker {
    fn push(&mut self, bytes: &[u8]) -> String {
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(bytes);
        let mut out = String::new();
        let mut rest = &buf[..];
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    rest = &[];
                    break;
                }
                Err(e) => {
                    let (valid, after) = rest.split_at(e.valid_up_to());
                    out.push_str(&String::from_utf8_lossy(valid));
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{FFFD}');
                            rest = &after[n..];
                        }
                        None => {
                            rest = after;
                            break;
                        }
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        out
    }

    /// At end of stream, whatever is still held back is a truncated character.
    fn finish(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Closing our input side must not look like Enter + Ctrl-D to whatever is on the other end.
    /// `cat` quits on Ctrl-D at the start of a line, so it is a reliable detector.
    fn child_survives_dropping(use_input_writer: bool) -> bool {
        let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut child = pair.slave.spawn_command(CommandBuilder::new("cat")).unwrap();
        drop(pair.slave);
        // Keep reading so the child never blocks on output.
        let mut reader = pair.master.try_clone_reader().unwrap();
        std::thread::spawn(move || while matches!(reader.read(&mut [0u8; 256]), Ok(n) if n > 0) {});
        if use_input_writer {
            drop(input_writer(&*pair.master).unwrap());
        } else {
            drop(pair.master.take_writer().unwrap());
        }
        std::thread::sleep(Duration::from_millis(400));
        let alive = child.try_wait().unwrap().is_none();
        child.kill().ok();
        alive
    }

    #[test]
    fn dropping_our_writer_sends_no_eof_to_the_child() {
        assert!(child_survives_dropping(true));
    }

    #[test]
    fn the_stock_writer_does_send_eof_which_is_why_we_avoid_it() {
        assert!(!child_survives_dropping(false));
    }

    #[test]
    fn every_split_point_of_mixed_text_decodes_intact() {
        let text = "echo 你好 shifu — 😀 café\r\n";
        let bytes = text.as_bytes();
        for cut in 0..=bytes.len() {
            let mut c = Utf8Chunker::default();
            let mut out = c.push(&bytes[..cut]);
            out += &c.push(&bytes[cut..]);
            out += &c.finish();
            assert_eq!(out, text, "split at byte {cut}");
        }
    }

    #[test]
    fn one_byte_at_a_time_decodes_intact() {
        let text = "汉字输出 ok";
        let mut c = Utf8Chunker::default();
        let mut out = String::new();
        for b in text.as_bytes() {
            out += &c.push(&[*b]);
        }
        assert_eq!(out, text);
    }

    #[test]
    fn incomplete_tail_is_held_back_not_replaced() {
        let mut c = Utf8Chunker::default();
        assert_eq!(c.push(&"你".as_bytes()[..2]), "");
        assert_eq!(c.push(&"你".as_bytes()[2..]), "你");
    }

    #[test]
    fn invalid_bytes_become_replacement_and_stream_continues() {
        let mut c = Utf8Chunker::default();
        assert_eq!(c.push(b"a\xffb"), "a\u{FFFD}b");
        // A lead byte followed by a non-continuation byte is invalid, not pending.
        assert_eq!(c.push(b"\xe4\xbdx"), "\u{FFFD}x");
    }

    #[test]
    fn truncated_character_at_eof_is_flushed_as_replacement() {
        let mut c = Utf8Chunker::default();
        assert_eq!(c.push(&"你".as_bytes()[..1]), "");
        assert_eq!(c.finish(), "\u{FFFD}");
    }
}
