#!/usr/bin/env bash
# Builds target/Spork.app (release): double-click it, or drag it to /Applications.
# shortcut: signed ad hoc only (runs on the Mac that built it); a Developer ID signature,
# notarization and a .dmg come with P7 installers.
set -euo pipefail
# the monorepo, whose workspace builds Spork
cd "$(dirname "$0")/../.."
cargo build --release -p spork
app=target/Spork.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/spork "$app/Contents/MacOS/Spork"
cp crates/spork/icon/Spork.icns "$app/Contents/Resources/"
# a release passes its tag's version (script/package-release); a local package uses the crate's
version=${SPORK_VERSION:-$(cargo pkgid -p spork | sed 's/.*[#@]//')}
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Spork</string>
  <key>CFBundleDisplayName</key><string>Spork</string>
  <key>CFBundleIdentifier</key><string>org.spork.Spork</string>
  <key>CFBundleExecutable</key><string>Spork</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>Spork</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>CFBundleShortVersionString</key><string>${version%%-*}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --sign - "$app"
echo "built $app"
