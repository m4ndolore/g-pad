#!/bin/sh
# Cross-build g-pad for the reMarkable 2 (windowed/qtfb mode only) and
# assemble a ready-to-scp AppLoad bundle in dist/rm2/g-pad/.
#
# The rM2 is 32-bit ARM. We target musl and link statically so the binary is
# independent of the device's (old) glibc. Requires cargo-zigbuild + zig:
#   rustup target add armv7-unknown-linux-musleabihf
#   brew install zig cargo-zigbuild        # or: cargo install cargo-zigbuild
set -e
cd "$(dirname "$0")"

# Homebrew installs rustup keg-only (not symlinked into PATH); find it anyway.
if ! command -v cargo >/dev/null 2>&1; then
    for p in /opt/homebrew/opt/rustup/bin "$HOME/.cargo/bin"; do
        [ -x "$p/cargo" ] && PATH="$p:$PATH" && break
    done
fi
command -v cargo >/dev/null 2>&1 || {
    echo "cargo not found — install Rust first: https://rustup.rs" >&2; exit 1; }

TARGET=armv7-unknown-linux-musleabihf
export RUSTFLAGS="-C target-feature=+crt-static"

cargo zigbuild --release --target $TARGET --features rm2 "$@"

OUT=target/$TARGET/release
DIST=dist/rm2/g-pad
rm -rf "$DIST"
mkdir -p "$DIST"

cp "$OUT/g-pad" "$DIST/g-pad"
cp scripts/appload-launch-windowed.sh "$DIST/appload-launch.sh"
chmod +x "$DIST/g-pad" "$DIST/appload-launch.sh"
cp icon.png oracle.env.example "$DIST/"
# Derive the bundle manifest from the tracked one so id, name, version, and
# description stay in one place. Only `qtfb` differs: the windowed build runs
# under qtfb, the takeover build does not.
#
# The id matters. AppLoad keys entries by it, so two bundles sharing an id
# collide in the launcher — which is how "riddle" and "riddle-takeover" both
# ended up listed as "The Diary".
sed 's/"qtfb": false/"qtfb": true/' external.manifest.json > "$DIST/external.manifest.json"
grep -q '"qtfb": true' "$DIST/external.manifest.json" || {
    echo "manifest: expected a qtfb field to flip" >&2; exit 1; }
grep -q '"id"' "$DIST/external.manifest.json" || {
    echo "manifest: no id — AppLoad entries would collide" >&2; exit 1; }

echo
echo "Bundle ready: $DIST"
echo "Install:  scp -O -r dist/rm2/g-pad root@10.11.99.1:/home/root/xovi/exthome/appload/"
echo "Then create oracle.env in that folder with your RIDDLE_OPENAI_KEY."
