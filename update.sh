#!/bin/bash
set -e

APP_NAME="Sessonix"
BUNDLE_PATH="src-tauri/target/release/bundle/macos/${APP_NAME}.app"
INSTALL_PATH="/Applications/${APP_NAME}.app"

export PATH="$HOME/.cargo/bin:$PATH"

# --- Version bump (patch by default, pass "minor" or "major" as $1) ---
# --- DMG mode: build + package DMG without version bump ---
if [ "${1:-}" = "dmg" ]; then
  echo "=== Building DMG ==="
  npm run tauri build -- --bundles app 2>&1 | grep -E '(Compiling sessonix|Finished|Bundling|Built|Error|error)'
  bash scripts/build-dmg.sh
  exit 0
fi

BUMP_TYPE="${1:-patch}"

# Read current version from package.json
CURRENT=$(grep '"version"' package.json | head -1 | sed 's/.*"\([0-9]*\.[0-9]*\.[0-9]*\)".*/\1/')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

case "$BUMP_TYPE" in
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  patch) PATCH=$((PATCH + 1)) ;;
  *) echo "Usage: $0 [patch|minor|major]"; exit 1 ;;
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
echo "=== Version: ${CURRENT} → ${NEW_VERSION} (${BUMP_TYPE}) ==="

# Update all three version files
sed -i '' "s/\"version\": \"${CURRENT}\"/\"version\": \"${NEW_VERSION}\"/" package.json
sed -i '' "s/\"version\": \"${CURRENT}\"/\"version\": \"${NEW_VERSION}\"/" src-tauri/tauri.conf.json
sed -i '' "s/^version = \"${CURRENT}\"/version = \"${NEW_VERSION}\"/" src-tauri/Cargo.toml

# Update Cargo.lock to match
(cd src-tauri && cargo update -p sessonix --precise "$NEW_VERSION" 2>/dev/null || cargo generate-lockfile 2>/dev/null || true)

echo "=== Building ${APP_NAME} v${NEW_VERSION} ==="
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

echo "=== Launching ${APP_NAME} ==="
open "$INSTALL_PATH"

echo "=== Done: ${APP_NAME} v${NEW_VERSION} ==="
