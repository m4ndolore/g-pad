#!/usr/bin/env bash
# Cut a GitHub release for the reMarkable 2: build, bundle, zip, checksum,
# tag, publish. The installer in the release downloads exactly these assets.
#
#   ./scripts/release.sh v0.2.0             # build, tag, and publish
#   ./scripts/release.sh v0.2.0 --dry-run   # build and stage; touch nothing remote
#
# Needs: the cross toolchain build-takeover-zig.sh needs (zig, cargo-zigbuild,
# the armv7 target), quill/build/<target>/libquill.so built for the OS in
# BUILT_FOR (default 3.28; see quill/build-zig.sh), and `gh` logged in.
#
# Assets uploaded:
#   anthink-rm2-<tag>.zip   the bundle, unpacking to g-pad/
#   SHA256SUMS              checksums the installer verifies
#   install.sh              scripts/install.sh, so the one-line install works
set -euo pipefail
cd "$(dirname "$0")/.."

TAG="${1:-}"
DRY=0
[ "${2:-}" = --dry-run ] && DRY=1
case "$TAG" in
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "usage: $0 vX.Y.Z [--dry-run]" >&2; exit 2 ;;
esac

if [ -n "$(git status --porcelain)" ]; then
    echo "the working tree is not clean; commit or stash before releasing" >&2; exit 1
fi
CARGO_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
if [ "v$CARGO_VERSION" != "$TAG" ]; then
    echo "Cargo.toml says $CARGO_VERSION but the tag is $TAG; bump Cargo.toml first" >&2; exit 1
fi

echo "== building for the reMarkable 2"
./build-takeover-zig.sh >/dev/null
VERSION="$TAG" DEVICE=rm2 ./scripts/make-bundle.sh

echo "== packing"
OUT=dist/release
rm -rf "$OUT"
mkdir -p "$OUT"
ZIP="anthink-rm2-$TAG.zip"
(cd dist/rm2-takeover && zip -qr "../release/$ZIP" g-pad)
if unzip -l "$OUT/$ZIP" | grep -q libqsgepaper; then
    echo "refusing: the zip contains reMarkable's proprietary library" >&2; exit 1
fi
install -m 755 scripts/install.sh "$OUT/install.sh"
(cd "$OUT" && shasum -a 256 "$ZIP" install.sh > SHA256SUMS)
cat "$OUT/SHA256SUMS"

NOTES="$OUT/notes.md"
{
    echo "Prebuilt for the reMarkable 2 on OS ${BUILT_FOR:-3.28}. Install from any machine with ssh:"
    echo
    echo '```sh'
    echo "curl -fsSL https://github.com/m4ndolore/g-pad/releases/download/$TAG/install.sh | bash"
    echo '```'
    echo
    echo "The installer finds the tablet over USB, copies the bundle, installs the boot unit, asks for an API key, and starts the pad. Run it again with \`--uninstall\` to go back to the stock UI."
    echo
    echo "The bundle does not contain reMarkable's e-ink library; the pad loads it from the tablet."
    echo
    echo "## Changes"
    echo
    PREV="$(git describe --tags --abbrev=0 --match 'v*' HEAD^ 2>/dev/null || true)"
    if [ -n "$PREV" ]; then
        git log --format='- %s' "$PREV..HEAD" | grep -vE '^- (Merge|chore)' || true
    else
        echo "- First public release."
    fi
} > "$NOTES"

if [ "$DRY" = 1 ]; then
    echo "== dry run: assets staged in $OUT, nothing tagged or published"
    exit 0
fi

echo "== tagging $TAG"
git tag -a "$TAG" -m "Anthink $TAG for the reMarkable 2"
git push origin "$TAG"

echo "== publishing"
gh release create "$TAG" "$OUT/$ZIP" "$OUT/SHA256SUMS" "$OUT/install.sh" \
    --title "Anthink $TAG" --notes-file "$NOTES"
echo "released: https://github.com/m4ndolore/g-pad/releases/tag/$TAG"
