#!/bin/sh
# Packs a built Linux binary into an AppImage.
#
# Usage: APPIMAGETOOL=/path/to/appimagetool RUNTIME=/path/to/runtime-x86_64 \\
#          scripts/appimage.sh <binary> <out-file>
# Run from the repository root.
set -eu

bin="$1"
out="$2"

appdir=$(mktemp -d)/MiBandHeartRate.AppDir
mkdir -p "$appdir/usr/bin"
cp "$bin" "$appdir/usr/bin/miband-heart-rate"
cp assets/miband-heart-rate.desktop "$appdir/"
cp assets/icon-512.png "$appdir/miband-heart-rate.png"
ln -s usr/bin/miband-heart-rate "$appdir/AppRun"

# appimagetool is itself an AppImage; CI runners have no FUSE, so let it unpack itself.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$APPIMAGETOOL" --no-appstream --runtime-file "$RUNTIME" "$appdir" "$out"
rm -rf "$(dirname "$appdir")"
