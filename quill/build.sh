#!/usr/bin/env bash
# Build the vendored clean-room Quill adapter for either supported tablet.
# libqsgepaper.so is pulled from the owner's device and is never distributed.
set -euo pipefail
cd "$(dirname "$0")"

DEVICE="${DEVICE:-rmpp}"
case "$DEVICE" in
  rm2)
    SDK="${SDK:-$HOME/rm-sdk-rm2}"
    TARGET=armv7-unknown-linux-gnueabihf
    DEFAULT_HOST=rm2
    ;;
  rmpp)
    SDK="${SDK:-$HOME/rm-sdk-3.26}"
    TARGET=aarch64-unknown-linux-gnu
    DEFAULT_HOST=rm
    ;;
  *)
    echo "unknown DEVICE=$DEVICE (use rmpp or rm2)" >&2
    exit 1
    ;;
esac

if [[ "${1:-}" == "--print-config" ]]; then
  printf 'DEVICE=%s\nTARGET=%s\nSDK=%s\n' "$DEVICE" "$TARGET" "$SDK"
  exit 0
fi

ENV_FILE=$(find "$SDK" -maxdepth 1 -name 'environment-setup-*' -print -quit)
[ -n "$ENV_FILE" ] || { echo "no SDK environment under $SDK" >&2; exit 1; }
unset LD_LIBRARY_PATH
source "$ENV_FILE"

OUT="build/$TARGET"
VENDOR="vendor/$TARGET"
mkdir -p "$OUT" "$VENDOR"
if [ ! -f "$VENDOR/libqsgepaper.so" ]; then
  DEVICE_HOST="${QUILL_DEVICE_HOST:-$DEFAULT_HOST}"
  echo "pulling libqsgepaper.so from $DEVICE_HOST..."
  scp -O "$DEVICE_HOST:/usr/lib/plugins/scenegraph/libqsgepaper.so" "$VENDOR/"
fi

LIB_DESC=$(file -b "$VENDOR/libqsgepaper.so")
case "$DEVICE:$LIB_DESC" in
  rm2:*"ELF 32-bit"*"ARM"*) ;;
  rmpp:*"ELF 64-bit"*"aarch64"*) ;;
  *)
    echo "wrong libqsgepaper.so architecture for $DEVICE: $LIB_DESC" >&2
    exit 1
    ;;
esac

QTINC="$SDKTARGETSYSROOT/usr/include"
$CXX -fPIC -shared -O2 -std=c++17 \
  -I "$QTINC" -I "$QTINC/QtCore" -I "$QTINC/QtGui" \
  src/vendor_probe.cpp src/quill_c.cpp \
  -L "$VENDOR" -lqsgepaper -lQt6Gui -lQt6Core -ldl \
  -o "$OUT/libquill.so"

echo "built: $OUT/libquill.so ($DEVICE)"
