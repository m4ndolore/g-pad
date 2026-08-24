#!/bin/bash
# Launch the pad in full-takeover mode: stop xochitl, run g-pad against the
# vendor e-ink engine (instant ink), ALWAYS restore xochitl on exit.
#
# Exit the pad: power button, 5-finger tap, or SIGTERM. Escape hatch if
# anything wedges: ssh rm 'systemctl start xochitl'.

# Resolve our own install directory so the bundle works wherever it lives
# (e.g. /home/root/xovi/exthome/appload/g-pad/ when installed via AppLoad).
HERE=$(cd "$(dirname "$0")" && pwd)

restore() {
    "$HERE/g-pad-restore.sh"
}
# Under the Remagic Home session host (REMAGIC_SESSION=1), xochitl is already
# stopped and the session owns its restore — skip our own stop/restart.
if [ -z "${REMAGIC_SESSION:-}" ]; then
    trap restore EXIT INT TERM
fi

# Oracle config: put your API key in oracle.env next to this script, e.g.
#   RIDDLE_OPENAI_KEY=sk-...
#   RIDDLE_OPENAI_BASE=https://api.openai.com/v1     # optional
#   RIDDLE_OPENAI_MODEL=gpt-4o-mini                  # optional
# Without it, g-pad falls back to the pi backend (if pi is installed).
if [ -f "$HERE/oracle.env" ]; then
    set -a; . "$HERE/oracle.env"; set +a
fi

if [ -z "${REMAGIC_SESSION:-}" ]; then
    # xochitl normally holds this. Without a replacement, kernel autosleep can
    # resume into a second display engine while g-pad still owns the panel.
    if ! echo g-pad-takeover > /sys/power/wake_lock 2>/dev/null; then
        if command -v systemd-inhibit >/dev/null 2>&1; then
            echo "g-pad: kernel wakelock unavailable; using systemd sleep inhibitor" >&2
        else
            echo "g-pad: warning: no sleep-prevention mechanism available" >&2
        fi
    fi
    systemctl stop xochitl
fi
rm -f /tmp/epframebuffer.lock      # stale EPD lock blocks the engine
[ -z "${REMAGIC_SESSION:-}" ] && sleep 1

cd "$HERE"
# libquill.so ships in this bundle; libqsgepaper.so (reMarkable's proprietary
# engine) comes from the device's own scenegraph plugin dir. We search the
# bundle first, then a standalone /home/root/quill install, then the plugin dir.
if command -v systemd-inhibit >/dev/null 2>&1; then
    systemd-inhibit --what=sleep --who=g-pad \
        --why="g-pad owns the e-paper panel during takeover" --mode=block \
        /usr/bin/env LD_LIBRARY_PATH="$HERE:/home/root/quill:/usr/lib/plugins/scenegraph" \
        PAPERTERM_SHELL= HOME=/home/root "$HERE/g-pad"
else
    LD_LIBRARY_PATH="$HERE:/home/root/quill:/usr/lib/plugins/scenegraph" \
        PAPERTERM_SHELL= HOME=/home/root "$HERE/g-pad"
fi
echo "g-pad-takeover: pad closed ($?), restoring xochitl"
