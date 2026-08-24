#!/bin/sh
# Full takeover on the reMarkable 2: stop xochitl, drive the panel through
# the rm2display server (timower/rM2-stuff — vendor libqsgepaper engine,
# standalone), run g-pad with per-update waveform control, and ALWAYS hand
# the tablet back to xochitl afterwards — even if g-pad dies uncleanly.
#
# Prereqs on the tablet:
#   - rm2fb_server at /opt/bin/rm2fb_server (from rm2display.ipk), on a
#     FIRMWARE VERSION IT SUPPORTS (including the 3.27 standalone-server port)
#   - this script + g-pad + oracle.env in /home/root/xovi/exthome/appload/g-pad
#
# Launch from ssh for now:  sh g-pad-takeover-rm2.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd)

[ -f "$HERE/oracle.env" ] && { set -a; . "$HERE/oracle.env"; set +a; }

SERVER_PID=""
SERVER_UNIT_STARTED=0
restore() {
    [ "$SERVER_UNIT_STARTED" -eq 1 ] && systemctl stop rm2fb.service 2>/dev/null
    [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null
    systemctl start xochitl 2>/dev/null || true
}
trap restore EXIT INT TERM

systemctl stop xochitl
sleep 1

if systemctl -q is-active rm2fb.socket 2>/dev/null ||
   systemctl -q is-enabled rm2fb.socket 2>/dev/null; then
    systemctl start rm2fb.socket rm2fb.service
    SERVER_UNIT_STARTED=1
else
    /opt/bin/rm2fb_server >/tmp/rm2fb-server.log 2>&1 &
    SERVER_PID=$!
fi
# Give the server a moment to map the panel; bail out (restoring xochitl)
# if it died — usually an unsupported-firmware address lookup failure.
sleep 2
if { [ "$SERVER_UNIT_STARTED" -eq 1 ] &&
     ! systemctl -q is-active rm2fb.service 2>/dev/null; } ||
   { [ -n "$SERVER_PID" ] && ! kill -0 "$SERVER_PID" 2>/dev/null; }; then
    echo "rm2fb_server exited — unsupported firmware? see /tmp/rm2fb-server.log" >&2
    exit 1
fi

cd "$HERE"
HOME=/home/root "$HERE/g-pad"
