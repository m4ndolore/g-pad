#!/bin/bash
# Boot-time entry to the takeover: quiet the stock UI, hold a short window
# for the offline escape hatch, then hand over to g-pad-takeover.sh.
#
# Escape hatches, none of which need wifi or USB:
#   - press the power button within ~3s of the Anthink card appearing:
#     this boot stays on the stock UI (if the binary is broken, the window
#     is held on the frozen stock frame by this script alone)
#   - inside g-pad: five-finger tap or the power button exit to the stock
#     UI, which then keeps the panel until the next boot (Restart=no)
#   - anything wedged: hold power to force off, boot, press power in the
#     window
# Over ssh: `touch /home/root/g-pad-boot-off` disables boot takeover
# (the unit's ConditionPathExists), `systemctl stop g-pad-takeover` exits.

HERE=$(cd "$(dirname "$0")" && pwd)

# Free the panel and the power button. logind ignores the power key on
# this OS (/etc/systemd/logind.conf.d/powerkey.conf), so during the
# window a press reaches only whoever is reading event0: g-pad, or the
# fallback read below.
systemctl stop xochitl

# The escape window normally lives inside g-pad: it draws the Anthink card,
# then watches the power key for 3s. That needs a binary that starts. If it
# cannot even print its version, hold the window here on the frozen stock
# frame instead, with bash's own timed read (OS 3.28 has no `timeout`).
# event0 carries nothing but the power key, so one byte is a press. Not
# `-N 16` for the whole struct: bash's read drops NUL bytes, and a 16-byte
# input_event is mostly NULs, so a full-struct read misses the event.
# The probe needs the same library path g-pad-takeover.sh gives the real
# run: the binary links libquill.so, which links the vendor engine.
probe_libs="$HERE:/home/root/quill:/usr/lib/plugins/scenegraph"
if ! LD_LIBRARY_PATH="$probe_libs" "$HERE/g-pad" --version >/dev/null 2>&1; then
    echo "g-pad-boot: binary does not start; escape window on the stock frame"
    if IFS= read -r -t 3 -N 1 _ < /dev/input/event0; then
        echo "g-pad-boot: power press in the escape window — stock UI this boot"
        systemctl start xochitl
        exit 0
    fi
fi

exec bash "$HERE/g-pad-takeover.sh"
