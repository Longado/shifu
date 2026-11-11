#!/bin/bash
set -euo pipefail
package_dir="$(cd "$(dirname "$0")" && pwd)"
repo_dir="$(cd "$package_dir/../.." && pwd)"
swift build --package-path "$package_dir" -c release
bin_dir="$(swift build --package-path "$package_dir" -c release --show-bin-path)"
app_dir="$repo_dir/target/computer-use/Shifu Computer Use.app"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
cp "$bin_dir/orca-computer-use-macos" "$app_dir/Contents/MacOS/shifu-computer-use-macos"
cp "$repo_dir/app/src-tauri/icons/icon.icns" "$app_dir/Contents/Resources/icon.icns"
cp "$package_dir/LICENSE" "$app_dir/Contents/Resources/Orca-LICENSE"
cat > "$app_dir/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.longado.shifu.computer-use</string>
<key>CFBundleExecutable</key><string>shifu-computer-use-macos</string>
<key>CFBundleName</key><string>Shifu Computer Use</string>
<key>CFBundleDisplayName</key><string>Shifu Computer Use</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>LSUIElement</key><true/>
<key>NSScreenCaptureUsageDescription</key><string>让 Shifu 中的 Agent 查看你指定的应用窗口。</string>
</dict></plist>
PLIST
signing_identity="${SHIFU_SIGNING_IDENTITY:-$(/usr/bin/plutil -extract bundle.macOS.signingIdentity raw "$repo_dir/app/src-tauri/tauri.conf.json")}"
/usr/bin/codesign --force --deep --sign "$signing_identity" "$app_dir"
/usr/bin/codesign --verify --deep --strict "$app_dir"
printf '%s\n' "$app_dir"
