//! Jev's judgement log (APP-SPEC-C1). Shifu only reads the end of it; whether a turn was a report or a question is
//! decided in `src/jev.ts`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A line is about 200 bytes, so this covers the last few hundred turns.
const TAIL_BYTES: u64 = 64 * 1024;

/// `SHIFU_JEV_LOG` points the acceptance runs at a fake log; without it the log is JevDesk's own.
fn log_path() -> PathBuf {
    match std::env::var("SHIFU_JEV_LOG").ok().filter(|p| !p.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Library/Application Support/JevDesk/waiting-log.jsonl"),
    }
}

/// The last `TAIL_BYTES` of the file, from the start of a line. `None` when the file is missing or unreadable:
/// there is no verdict to wait for then.
pub fn tail_of(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    if start == 0 {
        return Some(text.into_owned());
    }
    // Cut in the middle of a line (or of a character): drop up to the first newline.
    Some(text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default())
}

pub fn log_tail() -> Option<String> {
    tail_of(&log_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("shifu-jev-{}-{name}", std::process::id()))
    }

    #[test]
    fn missing_file_has_no_tail() {
        assert_eq!(tail_of(&temp("missing")), None);
    }

    #[test]
    fn small_file_is_returned_whole() {
        let path = temp("small");
        std::fs::write(&path, "a\nb\n").unwrap();
        assert_eq!(tail_of(&path).as_deref(), Some("a\nb\n"));
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn large_file_is_cut_at_a_line_start() {
        let path = temp("large");
        let line = format!("{}\n", "x".repeat(99));
        std::fs::write(&path, line.repeat(2000)).unwrap();
        let tail = tail_of(&path).unwrap();
        assert!(tail.len() < TAIL_BYTES as usize);
        assert!(tail.lines().all(|l| l.len() == 99), "every line is whole");
        std::fs::remove_file(&path).unwrap();
    }
}
