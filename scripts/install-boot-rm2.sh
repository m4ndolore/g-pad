#!/bin/bash
# Install boot persistence for the rM2 takeover: g-pad owns the panel from
# power-on, xochitl stays enabled underneath as the fallback surface.
#
#   Usage: ./scripts/install-boot-rm2.sh              # USB (10.11.99.1)
#          RM_HOST=192.168.1.230 ./scripts/install-boot-rm2.sh
#
# Caveat: an OS update swaps the root partition, which drops the unit from
# /etc/systemd/system (bundle files under /home survive) — rerun this after
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
echo "installed; takes effect next boot (or: systemctl start g-pad-takeover)"
