#!/bin/bash
set -euo pipefail
project_root=$(cd "$(dirname "$0")/.." && pwd)
bindgen_version=0.2.130
tools_dir=${SHIFU_REMOTE_TOOLS_DIR:-"$project_root/target/remote-tools"}
crypto_build_dir=${SHIFU_CRYPTO_BUILD_DIR:-"$project_root/remote/crypto/target"}
bindgen="$tools_dir/bin/wasm-bindgen"
if [ ! -x "$bindgen" ]; then
  cargo install --root "$tools_dir" --version "$bindgen_version" --locked wasm-bindgen-cli
fi
rustup target add wasm32-unknown-unknown
RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }--cfg getrandom_backend=\"wasm_js\"" cargo build --locked --manifest-path "$project_root/remote/crypto/Cargo.toml" --target-dir "$crypto_build_dir" --target wasm32-unknown-unknown --release
"$bindgen" --target web --out-dir "$project_root/remote/crypto/pkg" "$crypto_build_dir/wasm32-unknown-unknown/release/shifu_channel.wasm"
