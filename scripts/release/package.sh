#!/bin/bash
# Build the Apple Silicon download package for a release tag, e.g. remote-v0.3.8-preview.1.
# Output: target/release-package/Shifu-remote-v<version>-apple-silicon.zip and SHA256SUMS.
set -euo pipefail
tag=${1:?usage: package.sh <tag, e.g. remote-v0.3.8-preview.1>}
version=$(printf '%s' "$tag" | sed -E 's/^remote-v([0-9]+\.[0-9]+\.[0-9]+).*/\1/')
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "tag must look like remote-v1.2.3-...: $tag" >&2; exit 1; }
[ "$(uname -s)-$(uname -m)" = Darwin-arm64 ] || { echo 'Build on an Apple Silicon Mac.' >&2; exit 1; }
root=$(cd "$(dirname "$0")/../.." && pwd)
name="Shifu-remote-v$version-apple-silicon"
out="$root/target/release-package"
pkg="$out/$name"
rm -rf "$pkg" "$out/$name.zip" "$out/SHA256SUMS"
mkdir -p "$pkg/bin" "$pkg/helpers" "$pkg/scripts" "$pkg/docs"

# Desktop app (prepare-desktop.sh, run by tauri, builds the frontend and the bundled remote relay).
npm --prefix "$root/app" run build:desktop -- --config "{\"version\":\"$version\"}"
cache=$(dirname "$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)")/target/shifu-build-cache.noindex
app=$(ls -dt "$cache"/desktop-*/release/bundle/macos/Shifu.app 2>/dev/null | head -1)
[ -n "$app" ] || { echo 'Shifu.app not found after build' >&2; exit 1; }
ditto "$app" "$pkg/Shifu.app"
[ "$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$pkg/Shifu.app/Contents/Info.plist")" = "$version" ]

# Matching CLI and optional Computer Use helper (ad-hoc signed, not notarized).
cargo build --locked --release --manifest-path "$root/Cargo.toml" --bin shifu
install -m 755 "$root/target/release/shifu" "$pkg/bin/shifu"
SHIFU_SIGNING_IDENTITY=- bash "$root/native/computer-use-macos/build-app.sh" >/dev/null
ditto "$root/target/computer-use.noindex/Shifu Computer Use.app" "$pkg/helpers/Shifu Computer Use.app"

cp "$root/scripts/autopilot.py" "$pkg/scripts/"
for doc in AUTOPILOT BROWSER-USE BUILD-CACHE COMPUTER-USE INSTALL INSTALL.en REMOTE-RELAY REMOTE-RELAY.en STEPFUN WEB-POOL; do
  cp "$root/docs/$doc.md" "$pkg/docs/"
done
for lang in zh-CN en; do sed "s/{{VERSION}}/$version/g" "$root/scripts/release/INSTALL.$lang.md" > "$pkg/INSTALL.$lang.md"; done
cp "$root/LICENSE" "$root/THIRD_PARTY_NOTICES.md" "$pkg/"

commit=$(git -C "$root" rev-parse HEAD)
built_by=local
[ -n "${GITHUB_RUN_ID:-}" ] && built_by="GitHub Actions run $GITHUB_RUN_ID"
cat > "$pkg/BUILD-INFO.json" <<JSON
{
  "release_version": "$version",
  "release_tag": "$tag",
  "source_commit": "$commit",
  "source_tree": "$(git -C "$root" rev-parse 'HEAD^{tree}')",
  "platform": "macOS-arm64",
  "signature": "ad-hoc",
  "notarized": false,
  "built_by": "$built_by",
  "built_at_utc": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "included": ["Shifu.app", "bin/shifu", "helpers/Shifu Computer Use.app", "scripts/autopilot.py", "public instructions and licenses"],
  "external_runtimes": ["tmux", "chosen agents", "Node/Pi for StepFun", "browser runtime for Browser Use"]
}
JSON

(cd "$pkg" && find . -type f ! -name SHA256SUMS | sed 's#^\./##' | LC_ALL=C sort | tr '\n' '\0' | xargs -0 shasum -a 256 > SHA256SUMS)
(cd "$out" && ditto -c -k --keepParent --norsrc "$name" "$name.zip" && shasum -a 256 "$name.zip" > SHA256SUMS)
echo "$out/$name.zip"
