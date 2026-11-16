#!/bin/bash
set -euo pipefail
repo_dir=$(cd "$(dirname "$0")/.." && pwd)
browser_home=${SHIFU_BROWSER_HOME:-"$HOME/.shifu/browser"}
case "$browser_home" in /*) ;; *) echo 'SHIFU_BROWSER_HOME 必须是绝对路径。' >&2; exit 1;; esac
if ! command -v node >/dev/null || ! command -v npm >/dev/null; then
  echo '安装浏览器运行时需要 Node.js 20+ 和 npm；安装后不再依赖系统 PATH 中的 Node。' >&2
  exit 1
fi
node -e 'if (Number(process.versions.node.split(".")[0]) < 20) { console.error("需要 Node.js 20+"); process.exit(1); }'
node_source=$(node -p 'process.execPath')
npm --prefix "$repo_dir/browser" ci --ignore-scripts --no-audit --no-fund
mkdir -p "$browser_home" "$browser_home/backups"
chmod 700 "$browser_home" "$browser_home/backups"
stage=$(mktemp -d "$browser_home/runtime.install.XXXXXX")
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/bin" "$stage/licenses"
cp "$repo_dir/browser/package.json" "$repo_dir/browser/package-lock.json" "$stage/"
cp -R "$repo_dir/browser/node_modules" "$stage/node_modules"
cp -L "$node_source" "$stage/bin/node"
chmod 755 "$stage/bin/node"
# Keep the Node distribution notice beside the embedded runtime, including Homebrew layouts.
node - "$node_source" "$stage/licenses/Node-LICENSE" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');
const bin = fs.realpathSync(process.argv[2]);
const roots = [path.resolve(bin, '../..'), path.resolve(bin, '../../..')];
const license = roots.map(root => path.join(root, 'LICENSE')).find(file => fs.existsSync(file));
if (!license) { console.error('找不到 Node LICENSE，未安装；请使用官方 Node.js 发行包。'); process.exit(1); }
fs.copyFileSync(license, process.argv[3]);
NODE
NO_UPDATE_NOTIFIER=1 "$stage/bin/node" "$stage/node_modules/@playwright/cli/playwright-cli.js" --version --json
if [ -d "$browser_home/runtime" ]; then
  backup=$(mktemp -d "$browser_home/backups/update-$(date +%Y%m%d-%H%M%S).XXXXXX")
  mv "$browser_home/runtime" "$backup/runtime"
  if ! mv "$stage" "$browser_home/runtime"; then
    mv "$backup/runtime" "$browser_home/runtime"
    exit 1
  fi
  echo "上一版运行时：$backup"
else
  mv "$stage" "$browser_home/runtime"
fi
trap - EXIT
echo "浏览器运行时已安装：$browser_home/runtime"
echo '下一步：shifu browser doctor；shifu browser open https://example.com'
