#!/bin/bash
set -euo pipefail
umask 022
project_root=$(cd "$(dirname "$0")/.." && pwd)
if [ "$(uname -s)-$(uname -m)" != Linux-x86_64 ]; then
  echo '请在Linux x86_64构建环境中运行；Mac包不能安装到crew-room。' >&2
  exit 1
fi
if [ ! -f "$project_root/remote/web-dist/index.html" ]; then
  echo '请先在开发环境运行 npm --prefix app run build:remote，再把网页产物一起带入Linux构建环境。' >&2
  exit 1
fi
build_dir=${CARGO_TARGET_DIR:-"$project_root/remote/target"}
cargo build --locked --release --manifest-path "$project_root/remote/Cargo.toml" --target-dir "$build_dir"
mkdir -p "$project_root/target/remote-distribution"
stage=$(mktemp -d "$project_root/target/remote-distribution/linux.XXXXXX")
chmod 755 "$stage"
mkdir "$stage/bin"
install -m 755 "$build_dir/release/shifu-remote" "$stage/bin/shifu-remote"
cp -R "$project_root/remote/web-dist" "$stage/web"
cp -R "$project_root/remote/deploy" "$stage/deploy"
cp "$project_root/remote/deploy/README.md" "$stage/使用说明.md"
printf '%s\n' 'Linux-x86_64' > "$stage/PLATFORM"
revision=${SHIFU_REMOTE_BUILD_REVISION:-unknown}
if [[ ! "$revision" =~ ^([0-9a-f]{40}|unknown)$ ]]; then
  echo 'SHIFU_REMOTE_BUILD_REVISION必须是完整Git提交号。' >&2
  exit 1
fi
dirty=${SHIFU_REMOTE_BUILD_DIRTY:-unknown}
if [[ ! "$dirty" =~ ^([01]|unknown)$ ]]; then
  echo 'SHIFU_REMOTE_BUILD_DIRTY必须是0或1。' >&2
  exit 1
fi
minimum_glibc=$(readelf --version-info "$stage/bin/shifu-remote" | sed -n 's/.*Name: GLIBC_\([0-9.]*\).*/\1/p' | sort -V | tail -n 1)
printf 'platform=Linux-x86_64\nsource_revision=%s\nsource_dirty=%s\nrustc_version=%s\nminimum_glibc=%s\n' \
  "$revision" "$dirty" "$(rustc --version)" "${minimum_glibc:-not-recorded}" > "$stage/BUILD-INFO"
(cd "$stage" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
archive="$project_root/target/remote-distribution/shifu-relay-linux-x86_64-$(date +%Y%m%d-%H%M%S).tar.gz"
tar -czf "$archive" -C "$stage" .
echo "$archive"
