#!/usr/bin/env bash
set -euo pipefail

# Build script for KleKit macOS DMG
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PATH="$PROJECT_ROOT/target/release/bundle/macos/KleKit.app"
DMG_DIR="$PROJECT_ROOT/target/release/bundle/dmg"
DMG_PATH="$DMG_DIR/KleKit_0.1.0_aarch64.dmg"
VOLUME_NAME="KleKit"
ICON_PATH="$PROJECT_ROOT/src-tauri/icons/icon.icns"

echo "=== KleKit DMG Builder ==="

NO_BUILD=0
for arg in "$@"; do
    if [[ "$arg" == "--no-build" ]]; then
        NO_BUILD=1
    fi
done

if [[ $NO_BUILD -eq 0 || ! -d "$APP_PATH" ]]; then
    echo "Building release bundle with Tauri..."
    cd "$PROJECT_ROOT"
    npx @tauri-apps/cli build
fi

mkdir -p "$DMG_DIR"
rm -f "$DMG_PATH"

STAGING_DIR=$(mktemp -d -t klekit_dmg_staging_XXXXXX)
trap 'rm -rf "$STAGING_DIR"' EXIT

echo "Copying KleKit.app to staging..."
cp -R "$APP_PATH" "$STAGING_DIR/"

echo "Creating /Applications link..."
ln -s /Applications "$STAGING_DIR/Applications"

if [[ -f "$ICON_PATH" ]]; then
    cp "$ICON_PATH" "$STAGING_DIR/.VolumeIcon.icns"
fi

echo "Creating DMG with hdiutil..."
TEMP_DMG="$(mktemp -u /tmp/klekit_temp_XXXXXX).dmg"
hdiutil create -volname "$VOLUME_NAME" -srcfolder "$STAGING_DIR" -ov -format UDRW "$TEMP_DMG"

# Mount temporarily to mark volume icon attribute if icon exists
if [[ -f "$ICON_PATH" ]]; then
    MOUNT_DIR=$(mktemp -d -t klekit_mount_XXXXXX)
    hdiutil attach "$TEMP_DMG" -mountpoint "$MOUNT_DIR" -nobrowse -quiet
    SetFile -a C "$MOUNT_DIR" 2>/dev/null || true
    hdiutil detach "$MOUNT_DIR" -quiet || hdiutil detach "$MOUNT_DIR" -force -quiet
    rmdir "$MOUNT_DIR" 2>/dev/null || true
fi

echo "Compressing into final read-only DMG (UDZO)..."
hdiutil convert "$TEMP_DMG" -format UDZO -imagekey zlib-level=9 -o "$DMG_PATH"
rm -f "$TEMP_DMG"

DMG_SIZE=$(du -h "$DMG_PATH" | cut -f1)
echo "=== Successfully built DMG ==="
echo "Path: $DMG_PATH"
echo "Size: $DMG_SIZE"
