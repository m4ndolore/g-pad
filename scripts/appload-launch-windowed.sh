#!/bin/sh
# AppLoad entry point for windowed (qtfb) mode — used on the reMarkable 2,
# where the takeover backend does not exist. AppLoad sets QTFB_KEY for us;
# riddle sees it and picks the qtfb display backend.
HERE=$(cd "$(dirname "$0")" && pwd)

# Oracle config: put your API key in oracle.env next to this script.
if [ -f "$HERE/oracle.env" ]; then
    set -a; . "$HERE/oracle.env"; set +a
fi

# Track A (live-screen framebuffer capture) is PARKED: on this OS build the
# page lives only in xochitl's private Qt buffers (capture-xochitl.sh stays
# in scripts/ for the record, but isn't bundled). Track B instead: set
# RIDDLE_ASK_XOCHITL=1 in oracle.env and riddle asks about the newest
# rendered stock-notes page at startup.

cd "$HERE"
HOME=/home/root exec "$HERE/riddle"
