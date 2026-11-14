#!/bin/bash
set -euo pipefail
project_root=$(cd "$(dirname "$0")/.." && pwd)
if [ "$(uname -s)" != Darwin ]; then echo '请在目标架构的Mac上打包。' >&2; exit 1; fi
npm --prefix "$project_root/app" run build:remote
cargo build --locked --release --manifest-path "$project_root/remote/Cargo.toml"
cargo build --locked --release --manifest-path "$project_root/Cargo.toml" --bin shifu --target-dir "$project_root/remote/target"
mkdir -p "$project_root/target/remote-distribution"
stage=$(mktemp -d "$project_root/target/remote-distribution/payload.XXXXXX")
mkdir "$stage/bin"
install -m 755 "$project_root/remote/target/release/shifu" "$stage/bin/shifu"
install -m 755 "$project_root/remote/target/release/shifu-remote" "$stage/bin/shifu-remote"
cp -R "$project_root/remote/web-dist" "$stage/web"
cp "$project_root/scripts/install-remote.sh" "$stage/install.sh"
cp "$project_root/docs/REMOTE-RELAY.md" "$stage/使用说明.md"
printf '%s\n' "$(uname -s)-$(uname -m)" > "$stage/PLATFORM"
archive="$project_root/target/remote-distribution/shifu-remote-macos-$(uname -m)-$(date +%Y%m%d-%H%M%S).tar.gz"
tar -czf "$archive" -C "$stage" .
echo "$archive"
