#!/bin/sh
# AppLoad entry point for windowed (qtfb) mode — the fallback build for the
# reMarkable 2. Takeover (build-takeover.sh) is the better path where it is
# available: qtfb has a latency floor and no touch or power input at all.
# AppLoad sets QTFB_KEY for us; g-pad sees it and picks the qtfb backend.
HERE=$(cd "$(dirname "$0")" && pwd)

# Oracle config: put your API key in oracle.env next to this script.
if [ -f "$HERE/oracle.env" ]; then
    set -a; . "$HERE/oracle.env"; set +a
fi

# Track A (live-screen framebuffer capture) is PARKED: on this OS build the
# page lives only in xochitl's private Qt buffers (capture-xochitl.sh stays
# in scripts/ for the record, but isn't bundled). Track B instead: set
# RIDDLE_ASK_XOCHITL=1 in oracle.env and g-pad asks about the newest
# rendered stock-notes page at startup.

cd "$HERE"
HOME=/home/root exec "$HERE/g-pad"
