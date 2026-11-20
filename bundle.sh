#!/bin/sh
# Builds the release binary and refreshes the double-clickable .app bundle.
set -e
cd "$(dirname "$0")"
# Panic messages carry the path of the source file that raised them, and
# dependencies are compiled from this machine's home folder. The paths are
# rewritten here so a released binary never spells out where it was built.
VERSION=$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)
# Cargo splits RUSTFLAGS on whitespace, and this checkout lives under a folder
# whose name contains a space ("Craft Anything"), which truncates the flag and
# makes rustc reject it. The encoded form is unit-separator delimited and keeps
# the spaces intact.
SEP=$(printf '\037')
CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME/.cargo=/cargo${SEP}--remap-path-prefix=$HOME=/home${SEP}--remap-path-prefix=$(pwd)=/src"
export CARGO_ENCODED_RUSTFLAGS
unset RUSTFLAGS
cargo build --release
APP="Gitup Launcher.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/gitup-launcher "$APP/Contents/MacOS/gitup-launcher"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
# The bundle's description, so the Finder can open it as an app.
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Gitup Launcher</string>
  <key>CFBundleDisplayName</key><string>Gitup Launcher</string>
  <key>CFBundleIdentifier</key><string>app.gitup.launcher</string>
  <key>CFBundleExecutable</key><string>gitup-launcher</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
xattr -cr "$APP"
codesign --force --sign - "$APP"
echo "Built $APP (v${VERSION})"
