#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin ]]; then
    echo 'Run this script on macOS after cargo build --release.' >&2
    exit 1
fi
bundle=target/Folio.app
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp target/release/folio "$bundle/Contents/MacOS/folio"
cp assets/folio-guide.pdf "$bundle/Contents/Resources/folio-guide.pdf"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>folio</string>
  <key>CFBundleIdentifier</key><string>dev.folio.reader</string>
  <key>CFBundleName</key><string>Folio</string>
  <key>CFBundleDisplayName</key><string>Folio</string>
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
