#!/bin/bash
# Build a DMG from the .app bundle produced by `tauri build --bundles app`.
# Workaround for Tauri's built-in DMG script failing on macOS Tahoe (26.x).
set -euo pipefail

VERSION=$(grep '"version"' src-tauri/tauri.conf.json | head -1 | sed 's/.*: "//;s/".*//')
ARCH=$(uname -m)
APP="src-tauri/target/release/bundle/macos/Sessonix.app"
DMG_DIR="src-tauri/target/release/bundle/dmg"
DMG="$DMG_DIR/Sessonix_${VERSION}_${ARCH}.dmg"

if [ ! -d "$APP" ]; then
  echo "Error: $APP not found. Run 'npm run tauri build -- --bundles app' first."
  exit 1
fi

mkdir -p "$DMG_DIR"
TMPDIR=$(mktemp -d)
cp -R "$APP" "$TMPDIR/"
ln -s /Applications "$TMPDIR/Applications"
hdiutil create -volname "Sessonix" -srcfolder "$TMPDIR" -ov -format UDZO "$DMG"
rm -rf "$TMPDIR"

echo ""
echo "DMG created: $DMG ($(du -h "$DMG" | cut -f1))"
