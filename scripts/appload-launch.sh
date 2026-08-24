#!/bin/sh
# AppLoad entry point for takeover mode. AppLoad runs this inside xochitl's
# world, which is about to be stopped — so detach the real launch into a
# transient systemd unit (PID-1-owned, survives xochitl) and exit immediately.
#
# Works wherever the bundle is installed: we resolve our own directory rather
# than hardcoding a path, so dropping this folder into AppLoad just works.
HERE=$(cd "$(dirname "$0")" && pwd)
systemctl is-active --quiet g-pad-takeover && exit 0
# ExecStopPost is the safety net the in-script trap can't be: it runs even if
# g-pad is SIGKILLed or OOM-killed, so the tablet never stays UI-less.
# (`systemctl start` on an already-running xochitl is a no-op; the leading
# "-" ignores failures.) Fall back to a plain launch if the property is
# rejected by an older systemd.
systemd-run --unit=g-pad-takeover --collect \
    --property="ExecStopPost=-$HERE/g-pad-restore.sh" \
    /bin/bash "$HERE/g-pad-takeover.sh" \
  || systemd-run --unit=g-pad-takeover --collect /bin/bash "$HERE/g-pad-takeover.sh"
exit 0
