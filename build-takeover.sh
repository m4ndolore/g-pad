#!/bin/bash
# Build g-pad in TAKEOVER mode (links libquill.so + vendor Qt/qsgepaper).
#
# DEVICE=rmpp (default) builds for the Paper Pro. DEVICE=rm2 builds the
# clean-room Quill ARMv7 takeover path for the reMarkable 2.
set -euo pipefail
cd "$(dirname "$0")"

DEVICE="${DEVICE:-rmpp}"
case "$DEVICE" in
  rm2)
    SDK="${SDK:-$HOME/rm-sdk-rm2}"
    RUST_TARGET=armv7-unknown-linux-gnueabihf
    CARGO_LINKER_VAR=CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER
    CARGO_FEATURES=takeover,rm2
    ;;
  rmpp)
    SDK="${SDK:-$HOME/rm-sdk-3.26}"
    RUST_TARGET=aarch64-unknown-linux-gnu
    CARGO_LINKER_VAR=CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER
    CARGO_FEATURES=takeover
    ;;
  *)
    echo "unknown DEVICE=$DEVICE (use rmpp or rm2)" >&2
    exit 1
    ;;
esac

if [[ "${1:-}" == "--print-config" ]]; then
    printf 'DEVICE=%s\nTARGET=%s\nSDK=%s\nFEATURES=%s\n' \
        "$DEVICE" "$RUST_TARGET" "$SDK" "$CARGO_FEATURES"
    exit 0
fi

if ! command -v cargo >/dev/null 2>&1; then
    for p in /opt/homebrew/opt/rustup/bin "$HOME/.cargo/bin"; do
        [ -x "$p/cargo" ] && PATH="$p:$PATH" && break
    done
fi
command -v cargo >/dev/null 2>&1 || { echo "cargo not found" >&2; exit 1; }

ENV=$(find "$SDK" -maxdepth 1 -name 'environment-setup-*' -print -quit)
[ -n "$ENV" ] || { echo "no SDK environment under $SDK" >&2; exit 1; }
unset LD_LIBRARY_PATH          # SDK env refuses to source otherwise
source "$ENV"

QUILL_DIR="${QUILL_DIR:-$PWD/quill}"
DEVICE="$DEVICE" SDK="$SDK" "$QUILL_DIR/build.sh"
export QUILL_BUILD_DIR="$QUILL_DIR/build/$RUST_TARGET"
export QUILL_VENDOR_DIR="$QUILL_DIR/vendor/$RUST_TARGET"
export RIDDLE_SDK_SYSROOT_LIB="$SDKTARGETSYSROOT/usr/lib"

# Point cargo's cross linker at the SDK gcc. $CC includes the -mcpu/-sysroot
# flags as one string; cargo wants a single program, so wrap it.
mkdir -p "target/$RUST_TARGET"
WRAPPER="$PWD/target/$RUST_TARGET/sdk-cc.sh"
cat > "$WRAPPER" <<EOF
#!/bin/bash
exec $CC "\$@"
EOF
chmod +x "$WRAPPER"

export "$CARGO_LINKER_VAR=$WRAPPER"

cargo build --release --target "$RUST_TARGET" --features "$CARGO_FEATURES" "$@"

# The windowed (default-feature) build shares the same output path and would
# clobber this one. Copy the takeover binary to a distinct name so the two
# never collide.
OUT="target/$RUST_TARGET/release"
cp "$OUT/g-pad" "$OUT/g-pad-takeover"
echo "built: $OUT/g-pad-takeover ($DEVICE; clean-room Quill takeover)"
