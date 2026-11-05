#!/bin/sh
# Wraps a built binary into "MiBand Heart Rate.app".
#
# A bare binary can only use Bluetooth through the terminal that launched it;
# an app bundle gets its own permission prompt, which needs
# NSBluetoothAlwaysUsageDescription in Info.plist.
#
# Usage: scripts/bundle-macos.sh <binary> <version> <out-dir>
set -eu

bin="$1"
version="$2"
out="$3"

app="$out/MiBand Heart Rate.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/miband-heart-rate"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>MiBand Heart Rate</string>
  <key>CFBundleExecutable</key><string>miband-heart-rate</string>
  <key>CFBundleIdentifier</key><string>io.github.fosoaq.miband-heart-rate</string>
  <key>CFBundleName</key><string>MiBand Heart Rate</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSBluetoothAlwaysUsageDescription</key>
  <string>Reads the heart rate your band broadcasts over Bluetooth.</string>
</dict>
</plist>
EOF

# Ad-hoc signature: required to run on Apple silicon at all. Not notarized.
codesign --force --sign - "$app"
echo "$app"
