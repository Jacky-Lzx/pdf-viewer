#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
    echo 'Run this script on macOS after cargo build --release.' >&2
    exit 1
fi
bundle=target/Pagewing.app
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/pagewing "$bundle/Contents/MacOS/pagewing"
cp assets/pagewing-guide.pdf "$bundle/Contents/Resources/pagewing-guide.pdf"
iconset=target/pagewing.iconset
mkdir -p "$iconset"
# macOS scales the whole canvas in the Dock; inset the artwork to match other apps.
# Regenerate the macOS asset from the shared vector with:
# rsvg-convert -w 824 -h 824 --page-width 1024 --page-height 1024 \
#     --left 100 --top 100 assets/icon.svg -o assets/icon-macos.png
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" assets/icon-macos.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" assets/icon-macos.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns -o "$bundle/Contents/Resources/Pagewing.icns" "$iconset"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>pagewing</string>
  <key>CFBundleIdentifier</key><string>dev.pagewing.reader</string>
  <key>CFBundleName</key><string>Pagewing</string>
  <key>CFBundleDisplayName</key><string>Pagewing</string>
  <key>CFBundleIconFile</key><string>Pagewing</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
</dict></plist>
PLIST
codesign --force --deep --sign - "$bundle"
echo "Built $bundle (locally signed; public distribution requires notarization)."
