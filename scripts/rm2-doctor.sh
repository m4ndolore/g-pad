#!/usr/bin/env bash
# g-pad rM2 doctor — why AppLoad (or g-pad inside it) is missing.
#
#   Usage:  ./scripts/rm2-doctor.sh                      # USB (10.11.99.1)
#           RM_HOST=192.168.1.42 ./scripts/rm2-doctor.sh # over Wi-Fi
#
# Read-only: it looks, reports, and prints the exact command that fixes each
# thing it finds. It never changes the tablet.
#
# AppLoad is not a stock app. It exists only while xovi is loaded into a
# running xochitl, so "AppLoad is missing" is always one of four states, and
# this tells you which:
#   1. xovi was never installed
#   2. xovi is installed but not running (the usual one after a reboot or an
#      OS update, which restarts xochitl without the loader)
#   3. xovi is running but appload.so is absent or failed to load
#   4. AppLoad is there and it is the g-pad bundle that is missing
set -euo pipefail

RM_HOST="${RM_HOST:-10.11.99.1}"

# Same rM2 SSH quirks the installer works around: dropbear 2020.81 hangs up
# when RSA is negotiated, while older firmware offers nothing but ssh-rsa.
WORK="$(mktemp -d)"
SSH_OPTS=(-o HostKeyAlgorithms=ssh-ed25519,ssh-rsa -o PubkeyAcceptedAlgorithms=+ssh-rsa
          -o ControlMaster=auto -o "ControlPath=$WORK/ssh-ctrl" -o ControlPersist=60
          -o ConnectTimeout=10)
rm_ssh() { ssh -n "${SSH_OPTS[@]}" "root@$RM_HOST" "$@"; }
# `set -e` is in force inside the trap: a failing `ssh -O exit` (no control
# master, or no ssh at all) would abort cleanup before the temp dir is removed
# and would overwrite the script's exit status. Swallow it.
cleanup() { ssh "${SSH_OPTS[@]}" -O exit "root@$RM_HOST" 2>/dev/null || true; rm -rf "$WORK"; }
trap cleanup EXIT

command -v ssh >/dev/null || { echo "error: no ssh client on this machine" >&2; exit 1; }

ok()   { printf '  \033[32mok\033[0m    %s\n' "$*"; }
bad()  { printf '  \033[31mmiss\033[0m  %s\n' "$*"; VERDICT+=("$*"); }
warn() { printf '  \033[33mnote\033[0m  %s\n' "$*"; }
say()  { printf '\n\033[1m%s\033[0m\n' "$*"; }
fix()  { FIXES+=("$*"); }
VERDICT=(); FIXES=()

say "Reaching root@$RM_HOST"
if ! rm_ssh true 2>/dev/null; then
    cat >&2 <<EOF
  cannot reach root@$RM_HOST over SSH.

  Over Wi-Fi the address is not 10.11.99.1 (that is the USB link). Read the
  tablet's Wi-Fi IP under Settings > General > Help > Copyrights and licenses
  (GPLv3 Compliance, bottom of the page) — the root password is there too —
  and re-run:  RM_HOST=<that-ip> $0

  If it refuses the key: ssh-keygen -R "$RM_HOST", then try again.
EOF
    exit 1
fi
ok "SSH works"

MACHINE="$(rm_ssh 'cat /sys/devices/soc0/machine 2>/dev/null' || true)"
OSVER="$(rm_ssh 'sed -n "s/^REMARKABLE_RELEASE_VERSION=//p" /usr/share/remarkable/update.conf 2>/dev/null' || true)"
[ "$MACHINE" = "reMarkable 2.0" ] && ok "device: $MACHINE" || warn "device reports '${MACHINE:-unknown}' — this doctor is written for the rM2"
[ -n "$OSVER" ] && ok "reMarkable OS $OSVER"

# --- 1. is xovi installed? ---------------------------------------------------
say "xovi (the loader AppLoad rides on)"
if rm_ssh 'test -x /home/root/xovi/start'; then
    ok "/home/root/xovi is installed"
else
    bad "xovi is not installed"
    fix "./scripts/install-rm2.sh   # with RM_HOST=$RM_HOST — installs xovi, AppLoad, and g-pad"
    # Nothing below can be true without it; report and stop.
    printf '\n\033[1mVerdict\033[0m\n  AppLoad is missing because xovi was never installed.\n\n'
    printf '  %s\n' "${FIXES[@]}"
    exit 0
fi

# --- 2. is it actually loaded into the running xochitl? ----------------------
# This is the state an OS update or a plain reboot leaves behind: every file is
# on disk, and none of it is in the process.
XPID="$(rm_ssh 'pidof xochitl 2>/dev/null' || true)"
if [ -z "$XPID" ]; then
    bad "xochitl is not running (no stock UI to host AppLoad)"
    fix "ssh root@$RM_HOST 'systemctl start xochitl'   # then start xovi below"
elif rm_ssh "grep -q xovi /proc/$XPID/maps 2>/dev/null"; then
    ok "xovi is loaded into the running xochitl (pid $XPID)"
else
    bad "xochitl is running WITHOUT xovi — this is why AppLoad is gone"
    fix "ssh root@$RM_HOST '/home/root/xovi/start'   # restarts xochitl with the loader"
fi

if rm_ssh 'systemctl is-enabled xovi-tripletap >/dev/null 2>&1'; then
    ok "xovi-tripletap is installed (triple-press power toggles xovi)"
else
    warn "no xovi-tripletap: nothing re-loads xovi after a reboot, so AppLoad"
    warn "      will keep vanishing. The installer sets this up."
fi

# --- 3. AppLoad itself -------------------------------------------------------
say "AppLoad"
if rm_ssh 'test -f /home/root/xovi/extensions.d/appload.so'; then
    ok "extensions.d/appload.so is present"
else
    bad "appload.so is not in /home/root/xovi/extensions.d"
    fix "./scripts/install-rm2.sh   # with RM_HOST=$RM_HOST — reinstalls the launcher"
fi
if rm_ssh 'test -d /home/root/xovi/exthome/appload/shims'; then
    ok "qtfb shims are in exthome/appload/shims"
else
    warn "no exthome/appload/shims — windowed apps will fail with"
    warn "      \"qtfb server rejected init\" even if the launcher appears"
fi

# AppLoad injects its launcher into xochitl's UI through qt-resource-rebuilder,
# whose hashtable is built per OS version. After an OS update the old table no
# longer matches and the entry can silently stop appearing.
HASH="$(rm_ssh 'ls /home/root/xovi/exthome/qt-resource-rebuilder/*.dat /home/root/xovi/exthome/qt-resource-rebuilder/hashtab* 2>/dev/null | head -n 1' || true)"
if [ -n "$HASH" ]; then
    ok "qt-resource-rebuilder hashtable present ($HASH)"
else
    warn "no qt-resource-rebuilder hashtable — AppLoad's entry in the stock UI"
    warn "      may not draw. Rebuild it (needs the tablet online):"
    fix "ssh root@$RM_HOST '/home/root/xovi/rebuild_hashtable' && ssh root@$RM_HOST '/home/root/xovi/start'"
fi

if [ -n "$XPID" ] && rm_ssh "grep -q appload /proc/$XPID/maps 2>/dev/null"; then
    ok "appload.so is mapped into xochitl — the launcher is live right now"
fi

# --- 4. the g-pad bundle inside AppLoad --------------------------------------
say "g-pad bundle"
FOUND=0
for app in g-pad g-pad-windowed; do
    if rm_ssh "test -x /home/root/xovi/exthome/appload/$app/g-pad"; then
        ok "exthome/appload/$app is installed"
        FOUND=1
        if rm_ssh "test -f /home/root/xovi/exthome/appload/$app/oracle.env"; then
            ok "  $app/oracle.env exists"
        else
            bad "  $app has no oracle.env — it will start but never reply"
            fix "copy oracle.env.example to /home/root/xovi/exthome/appload/$app/oracle.env and set RIDDLE_OPENAI_KEY"
        fi
    fi
done
[ "$FOUND" -eq 1 ] || {
    bad "no g-pad bundle under exthome/appload"
    fix "./build-rm2.sh && RM_HOST=$RM_HOST ./scripts/install-rm2.sh"
}

# --- verdict -----------------------------------------------------------------
printf '\n\033[1mVerdict\033[0m\n'
if [ "${#VERDICT[@]}" -eq 0 ]; then
    echo "  Everything the pad needs is installed and loaded. If AppLoad still is"
    echo "  not on screen, open it from xochitl's menu and tap Reload; if the entry"
    echo "  itself is absent, rebuild the hashtable and restart xovi:"
    echo "    ssh root@$RM_HOST '/home/root/xovi/rebuild_hashtable && /home/root/xovi/start'"
else
    printf '  %s\n' "${VERDICT[@]}"
fi
if [ "${#FIXES[@]}" -gt 0 ]; then
    printf '\n\033[1mDo this\033[0m\n'
    printf '  %s\n' "${FIXES[@]}"
fi
