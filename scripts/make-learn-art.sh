#!/bin/sh
# Regenerate the Learn play-deck character art from the SVG sources.
#
# WebKit (via qlmanage) is the renderer — ImageMagick's own SVG rasterizer
# drops inherited strokes, which erases the art's ink line. magick then
# trims the padding, adds a small margin, area-averages down, and writes
# 8-bit grayscale — the format blit_gray/GC16 needs (see assets/mc-mark.png).
#
# Run from the repo root on a Mac: sh scripts/make-learn-art.sh
set -e
src=assets/learn/src
out=assets/learn
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for f in "$src"/*.svg; do
    name=$(basename "$f" .svg)
    qlmanage -t -s 1440 -o "$tmp" "$f" >/dev/null
    magick "$tmp/$name.svg.png" \
        -background white -flatten -trim +repage \
        -bordercolor white -border 12 \
        -colorspace Gray -resize 720x720 -depth 8 \
        -define png:color-type=0 -define png:bit-depth=8 \
        "$out/$name.png"
    echo "$out/$name.png: $(magick identify -format '%wx%h %[colorspace]' "$out/$name.png")"
done
