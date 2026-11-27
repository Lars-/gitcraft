#!/bin/sh
# Makes the Debian package and the AppImage of a Linux release build, in the current folder.
# Usage: packaging/linux.sh <gitcraft binary> <x86_64|aarch64> <version>
set -eu
binary=$(realpath "$1")
arch=$2
version=$3
out=$PWD
cd "$(dirname "$0")/.."
case $arch in
  x86_64) deb=amd64 tool=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0 runtime=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d ;;
  aarch64) deb=arm64 tool=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158 runtime=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444 ;;
  *) echo "No packages for $arch" >&2 && exit 1 ;;
esac
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# What both install: the binary with its licences, the menu entry and the icon.
root=$tmp/root
install -Dm755 "$binary" "$root/usr/bin/gitcraft"
install -Dm644 -t "$root/usr/share/doc/gitcraft" README.md LICENSE-MIT LICENSE-APACHE
install -Dm644 packaging/gitcraft.desktop "$root/usr/share/applications/gitcraft.desktop"
install -Dm644 assets/icon/gitcraft.svg "$root/usr/share/icons/hicolor/scalable/apps/gitcraft.svg"
for size in 16 32 48 64 128 256 512; do
  install -Dm644 "assets/icon/$size.png" "$root/usr/share/icons/hicolor/${size}x$size/apps/gitcraft.png"
done

# The AppImage starts the binary itself and takes its menu entry and icon from the top folder.
cp -a "$root" "$tmp/AppDir"
ln -s usr/bin/gitcraft "$tmp/AppDir/AppRun"
ln -s usr/share/applications/gitcraft.desktop "$tmp/AppDir/gitcraft.desktop"
ln -s usr/share/icons/hicolor/256x256/apps/gitcraft.png "$tmp/AppDir/gitcraft.png"
fetch() { curl -fsSL -o "$tmp/$1" "$2" && echo "$3  $tmp/$1" | sha256sum -c --quiet; }
fetch appimagetool "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-$arch.AppImage" $tool
fetch runtime "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-$arch" $runtime
chmod +x "$tmp/appimagetool"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=$arch "$tmp/appimagetool" --runtime-file "$tmp/runtime" "$tmp/AppDir" \
  "$out/gitcraft-v$version-$arch.AppImage"

# The Debian package needs the newest glibc the binary asks for, git, and the libraries the
# window loads while it runs: EGL, and Wayland's or X11's.
glibc=$(grep -aoE 'GLIBC_[0-9.]+' "$binary" | sort -uV | tail -n 1)
mkdir "$root/DEBIAN"
(cd "$root" && find usr -type f -exec md5sum {} +) > "$root/DEBIAN/md5sums"
cat > "$root/DEBIAN/control" <<CONTROL
Package: gitcraft
Version: $version
Architecture: $deb
Maintainer: Lars Jansen <5690934+Lars-@users.noreply.github.com>
Installed-Size: $(du -sk "$root/usr" | cut -f 1)
Depends: git (>= 1:2.32), libc6 (>= ${glibc#GLIBC_}), libegl1, libxkbcommon0, libwayland-client0, libwayland-egl1, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxkbcommon-x11-0, libdbus-1-3
Section: vcs
Priority: optional
Homepage: https://github.com/Lars-/gitcraft
Description: fast graphical git client
 GitCraft shows every branch at once in a commit graph and does what you do
 with a repository day to day: stage single lines, commit, branch, merge,
 rebase, resolve conflicts, fetch, pull and push, through your own git.
CONTROL
dpkg-deb --root-owner-group -Zxz --build "$root" "$out/gitcraft-v$version-$deb.deb"
