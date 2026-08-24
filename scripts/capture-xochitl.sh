#!/bin/sh
# capture-xochitl.sh — dump the live screen out of xochitl's process memory.
#
# Path B step 1: runs ON the rM2 while xochitl is in the foreground. The rM2
# has no kernel framebuffer for xochitl's output; the composited page lives in
# xochitl's heap, in the anonymous mapping right after its /dev/fb0 mapping
# (the established reStream technique): 8 bytes of header, then 1872x1404
# 8-bit grayscale pixels.
#
# NOT YET VERIFIED against this device's OS build (20260612085811) — that
# verification is the point of this script. Run it, pull the dump, and look:
#   ssh rm2 'sh /home/root/capture-xochitl.sh /tmp/screen.raw'
#   scp -O rm2:/tmp/screen.raw .
#   ffmpeg -f rawvideo -pixel_format gray -video_size 1872x1404 -i screen.raw \
#          -vf transpose=2 screen.png    # transpose: the panel is mounted rotated
#
# Usage: capture-xochitl.sh [output.raw]

set -eu

OUT="${1:-/tmp/xochitl-screen.raw}"

PID="$(pidof xochitl | cut -d' ' -f1)"
[ -n "$PID" ] || { echo "capture: xochitl is not running" >&2; exit 1; }

# The framebuffer is the anonymous region mapped just after /dev/fb0.
ADDR="$(grep -A1 '/dev/fb0' "/proc/$PID/maps" | tail -n1 | cut -d- -f1)"
[ -n "$ADDR" ] || { echo "capture: no /dev/fb0 mapping in xochitl (OS layout changed?)" >&2; exit 1; }

WIDTH=1872 HEIGHT=1404
SKIP=$(( 0x$ADDR + 8 ))
BYTES=$(( WIDTH * HEIGHT ))

# BusyBox dd has no skip_bytes: seek with a zero-count dd, then read the
# window with head -c (a single big dd read can come back short from procfs).
{
    dd bs=1 skip="$SKIP" count=0 2>/dev/null
    head -c "$BYTES"
} < "/proc/$PID/mem" > "$OUT"

GOT="$(wc -c < "$OUT")"
[ "$GOT" -eq "$BYTES" ] || { echo "capture: short read ($GOT of $BYTES bytes)" >&2; exit 1; }
echo "capture: $OUT (${WIDTH}x${HEIGHT} gray8, $BYTES bytes)"
