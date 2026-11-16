#!/usr/bin/env bash
# Run on Ubuntu 24.04 x86_64 with the repository's Rust toolchain installed.
# sudo apt install build-essential pkg-config libgtk-4-dev librsvg2-dev librsvg2-bin \
#   curl ca-certificates file patchelf desktop-file-utils squashfs-tools python3
set -euo pipefail
cd "$(dirname "$0")/../.."
[[ $(uname -m) == x86_64 ]] || { echo 'Only x86_64 is supported' >&2; exit 1; }
source /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 24.04 ]] || {
    echo 'Build releases on Ubuntu 24.04 to preserve the documented glibc baseline' >&2
    exit 1
}
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
root="$PWD"
build="$root/target/appimage"
appdir="$build/AppDir"
mkdir -p "$build" "$root/dist"
# A reused AppDir could silently retain old libraries or resources.
[[ ! -e "$appdir" ]] || { echo "Use a clean target/appimage/AppDir before packaging" >&2; exit 1; }
tool="$build/linuxdeploy-x86_64.AppImage"
if [[ ! -f "$tool" ]]; then
    curl --fail --location --retry 3 \
        https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage \
        --output "$tool"
fi
# Fail closed if the upstream continuous release changes. Review before updating.
echo "df908b19f1ed1cdafa79508d415aa12d6c56ce4d56d4149aa8c09286718706e5  $tool" | sha256sum --check
chmod +x "$tool"
runtime="$build/runtime-x86_64"
if [[ ! -f "$runtime" ]]; then
    curl --fail --location --retry 3 \
        https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64 \
        --output "$runtime"
fi
echo "617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46  $runtime" | sha256sum --check
# The offline notice collector enumerates dependencies for all target platforms.
# A native cargo build alone does not populate that complete registry cache.
cargo fetch --locked
cargo build --workspace --release --locked
mkdir -p "$appdir/usr/share/glib-2.0/schemas" "$appdir/usr/lib" "$appdir/usr/share/doc/ncatimage"
cp /usr/share/glib-2.0/schemas/*.xml "$appdir/usr/share/glib-2.0/schemas/"
glib-compile-schemas "$appdir/usr/share/glib-2.0/schemas"
libdir=$(pkg-config --variable=libdir gdk-pixbuf-2.0)
cp -a "$libdir/gdk-pixbuf-2.0" "$appdir/usr/lib/"
# Resolve relative loader names through AppRun's LD_LIBRARY_PATH.
"$libdir/gdk-pixbuf-2.0/gdk-pixbuf-query-loaders" | \
    sed "s|$libdir/gdk-pixbuf-2.0/2.10.0/loaders/||g" \
    > "$appdir/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
mkdir -p "$appdir/usr/share/icons"
cp -a /usr/share/icons/Adwaita /usr/share/icons/hicolor "$appdir/usr/share/icons/"
cp README.md "$appdir/usr/share/doc/ncatimage/"
cp packaging/linux/runtime-LICENSE "$appdir/usr/share/doc/ncatimage/APPIMAGE-RUNTIME-LICENSE"
python3 packaging/linux/collect-notices.py "$appdir/usr/share/doc/ncatimage"
{
    printf 'nCatImage %s\nBuild base: Ubuntu 24.04 x86_64\n' "$version"
    rustc --version
    cargo --version
    printf 'GTK: '
    pkg-config --modversion gtk4
    dpkg-query -W -f='${Package} ${Version}\n'
} > "$appdir/usr/share/doc/ncatimage/BUILD-INFO.txt"
chmod +x packaging/linux/AppRun
rsvg-convert packaging/linux/ncatimage.svg --output "$build/ncatimage.png"
export APPIMAGE_EXTRACT_AND_RUN=1
export LDAI_OUTPUT="$root/dist/nCatImage-$version-x86_64.AppImage"
export LDAI_RUNTIME_FILE="$runtime"
export ARCH=x86_64
"$tool" --appdir "$appdir" \
    --executable target/release/ncat-linux --executable target/release/ncat-worker \
    --deploy-deps-only "$appdir/usr/lib" \
    --library "$(pkg-config --variable=libdir librsvg-2.0)/librsvg-2.so.2" \
    --desktop-file packaging/linux/ncatimage.desktop \
    --icon-file "$build/ncatimage.png" --custom-apprun packaging/linux/AppRun \
    --output appimage
cd "$root/dist"
sha256sum "nCatImage-$version-x86_64.AppImage" > SHA256SUMS
