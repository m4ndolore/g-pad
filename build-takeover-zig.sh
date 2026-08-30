#!/bin/bash
# Build g-pad in TAKEOVER mode for the reMarkable 2 WITHOUT the vendor SDK.
#
# build-takeover.sh is the original path: it sources the reMarkable SDK, runs
# quill/build.sh to compile libquill.so from source, and links with the SDK's
# gcc. That is still the way to build libquill.so itself, because compiling it
# needs the SDK's Qt headers.
#
# This script covers the other case: libquill.so is ALREADY built (carried over
# from a previous build or from the riddle tree) and only the Rust side needs
# linking. cargo-zigbuild supplies the cross-linker, so no SDK is required.
#
#   Prereqs: brew install zig cargo-zigbuild
#            rustup target add armv7-unknown-linux-gnueabihf
#            quill/build/<target>/libquill.so       (prebuilt)
#            quill/vendor/<target>/libqsgepaper.so  (pulled from your device)
set -euo pipefail
cd "$(dirname "$0")"

RUST_TARGET=armv7-unknown-linux-gnueabihf
QUILL_DIR="${QUILL_DIR:-$PWD/quill}"
QUILL_BUILD="$QUILL_DIR/build/$RUST_TARGET"
QUILL_VENDOR="$QUILL_DIR/vendor/$RUST_TARGET"

if ! command -v cargo >/dev/null 2>&1; then
    for p in /opt/homebrew/opt/rustup/bin "$HOME/.cargo/bin"; do
        [ -x "$p/cargo" ] && PATH="$p:$PATH" && break
    done
fi
command -v cargo >/dev/null 2>&1 || { echo "cargo not found" >&2; exit 1; }
command -v zig >/dev/null 2>&1 || {
    echo "zig not found — brew install zig cargo-zigbuild" >&2; exit 1; }

[ -f "$QUILL_BUILD/libquill.so" ] || {
    echo "no prebuilt $QUILL_BUILD/libquill.so" >&2
    echo "build it once with: DEVICE=rm2 ./build-takeover.sh  (needs the SDK)" >&2
    exit 1; }
[ -f "$QUILL_VENDOR/libqsgepaper.so" ] || {
    echo "no $QUILL_VENDOR/libqsgepaper.so — pull it from your own device:" >&2
    echo "  scp -O root@10.11.99.1:/usr/lib/plugins/scenegraph/libqsgepaper.so $QUILL_VENDOR/" >&2
    exit 1; }

# zig records the link path it is GIVEN as the binary's DT_NEEDED entry, and
# libquill.so carries no SONAME to override it. Any path at all — absolute or
# repo-relative — would be baked in and could never resolve on the tablet,
# where the bundle lives under AppLoad. So stage the libraries and link from
# INSIDE that directory, which makes the recorded name bare; the runtime rpath
# ($ORIGIN plus the device's scenegraph plugin dir) then does the resolving.
STAGE="$PWD/target/$RUST_TARGET/linkstage"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$QUILL_BUILD/libquill.so" "$QUILL_VENDOR/libqsgepaper.so" "$STAGE/"

# `-l:libquill.so` asks the linker for that exact filename, found via -L, and
# records it verbatim as DT_NEEDED — a bare name, whatever the search path was.
# build.rs already emits the -L entries, so only the naming needs overriding.
QUILL_BUILD_DIR="$STAGE" QUILL_VENDOR_DIR="$STAGE" \
    cargo zigbuild --release --target "$RUST_TARGET" \
    --features takeover,rm2 "$@"

OUT="target/$RUST_TARGET/release"
cp "$OUT/g-pad" "$OUT/g-pad-takeover"

# ld.lld records the staging path it was handed, so shorten it back to a bare
# name and let the rpath resolve it on the tablet.
python3 scripts/bare-needed.py "$OUT/g-pad-takeover" "$STAGE/libquill.so"

# Belt and braces: a '/' anywhere in the recorded name means the binary would
# die with "cannot open shared object file" on the device.
if strings -a "$OUT/g-pad-takeover" | grep -qE '/libquill\.so'; then
    echo "error: a build-host path is still baked into DT_NEEDED" >&2
    strings -a "$OUT/g-pad-takeover" | grep -E 'libquill\.so' >&2
    exit 1
fi

echo "built: $OUT/g-pad-takeover (rm2; prebuilt Quill, no SDK)"
echo "next:  DEVICE=rm2 ./scripts/make-bundle.sh"
