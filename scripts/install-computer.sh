#!/bin/bash
set -euo pipefail
repo_dir=$(cd "$(dirname "$0")/.." && pwd)
if [ "$(uname -s)" != Darwin ]; then echo 'Computer Use 辅助程序目前只支持 macOS。' >&2; exit 1; fi
bash "$repo_dir/native/computer-use-macos/build-app.sh"
source_app="$repo_dir/target/computer-use.noindex/Shifu Computer Use.app"
install_dir="$HOME/Applications"
target_app="$install_dir/Shifu Computer Use.app"
mkdir -p "$install_dir" "$HOME/.shifu/computer/backups"
chmod 700 "$HOME/.shifu/computer" "$HOME/.shifu/computer/backups"
stage=$(mktemp -d "$install_dir/.shifu-computer-install.XXXXXX")
trap 'rm -rf "$stage"' EXIT
ditto "$source_app" "$stage/Shifu Computer Use.app"
/usr/bin/codesign --verify --deep --strict "$stage/Shifu Computer Use.app"
if [ -d "$target_app" ]; then
  cli="${SHIFU_CLI:-$HOME/.cargo/bin/shifu}"
  "$cli" computer stop
  backup=$(mktemp -d "$HOME/.shifu/computer/backups/update-$(date +%Y%m%d-%H%M%S).XXXXXX")
  mv "$target_app" "$backup/Shifu Computer Use.app"
  if ! mv "$stage/Shifu Computer Use.app" "$target_app"; then
    mv "$backup/Shifu Computer Use.app" "$target_app"
    exit 1
  fi
  echo "上一版：$backup"
else
  mv "$stage/Shifu Computer Use.app" "$target_app"
fi
echo "已安装：$target_app"
echo '下一步：shifu computer doctor；需要授权时运行 shifu computer permissions，由本人操作系统设置。'
