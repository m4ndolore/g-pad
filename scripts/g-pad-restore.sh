#!/bin/sh
# Idempotent stock-UI recovery for normal exit and systemd's crash hook.
echo g-pad-takeover > /sys/power/wake_unlock 2>/dev/null || true
rm -f /tmp/epframebuffer.lock
systemctl reset-failed xochitl 2>/dev/null || true
systemctl start xochitl
