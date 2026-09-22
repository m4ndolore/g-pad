#!/bin/bash
# Install boot persistence for the rM2 takeover: g-pad owns the panel from
# power-on, xochitl stays enabled underneath as the fallback surface.
#
#   Usage: ./scripts/install-boot-rm2.sh              # USB (10.11.99.1)
#          RM_HOST=192.168.1.230 ./scripts/install-boot-rm2.sh
#
# Caveat: an OS update swaps the root partition, which drops the unit from
# /etc/systemd/system and restores the stock shutdown images under
# /usr/share/remarkable (bundle files under /home survive) — rerun this after
# an update. Undo: `systemctl disable g-pad-takeover` on the device, or
# offline via `touch /home/root/g-pad-boot-off`.
set -euo pipefail
cd "$(dirname "$0")/.."

RM_HOST="${RM_HOST:-10.11.99.1}"
SSH="/usr/bin/ssh root@$RM_HOST"
APP=/home/root/xovi/exthome/appload/g-pad

$SSH "cat > $APP/g-pad-boot.sh && chmod +x $APP/g-pad-boot.sh" < scripts/g-pad-boot.sh
$SSH "cat > /etc/systemd/system/g-pad-takeover.service" < scripts/g-pad-takeover.service
$SSH "systemctl daemon-reload && systemctl enable g-pad-takeover.service"

# The OS's own power-off and reboot images become the Anthink cards, rendered
# by the host build so they match what the pad draws pixel for pixel. The
# stock files are kept once under /home/root, which survives OS updates;
# the images themselves do not (see the caveat above) — rerun after an update.
# The rm2 feature picks the panel size: 1404x1872, not the rMPP's 1620x2160.
CARDS=dist/cards
cargo run --quiet --release --features rm2 -- --render-cards "$CARDS" >/dev/null
$SSH 'mkdir -p /home/root/g-pad-stock-images && for f in poweroff rebooting; do
    [ -e /home/root/g-pad-stock-images/$f.png ] || cp /usr/share/remarkable/$f.png /home/root/g-pad-stock-images/; done'
for f in poweroff rebooting; do
    $SSH "cat > /usr/share/remarkable/$f.png" < "$CARDS/$f.png"
done
echo "installed; takes effect next boot (or: systemctl start g-pad-takeover)"
