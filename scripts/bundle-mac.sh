#!/usr/bin/env bash
# Bundle vibe-app into a macOS .app and optionally a .dmg
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BINARY="$REPO_ROOT/target/release/vibe-app"
LOGO="$REPO_ROOT/apps/desktop/assets/vibe-plus-logo.png"
OUT_DIR="$REPO_ROOT/dist/mac"
APP="$OUT_DIR/Vibe Plus.app"
BUNDLE_ID="dev.vibeplus.app"
VERSION=$(grep '^version' "$REPO_ROOT/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')

echo "→ Version: $VERSION"
echo "→ Binary:  $BINARY"

if [[ ! -f "$BINARY" ]]; then
  echo "Binary not found — run: cargo build --release -p vibe-app"
  exit 1
fi

# ── 1. Scaffold .app bundle ────────────────────────────────────────────────
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"

cp "$BINARY" "$APP/Contents/MacOS/vibe-app"
chmod +x "$APP/Contents/MacOS/vibe-app"

# ── 2. Info.plist ──────────────────────────────────────────────────────────
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>              <string>Vibe Plus</string>
  <key>CFBundleDisplayName</key>       <string>Vibe Plus</string>
  <key>CFBundleIdentifier</key>        <string>${BUNDLE_ID}</string>
  <key>CFBundleVersion</key>           <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key>        <string>vibe-app</string>
  <key>CFBundlePackageType</key>       <string>APPL</string>
  <key>CFBundleIconFile</key>          <string>AppIcon</string>
  <key>NSHighResolutionCapable</key>   <true/>
  <key>LSMinimumSystemVersion</key>    <string>11.0</string>
  <key>NSAppTransportSecurity</key>
  <dict>
    <key>NSAllowsLocalNetworking</key> <true/>
  </dict>
  <key>LSUIElement</key>               <false/>
</dict>
</plist>
PLIST

# ── 3. Generate .icns from the 512x512 PNG ────────────────────────────────
ICONSET=$(mktemp -d)/AppIcon.iconset
mkdir -p "$ICONSET"

for SIZE in 16 32 64 128 256 512; do
  sips -z $SIZE $SIZE "$LOGO" --out "$ICONSET/icon_${SIZE}x${SIZE}.png"    > /dev/null
  sips -z $((SIZE*2)) $((SIZE*2)) "$LOGO" --out "$ICONSET/icon_${SIZE}x${SIZE}@2x.png" > /dev/null
done

iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$ICONSET")"

echo "✓ .app bundle created: $APP"

# ── 4. Optional: create a .dmg (requires hdiutil, always available on macOS) ──
DMG="$OUT_DIR/VibePlus-${VERSION}-mac.dmg"
STAGING=$(mktemp -d)
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"

hdiutil create \
  -volname "Vibe Plus" \
  -srcfolder "$STAGING" \
  -ov -format UDZO \
  "$DMG" > /dev/null

rm -rf "$STAGING"

echo "✓ .dmg created:        $DMG"
echo ""
echo "Send $DMG to your friend."
echo "They drag 'Vibe Plus' to Applications and double-click."
