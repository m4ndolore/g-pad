#!/bin/bash
# Boot-time entry to the takeover: quiet the stock UI, hold a short window
# for the offline escape hatch, then hand over to g-pad-takeover.sh.
#
# Escape hatches, none of which need wifi or USB:
#   - press the power button within ~3s of the stock screen settling:
#     this boot stays on the stock UI (works even if the g-pad binary is
#     broken — only this script and busybox are involved)
#   - inside g-pad: five-finger tap or the power button exit to the stock
#     UI, which then keeps the panel until the next boot (Restart=no)
#   - anything wedged: hold power to force off, boot, press power in the
#     window
# Over ssh: `touch /home/root/g-pad-boot-off` disables boot takeover
# (the unit's ConditionPathExists), `systemctl stop g-pad-takeover` exits.

HERE=$(cd "$(dirname "$0")" && pwd)

# Free the panel and the power button. logind ignores the power key on
# this OS (/etc/systemd/logind.conf.d/powerkey.conf), so during the
# window a press reaches only our read below.
systemctl stop xochitl

# The e-ink panel still shows the stock UI's last frame — that frozen
# frame IS the escape window. One input event on the power button
# (16-byte struct) within 3s means: stay stock this boot.
pressed=$(timeout 3 dd if=/dev/input/event0 bs=16 count=1 2>/dev/null | wc -c)
if [ "${pressed:-0}" -gt 0 ]; then
    echo "g-pad-boot: power press in the escape window — stock UI this boot"
    systemctl start xochitl
    exit 0
fi

exec bash "$HERE/g-pad-takeover.sh"
