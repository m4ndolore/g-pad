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
#            RM_HOSTS="192.168.1.78 192.168.1.230" ./scripts/hub-tunnel.sh
#
# Install as a launchd agent (survives reboots, reconnects on drops) — the
# script is copied next to anthink-hub so the agent never depends on a checkout:
#   cp scripts/hub-tunnel.sh ~/.local/bin/gpad-hub-tunnel
#   cp scripts/com.paulgarcia.gpad-hub-tunnel.plist ~/Library/LaunchAgents/
#   launchctl load ~/Library/LaunchAgents/com.paulgarcia.gpad-hub-tunnel.plist
#
# Dropbear on the rM2 accepts remote forwards; the forward binds loopback
# only, so nothing on the tablet's Wi-Fi network can reach the hub through it.
#
# Two things change under the tunnel and it copes with both:
# - The tablet's Wi-Fi lease moves. Whenever the cable is in, the tablet is
#   asked where it is on Wi-Fi and that address is tried first next time.
# - Every reMarkable OS update regenerates the tablet's ssh host key. Over
#   the cable there is nothing to impersonate, so the USB leg pins no key;
#   the Wi-Fi legs keep the normal known_hosts check (a first key is learned,
#   a changed one is refused — then `ssh-keygen -R <address>` on the laptop).
set -u

USB=10.11.99.1
HUB_PORT="${HUB_PORT:-9707}"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/gpad-hub-tunnel"
mkdir -p "$STATE"

# Wi-Fi addresses, the learned one first, without repeats; USB always last.
wifi_hosts() {
  local learned=""
  [ -f "$STATE/wifi-ip" ] && learned=$(cat "$STATE/wifi-ip")
  # shellcheck disable=SC2086
  printf '%s\n' $learned ${RM_HOSTS:-192.168.1.78 192.168.1.230} | awk 'NF && !seen[$0]++'
}

ssh_opts() {
  if [ "$1" = "$USB" ]; then
    printf '%s\n' -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR
  else
    printf '%s\n' -o StrictHostKeyChecking=accept-new
  fi
}

# Ask the tablet over USB where it is on Wi-Fi, for the next reconnect.
learn_wifi_ip() {
  local ip
  # shellcheck disable=SC2046
  ip=$(/usr/bin/ssh $(ssh_opts "$USB") -o BatchMode=yes -o ConnectTimeout=3 "root@$USB" \
        "nmcli -g IP4.ADDRESS dev show wlan0 2>/dev/null | cut -d/ -f1" 2>/dev/null | head -n 1)
  case "$ip" in
    *.*.*.*) echo "$ip" > "$STATE/wifi-ip" ;;
  esac
}

while :; do
  for host in $(wifi_hosts) "$USB"; do
    [ "$host" = "$USB" ] && learn_wifi_ip
    # shellcheck disable=SC2046
    /usr/bin/ssh $(ssh_opts "$host") \
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
