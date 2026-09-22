#!/usr/bin/env bash
# Build libquill.so for the reMarkable 2 without the vendor SDK.
#
# quill/build.sh needs the reMarkable Yocto SDK (Qt headers plus a cross gcc).
# This path needs only zig, a Debian armhf qt6-base-dev package for the Qt
# headers, and three libraries pulled from your own tablet as link inputs:
# libQt6Core, libQt6Gui and the proprietary libqsgepaper.so. Everything pulled
# from the device stays under quill/vendor/ and is never redistributed.
#
#   ./quill/build-zig.sh                       # pulls from ssh host rm2 when missing
#   QUILL_DEVICE_HOST=root@10.11.99.1 ./quill/build-zig.sh
#
# Headers from a Qt minor older than the tablet's runtime are fine: quill only
# touches public QImage/QRect/QCoreApplication API, and Qt keeps that ABI
# stable across minor versions (6.8 headers were used against the 6.10 runtime
# of OS 3.28). Set QT6_DEB to a downloaded .deb to skip the fetch.
set -euo pipefail
cd "$(dirname "$0")"

TARGET=armv7-unknown-linux-gnueabihf
VENDOR="vendor/$TARGET"
OUT="build/$TARGET"
HDR="$VENDOR/qt6-headers"
HOST="${QUILL_DEVICE_HOST:-rm2}"
SCP="${QUILL_SCP:-/usr/bin/scp -O}"
DEB_URL="${QT6_DEB_URL:-https://deb.debian.org/debian/pool/main/q/qt6-base/qt6-base-dev_6.8.2+dfsg-9+deb13u2_armhf.deb}"

command -v zig >/dev/null || { echo "zig not found — brew install zig" >&2; exit 1; }
mkdir -p "$VENDOR" "$OUT"

# 1. Qt headers (the armhf package carries the 32-bit qconfig.h, which is what
#    makes QT_POINTER_SIZE and the QImage constructor ABI come out right).
if [ ! -f "$HDR/qt6/QtCore/qconfig.h" ]; then
  deb="${QT6_DEB:-$VENDOR/qt6-base-dev_armhf.deb}"
  [ -f "$deb" ] || curl -fsSL -o "$deb" "$DEB_URL"
  deb_abs="$(cd "$(dirname "$deb")" && pwd)/$(basename "$deb")"
  work=$(mktemp -d)
  ( cd "$work" && ar x "$deb_abs" && tar -xf data.tar.* )
  mkdir -p "$HDR"
  cp -R "$work/usr/include/arm-linux-gnueabihf/qt6" "$HDR/"
  rm -rf "$work"
fi

# 2. Tablet libraries. scp follows the .so.6 symlinks, so the copies are the
#    real libraries under their SONAME.
pull() {
  [ -f "$VENDOR/$2" ] && return
  echo "pulling $1 from $HOST..."
  $SCP "$HOST:$1" "$VENDOR/$2"
}
pull /usr/lib/plugins/scenegraph/libqsgepaper.so libqsgepaper.so
pull /usr/lib/libQt6Core.so.6 libQt6Core.so.6
pull /usr/lib/libQt6Gui.so.6 libQt6Gui.so.6

# 3. Link from inside the vendor dir so lld records bare names: libqsgepaper.so
#    has no SONAME and is found through the device's scenegraph plugin dir at
#    runtime. --no-as-needed keeps it as a DT_NEEDED entry even though quill
#    resolves every engine symbol with dlsym — without the entry the plugin is
#    never loaded and EPFramebuffer::instance is unresolvable.
QT="$(cd "$HDR/qt6" && pwd)"
SRC="$(cd src && pwd)"
OUT_ABS="$(cd "$OUT" && pwd)"
( cd "$VENDOR" && zig c++ -target arm-linux-gnueabihf.2.31 -mcpu=cortex_a7 \
    -fPIC -shared -O2 -std=c++17 -s -Wno-nullability-completeness \
    -I "$QT" -I "$QT/QtCore" -I "$QT/QtGui" \
    "$SRC/vendor_probe.cpp" "$SRC/quill_c.cpp" \
    -Wl,--no-as-needed libqsgepaper.so libQt6Gui.so.6 libQt6Core.so.6 -ldl \
    -o "$OUT_ABS/libquill.so" )

echo "built: $OUT/libquill.so (rm2; zig, no SDK)"
