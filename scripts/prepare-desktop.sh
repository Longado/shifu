#!/bin/bash
set -euo pipefail
project_root=$(cd "$(dirname "$0")/.." && pwd)
build_dir=${SHIFU_REMOTE_BUILD_DIR:-"$project_root/remote/target"}
bundle="$project_root/app/src-tauri/remote-bundle"
npm --prefix "$project_root/app" run build:remote
npm --prefix "$project_root/app" run build
cargo build --locked --release --manifest-path "$project_root/remote/Cargo.toml" --target-dir "$build_dir"
stage_root=${SHIFU_BUILD_STAGE_ROOT:-"$project_root/target/remote-bundle-staging"}
mkdir -p "$bundle" "$stage_root"
stage=$(mktemp -d "$stage_root/release.XXXXXX")
finish_stage() {
  result=$?
  trap - EXIT
  if [ "$result" -eq 0 ]; then
    rm -rf "$stage"
  else
    echo "构建未完成，保留本次暂存用于续跑：$stage" >&2
  fi
  exit "$result"
}
trap finish_stage EXIT
install -m 755 "$build_dir/release/shifu-remote" "$stage/shifu-remote"
cp -R "$project_root/remote/web-dist" "$stage/web"
if [ -d "$bundle/web" ]; then mv "$bundle/web" "$stage/previous-web"; fi
mv -f "$stage/shifu-remote" "$bundle/shifu-remote"
mv "$stage/web" "$bundle/web"
