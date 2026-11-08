#!/bin/sh
# Builds the release binary and refreshes the double-clickable .app bundle.
set -e
cd "$(dirname "$0")"
cargo build --release
APP="Gitup Launcher.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/gitup-launcher "$APP/Contents/MacOS/gitup-launcher"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
# The bundle's description, so the Finder can open it as an app.
cat > "$APP/Contents/Info.plist" <<'PLIST'
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
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
xattr -cr "$APP"
codesign --force --sign - "$APP"
echo "Built $APP"
