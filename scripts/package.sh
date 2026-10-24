#!/bin/bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DIR"

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d '"' -f2)
echo "=== Packaging Yegz v${VERSION} ==="

DIST="$DIR/dist"
rm -rf "$DIST"
mkdir -p "$DIST"

echo "1. Building for Apple Silicon (aarch64-apple-darwin)..."
cargo build --release --target aarch64-apple-darwin

echo "2. Building for Intel Mac (x86_64-apple-darwin)..."
cargo build --release --target x86_64-apple-darwin

echo "3. Creating Universal 2 fat binary with lipo..."
mkdir -p target/universal
lipo -create \
  target/aarch64-apple-darwin/release/yegz \
  target/x86_64-apple-darwin/release/yegz \
  -output target/universal/yegz
chmod +x target/universal/yegz
lipo -info target/universal/yegz

echo "4. Creating Yegz.app bundle..."
APP="$DIST/Yegz.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"

cp target/universal/yegz "$APP/Contents/MacOS/yegz"
chmod +x "$APP/Contents/MacOS/yegz"
cp scripts/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Yegz</string>
    <key>CFBundleDisplayName</key>
    <string>Yegz</string>
    <key>CFBundleIdentifier</key>
    <string>com.yegz.editor</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleSignature</key>
    <string>????</string>
    <key>CFBundleExecutable</key>
    <string>yegz</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIconName</key>
    <string>AppIcon</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
    <key>NSAutomaticTouchBarItemsEnabled</key>
    <false/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key>
            <string>Text Document</string>
            <key>CFBundleTypeRole</key>
            <string>Editor</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.plain-text</string>
                <string>public.text</string>
                <string>public.source-code</string>
                <string>public.data</string>
                <string>public.item</string>
                <string>public.image</string>
                <string>com.adobe.pdf</string>
            </array>
        </dict>
    </array>
</dict>
</plist>
EOF

echo "5. Ad-hoc codesigning Yegz.app..."
codesign --force --deep --sign - "$APP"

echo "6. Creating DMG installer..."
DMG_STAGING="$DIST/dmg_staging"
mkdir -p "$DMG_STAGING"
cp -R "$APP" "$DMG_STAGING/"
ln -s /Applications "$DMG_STAGING/Applications"
DMG_OUT="$DIST/Yegz-${VERSION}-macos-universal.dmg"
rm -f "$DMG_OUT"
hdiutil create -volname "Yegz" -srcfolder "$DMG_STAGING" -ov -format UDZO "$DMG_OUT"
rm -rf "$DMG_STAGING"

echo "7. Creating ZIP package..."
(cd "$DIST" && zip -r -y "Yegz-${VERSION}-macos-universal.zip" "Yegz.app")

echo "8. Creating standalone CLI tarball..."
CLI_STAGING="$DIST/cli_staging"
mkdir -p "$CLI_STAGING"
cp target/universal/yegz "$CLI_STAGING/"
cp bin/yegz "$CLI_STAGING/yegz-launcher"
cp README.md LICENSE "$CLI_STAGING/"
tar -czf "$DIST/yegz-${VERSION}-macos-universal-cli.tar.gz" -C "$CLI_STAGING" .
rm -rf "$CLI_STAGING"

echo "9. Generating SHA256 checksums..."
(cd "$DIST" && shasum -a 256 *.dmg *.zip *.tar.gz > SHA256SUMS.txt)

echo "=== Packaging Complete! ==="
ls -lh "$DIST"
cat "$DIST/SHA256SUMS.txt"
