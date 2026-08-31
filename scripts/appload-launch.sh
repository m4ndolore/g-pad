#!/bin/sh
# AppLoad entry point for takeover mode. AppLoad runs this inside xochitl's
# world, which is about to be stopped — so hand the real launch to systemd
# (PID-1-owned, survives xochitl) and exit immediately.
#
# Works wherever the bundle is installed: we resolve our own directory rather
# than hardcoding a path, so dropping this folder into AppLoad just works.
HERE=$(cd "$(dirname "$0")" && pwd)
systemctl is-active --quiet g-pad-takeover && exit 0

# With boot persistence installed, g-pad-takeover is a file unit and
# systemd-run cannot reuse the name — start the unit itself. This bundle's
# RIDDLE_PAGE (oracle.env) cannot cross `systemctl start` as an env var, so
# it lands in the saved preference instead; the unit's environment carries
# no RIDDLE_PAGE, and the saved page wins the resolve.
# (If /home/root/g-pad-boot-off exists, the unit's condition skips the
# start silently — that file means g-pad is deliberately off.)
if systemctl cat g-pad-takeover.service >/dev/null 2>&1; then
    page=$(sed -n 's/^RIDDLE_PAGE=//p' "$HERE/oracle.env" 2>/dev/null | tail -n 1)
    if [ -n "$page" ]; then
        mkdir -p /home/root/g-pad-data
        prefs=/home/root/g-pad-data/preferences
        grep -v '^page=' "$prefs" 2>/dev/null > "$prefs.tmp" || true
        echo "page=$page" >> "$prefs.tmp"
        mv "$prefs.tmp" "$prefs"
    fi
    systemctl start --no-block g-pad-takeover
    exit 0
fi

# No file unit installed: detach into a transient unit of the same name.
# ExecStopPost is the safety net the in-script trap can't be: it runs even
# if g-pad is SIGKILLed or OOM-killed, so the tablet never stays UI-less.
# (`systemctl start` on an already-running xochitl is a no-op; the leading
# "-" ignores failures.) Fall back to a plain launch if the property is
# rejected by an older systemd.
systemd-run --unit=g-pad-takeover --collect \
    --property="ExecStopPost=-$HERE/g-pad-restore.sh" \
    /bin/bash "$HERE/g-pad-takeover.sh" \
  || systemd-run --unit=g-pad-takeover --collect /bin/bash "$HERE/g-pad-takeover.sh"
exit 0
