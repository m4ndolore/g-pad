#!/usr/bin/env bash
# Stage the AppLoad bundle into dist/riddle/, ready for `remagic publish`.
# Prereq: DEVICE=<rmpp|rm2> ./build-takeover.sh has produced the binary.
set -euo pipefail
cd "$(dirname "$0")/.."

DEVICE="${DEVICE:-rmpp}"
case "$DEVICE" in
  rm2)
    TARGET=armv7-unknown-linux-gnueabihf
    DIST=dist/rm2-takeover/riddle
    ;;
  rmpp)
    TARGET=aarch64-unknown-linux-gnu
    DIST=dist/riddle
    ;;
  *) echo "unknown DEVICE=$DEVICE (use rmpp or rm2)" >&2; exit 1 ;;
esac

BIN="target/$TARGET/release/riddle-takeover"
QUILL="quill/build/$TARGET/libquill.so"
[ -f "$BIN" ] || { echo "build first: DEVICE=$DEVICE ./build-takeover.sh" >&2; exit 1; }
[ -f "$QUILL" ] || { echo "missing $QUILL" >&2; exit 1; }

rm -rf "$DIST"
mkdir -p "$DIST"
install -m 755 "$BIN" "$DIST/riddle"
install -m 755 "$QUILL" "$DIST/"
install -m 755 scripts/appload-launch.sh scripts/riddle-takeover.sh \
    scripts/riddle-restore.sh "$DIST/"
install -m 644 external.manifest.json icon.png oracle.env.example settings.schema.json "$DIST/"

echo "staged: $(du -sh "$DIST" | cut -f1) in $DIST/"
