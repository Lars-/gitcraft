#!/bin/sh
# Makes GitCraft.app of a macOS release build, in a disk image in the current folder.
# Usage: packaging/macos.sh <gitcraft binary> <x86_64|aarch64> <version>
set -eu
binary=$(realpath "$1")
version=$3
out=$PWD
cd "$(dirname "$0")/.."
case $2 in
  x86_64) arch=x86_64 ;;
  aarch64) arch=arm64 ;;
  *) echo "No disk image for $2" >&2 && exit 1 ;;
esac
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

app=$tmp/disk/GitCraft.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$tmp/gitcraft.iconset"
cp "$binary" "$app/Contents/MacOS/gitcraft"
for size in 16 32 128 256 512; do
  cp "assets/icon/$size.png" "$tmp/gitcraft.iconset/icon_${size}x$size.png"
  cp "assets/icon/$((size * 2)).png" "$tmp/gitcraft.iconset/icon_${size}x$size@2x.png"
done
iconutil -c icns -o "$app/Contents/Resources/gitcraft.icns" "$tmp/gitcraft.iconset"
# The oldest macOS the binary was built for.
minimum=$(vtool -show-build "$binary" | awk '$1 == "minos" { print $2 }')
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>GitCraft</string>
  <key>CFBundleIdentifier</key><string>io.github.Lars-.gitcraft</string>
  <key>CFBundleExecutable</key><string>gitcraft</string>
  <key>CFBundleIconFile</key><string>gitcraft</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>$minimum</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# An ad-hoc signature, without a certificate: Apple silicon runs nothing unsigned, and it seals
# the app as a whole, so macOS can tell it is complete.
codesign --force --sign - "$app"
ln -s /Applications "$tmp/disk/Applications"
hdiutil create -quiet -volname GitCraft -srcfolder "$tmp/disk" -format UDZO "$out/gitcraft-v$version-$arch.dmg"
