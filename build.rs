//! One fingerprint of `src/`, compiled into the `shifu` CLI and into the desktop app (which links this crate).
//! The app's hooks call the CLI as a separate installed file; comparing fingerprints tells the two apart when
//! only one of them was replaced.

use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.unwrap_or_else(|e| panic!("read {}: {e}", dir.display())).path();
        if path.is_dir() { files(&path, out) } else { out.push(path) }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let mut all = Vec::new();
    files(&root.join("src"), &mut all);
    all.sort();
    // FNV-1a over each file's path and contents: the same source gives the same value on every machine and toolchain.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes.iter().chain([0u8].iter()) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for path in &all {
        feed(path.strip_prefix(&root).expect("under the crate").to_string_lossy().as_bytes());
        feed(&std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())));
    }
    println!("cargo:rustc-env=SHIFU_SOURCE={hash:016x}");
    println!("cargo:rerun-if-changed=src");
}
