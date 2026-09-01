#!/bin/sh
# Idempotent stock-UI recovery for normal exit and systemd's crash hook.
echo g-pad-takeover > /sys/power/wake_unlock 2>/dev/null || true
rm -f /tmp/epframebuffer.lock
systemctl reset-failed xochitl 2>/dev/null || true
# --no-block for the same reason as the unit's ExecStopPost: this script
# runs inside the takeover's exit trap, and a blocking start deadlocks
# `systemctl restart g-pad-takeover` (seen wedged on hardware 2026-09-01).
systemctl start --no-block xochitl
