#!/usr/bin/env bash
# Publish the Anthink hub onto the tablet's own loopback.
#
# The pad polls the hub (bridge.rs), but home networks move under it: the
# laptop's LAN address changes, subnets get NAT'd apart, and the pad is left
# with a dead RIDDLE_BRIDGE_URL and an empty agents drawer. The laptop can
# almost always reach the tablet, though — over Wi-Fi or the USB cable — so
# instead of chasing addresses, this keeps a reverse ssh tunnel open that
# makes the hub appear at http://127.0.0.1:9707 *on the tablet*. The pad's
# bridge URL then never has to change again.
#
#   Usage:   ./scripts/hub-tunnel.sh                     # foreground loop
#            RM_HOSTS="192.168.1.230 10.11.99.1" ./scripts/hub-tunnel.sh
#
# Install as a launchd agent (survives reboots, reconnects on drops) — the
# script is copied next to anthink-hub so the agent never depends on a checkout:
#   cp scripts/hub-tunnel.sh ~/.local/bin/gpad-hub-tunnel
#   cp scripts/com.paulgarcia.gpad-hub-tunnel.plist ~/Library/LaunchAgents/
#   launchctl load ~/Library/LaunchAgents/com.paulgarcia.gpad-hub-tunnel.plist
#
# Dropbear on the rM2 accepts remote forwards; the forward binds loopback
# only, so nothing on the tablet's Wi-Fi network can reach the hub through it.
set -u

# Wi-Fi address first (the pad's normal life), USB as the docked fallback.
RM_HOSTS="${RM_HOSTS:-192.168.1.230 10.11.99.1}"
HUB_PORT="${HUB_PORT:-9707}"

while :; do
  for host in $RM_HOSTS; do
    /usr/bin/ssh \
      -o HostKeyAlgorithms=ssh-ed25519,ssh-rsa \
      -o BatchMode=yes \
      -o ConnectTimeout=6 \
      -o ExitOnForwardFailure=yes \
      -o ServerAliveInterval=15 \
      -o ServerAliveCountMax=2 \
      -N -R "${HUB_PORT}:127.0.0.1:${HUB_PORT}" \
      "root@${host}" && :
    # ssh returning at all means the tunnel dropped (sleep, roam, reboot);
    # fall through to the next host, then come around again.
  done
  sleep 5
done
