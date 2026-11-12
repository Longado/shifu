#!/bin/bash
set -euo pipefail

project_root=$(cd "$(dirname "$0")/.." && pwd)
remote_home=${SHIFU_REMOTE_INSTALL_HOME:-"$HOME/.shifu/remote"}
cli_target=${SHIFU_REMOTE_INSTALL_CLI:-"$HOME/.cargo/bin/shifu"}
build_dir=${CARGO_TARGET_DIR:-"$project_root/remote/target"}

if [ -x "$remote_home/bin/shifu-remote" ] && SHIFU_REMOTE_HOME="$remote_home" "$remote_home/bin/shifu-remote" status >/dev/null 2>&1; then
  echo '请先用 shifu remote stop 停止入口，再安装更新。原 Agent 不需要停止。' >&2
  exit 1
fi
if [ ! -d "$project_root/app/node_modules" ]; then npm --prefix "$project_root/app" ci; fi
npm --prefix "$project_root/app" run build:remote
cargo build --locked --release --manifest-path "$project_root/remote/Cargo.toml" --target-dir "$build_dir"
cargo build --locked --release --manifest-path "$project_root/Cargo.toml" --bin shifu --target-dir "$build_dir"

mkdir -p "$remote_home/bin" "$(dirname "$cli_target")"
chmod 700 "$remote_home"
backup="$remote_home/backups/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$backup"
chmod 700 "$backup"
if [ -f "$cli_target" ]; then cp -p "$cli_target" "$backup/shifu"; fi
if [ -f "$remote_home/bin/shifu-remote" ]; then cp -p "$remote_home/bin/shifu-remote" "$backup/shifu-remote"; fi
stage=$(mktemp -d "$remote_home/install.XXXXXX")
cp -R "$project_root/remote/web-dist" "$stage/web"
install -m 755 "$build_dir/release/shifu-remote" "$stage/shifu-remote"
cli_stage=$(mktemp "$(dirname "$cli_target")/.shifu-remote.XXXXXX")
install -m 755 "$build_dir/release/shifu" "$cli_stage"
mv -f "$cli_stage" "$cli_target"
mv -f "$stage/shifu-remote" "$remote_home/bin/shifu-remote"
if [ -d "$remote_home/web" ]; then mv "$remote_home/web" "$backup/web"; fi
mv "$stage/web" "$remote_home/web"
rmdir "$stage"
echo '手机远程入口已安装。运行 shifu remote start 启动；shifu remote code 在本机显示访问码。'
echo "上一版组件：$backup"
