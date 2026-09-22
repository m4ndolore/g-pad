#!/usr/bin/env bash
# Stage the install bundle into dist/, ready for scripts/install.sh or a
# release. Prereq: ./build-takeover-zig.sh (rm2) or DEVICE=rmpp
# ./build-takeover.sh has produced the binary.
#
#   DEVICE=rm2 ./scripts/make-bundle.sh
#   VERSION=v0.2.0 DEVICE=rm2 ./scripts/make-bundle.sh   # stamps release.txt
set -euo pipefail
cd "$(dirname "$0")/.."

DEVICE="${DEVICE:-rm2}"
case "$DEVICE" in
  rm2)
    TARGET=armv7-unknown-linux-gnueabihf
    DIST=dist/rm2-takeover/g-pad
    FEATURES=rm2
    # The OS whose e-ink engine ABI libquill.so was built against. The
    # installer warns when the tablet runs a different major.minor.
    BUILT_FOR="${BUILT_FOR:-3.28}"
    ;;
  rmpp)
    TARGET=aarch64-unknown-linux-gnu
    DIST=dist/g-pad
    FEATURES=
    BUILT_FOR="${BUILT_FOR:-}"
    ;;
  *) echo "unknown DEVICE=$DEVICE (use rm2 or rmpp)" >&2; exit 1 ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
    for p in /opt/homebrew/opt/rustup/bin "$HOME/.cargo/bin"; do
        [ -x "$p/cargo" ] && PATH="$p:$PATH" && break
    done
fi

BIN="target/$TARGET/release/g-pad-takeover"
QUILL="quill/build/$TARGET/libquill.so"
[ -f "$BIN" ] || { echo "build first: ./build-takeover-zig.sh (rm2) or DEVICE=$DEVICE ./build-takeover.sh" >&2; exit 1; }
[ -f "$QUILL" ] || { echo "missing $QUILL" >&2; exit 1; }

rm -rf "$DIST"
mkdir -p "$DIST/cards"
install -m 755 "$BIN" "$DIST/g-pad"
install -m 755 "$QUILL" "$DIST/"
install -m 755 scripts/appload-launch.sh scripts/g-pad-takeover.sh \
    scripts/g-pad-restore.sh scripts/g-pad-boot.sh "$DIST/"
install -m 644 scripts/g-pad-takeover.service "$DIST/"
install -m 644 external.manifest.json icon.png oracle.env.example settings.schema.json "$DIST/"

# The boot, power-off and restart cards, rendered by the host build so they
# match what the pad draws pixel for pixel. The installer copies two of them
# over the OS's own shutdown images.
cargo run --quiet --release ${FEATURES:+--features "$FEATURES"} -- --render-cards "$DIST/cards" >/dev/null

{
    echo "version=${VERSION:-dev}"
    echo "commit=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
    echo "device=$DEVICE"
    [ -n "$BUILT_FOR" ] && echo "built_for=$BUILT_FOR"
    echo "built=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$DIST/release.txt"

# reMarkable's library must never travel with the bundle.
if [ -e "$DIST/libqsgepaper.so" ]; then
    echo "refusing: $DIST contains libqsgepaper.so" >&2; exit 1
fi

echo "staged: $(du -sh "$DIST" | cut -f1) in $DIST/"
