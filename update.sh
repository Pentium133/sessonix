#!/bin/bash
set -e

APP_NAME="Sessonix"
BUNDLE_PATH="src-tauri/target/release/bundle/macos/${APP_NAME}.app"
INSTALL_PATH="/Applications/${APP_NAME}.app"

export PATH="$HOME/.cargo/bin:$PATH"

# --- DMG mode: build + package DMG ---
if [ "${1:-}" = "dmg" ]; then
  echo "=== Building DMG ==="
  npm run tauri build -- --bundles app 2>&1 | grep -E '(Compiling sessonix|Finished|Bundling|Built|Error|error)'
  bash scripts/build-dmg.sh
  exit 0
fi

# Read current version from package.json
VERSION=$(grep '"version"' package.json | head -1 | sed 's/.*"\([0-9]*\.[0-9]*\.[0-9]*\)".*/\1/')

echo "=== Building ${APP_NAME} v${VERSION} ==="
npm run tauri build -- --bundles app 2>&1 | grep -E '(Compiling sessonix|Finished|Bundling|Built|Error|error)'

if [ ! -d "$BUNDLE_PATH" ]; then
  echo "ERROR: Build failed, no .app bundle found"
  exit 1
fi

# Kill running instance if any
if pgrep -x "$APP_NAME" > /dev/null 2>&1; then
  echo "=== Stopping running ${APP_NAME} ==="
  pkill -x "$APP_NAME" || true
  sleep 1
fi

echo "=== Installing to /Applications ==="
rm -rf "$INSTALL_PATH"
cp -r "$BUNDLE_PATH" "$INSTALL_PATH"
xattr -rd com.apple.quarantine "$INSTALL_PATH" 2>/dev/null || true

echo "=== Launching ${APP_NAME} ==="
open "$INSTALL_PATH"

echo "=== Done: ${APP_NAME} v${VERSION} ==="
