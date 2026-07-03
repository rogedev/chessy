#!/usr/bin/env bash
# Build Chessy.app and a distributable DMG from a compiled binary.
#
# Usage: packaging/macos/make_app.sh <path-to-chessy-binary> <version> [output-dir]
#
# The bundle is ad-hoc signed. Without notarization macOS still shows a
# one-time Gatekeeper warning; see the README for the "Open Anyway" steps.
set -euo pipefail

BINARY="$1"
VERSION="$2"
OUT_DIR="${3:-.}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

APP="$OUT_DIR/Chessy.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$BINARY" "$APP/Contents/MacOS/chessy"
chmod +x "$APP/Contents/MacOS/chessy"
cp -R "$REPO_ROOT/assets" "$APP/Contents/Resources/assets"
cp "$REPO_ROOT/packaging/macos/icon.icns" "$APP/Contents/Resources/icon.icns"
sed "s/APP_VERSION/${VERSION#v}/g" "$REPO_ROOT/packaging/macos/Info.plist" \
    > "$APP/Contents/Info.plist"

codesign --force --deep --sign - "$APP"

DMG="$OUT_DIR/Chessy.dmg"
STAGING="$(mktemp -d)"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
rm -f "$DMG"
hdiutil create -volname "Chessy" -srcfolder "$STAGING" -ov -format UDZO "$DMG"
rm -rf "$STAGING"

echo "Created $APP and $DMG"
