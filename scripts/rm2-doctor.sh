#!/usr/bin/env bash
# Anthink rM2 doctor — why the pad is not on the screen.
#
#   Usage:  ./scripts/rm2-doctor.sh                      # finds the tablet itself
#           RM_HOST=192.168.1.42 ./scripts/rm2-doctor.sh # or tell it where to look
#
# Read-only: it looks, reports, and prints the exact command that fixes each
# thing it finds. It never changes the tablet.
#
# It checks, in order: the bundle and its release stamp against the OS
# version, the boot unit (present, enabled, running), the Anthink shutdown
# images (an OS update puts the stock ones back), and, only if they are
# installed, xovi and AppLoad.
set -euo pipefail

RM_HOST="${RM_HOST:-}"          # empty = go and find it

USB_HOST=10.11.99.1             # the USB-C link, always this address
HOTSPOT_NET=172.20.10           # iOS Personal Hotspot is always this /28

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

# --- finding the tablet ------------------------------------------------------
# Two addresses need no hunting at all. Over USB the rM2 is always 10.11.99.1,
# and an answer there ends the search. On an iPhone/iPad Personal Hotspot the
# subnet is always 172.20.10.0/28 — the phone is .1 and clients get .2 through
# .14 — so "I don't know the IP" is a thirteen-address scan, not a search.
# Anything else (home Wi-Fi) still needs RM_HOST, and the tablet will tell
# you: Settings > Wi-Fi, tap the connected network.
NC="$(command -v nc || true)"
# macOS ships a netcat whose -w is an idle timeout: a probe of a dead address
# waits the kernel's 75 s for the connect. Its -G flag caps the connect
# instead. Every other netcat applies -w to the connect already.
NC_PROBE=(-z -w 1)
if [ -n "$NC" ] && { "$NC" -h 2>&1 || true; } | grep -qE '(^|[[:space:]])-G[[:space:]]'; then
    NC_PROBE=(-z -G 1 -w 1)
fi

# Is anything listening on 22? nc when we have it (fast, no auth); otherwise ask
# ssh and read what it says — a refused key still proves an sshd answered.
port22() {
    if [ -n "$NC" ]; then
        "$NC" "${NC_PROBE[@]}" "$1" 22 >/dev/null 2>&1
    else
        out="$(ssh -n -o BatchMode=yes -o ConnectTimeout=2 \
                   -o StrictHostKeyChecking=no "root@$1" true 2>&1)" && return 0
        printf '%s' "$out" | grep -qiE 'denied|publickey|password'
    fi
}

# The rM2 runs dropbear, so its SSH banner identifies it before we authenticate.
banner() { [ -n "$NC" ] || return 0; "$NC" -w 2 "$1" 22 </dev/null 2>/dev/null | head -n 1 || true; }

discover() {
    say "Looking for the tablet (no RM_HOST given)"
    printf '  trying USB '
    FOUND=()
    if port22 "$USB_HOST"; then
        # Nothing but the tablet is ever 10.11.99.1; the hotspot scan would
        # only find the same tablet again and call it two hosts.
        FOUND=("$USB_HOST")
        printf 'answered\n'
    else
        printf 'is silent; the iOS hotspot range 172.20.10.2-14 '
        for h in $(seq 2 14 | sed "s|^|$HOTSPOT_NET.|"); do
            printf '.'
            port22 "$h" && FOUND+=("$h")
        done
        printf '\n'
    fi

    for h in "${FOUND[@]:-}"; do
        [ -n "$h" ] || continue
        B="$(banner "$h")"
        case "$B" in
            *[Dd]ropbear*) ok "$h answers on 22 — dropbear, which is what the rM2 runs" ;;
            "")            ok "$h answers on 22" ;;
            *)             warn "$h answers on 22 but its banner reads '$B' — probably not the tablet" ;;
        esac
    done

    case "${#FOUND[@]}" in
        0) cat >&2 <<EOF

  Found nothing to talk to.

  Three things to check, in this order:
    1. This computer must be on the same hotspot as the tablet. An iPad
       sharing its connection only routes between its own clients — join
       this machine to that hotspot too, or plug the tablet into USB, which
       needs no IP at all and is the easier path for fixing AppLoad.
    2. reMarkable turns Wi-Fi off when the sleep cover closes. Wake the
       tablet and leave it awake while this runs.
    3. If it is on some other network, read the address off the tablet:
       Settings > Wi-Fi, tap the connected network. Then:
         RM_HOST=<that-ip> $0

  The root password lives at Settings > General > Help > Copyrights and
  licenses, at the bottom under GPLv3 Compliance.
EOF
           exit 1 ;;
        1) RM_HOST="${FOUND[0]}"
           say "Using root@$RM_HOST" ;;
        *) printf '\n  More than one host answered. Pick the tablet and re-run:\n' >&2
           for h in "${FOUND[@]}"; do printf '    RM_HOST=%s %s\n' "$h" "$0" >&2; done
           exit 1 ;;
    esac
}

[ -n "$RM_HOST" ] || discover

say "Reaching root@$RM_HOST"
if ! rm_ssh true 2>/dev/null; then
    cat >&2 <<EOF
  something is listening at $RM_HOST, but SSH as root did not go through.

  If it asked for a password, this machine's key is not installed yet — the
  installer does that for you, and the password is on the tablet under
  Settings > General > Help > Copyrights and licenses (GPLv3 Compliance):

    curl -fsSL https://github.com/m4ndolore/g-pad/releases/latest/download/install.sh | RM_HOST=$RM_HOST bash

  If it hung up on you, clear a stale host key and retry:
    ssh-keygen -R $RM_HOST
EOF
    exit 1
fi
ok "SSH works"

MACHINE="$(rm_ssh 'cat /sys/devices/soc0/machine 2>/dev/null' || true)"
OSVER="$(rm_ssh 'sed -n "s/^REMARKABLE_RELEASE_VERSION=//p" /usr/share/remarkable/update.conf 2>/dev/null' || true)"
[ "$MACHINE" = "reMarkable 2.0" ] && ok "device: $MACHINE" || warn "device reports '${MACHINE:-unknown}' — this doctor is written for the rM2"
[ -n "$OSVER" ] && ok "reMarkable OS $OSVER"

INSTALL="curl -fsSL https://github.com/m4ndolore/g-pad/releases/latest/download/install.sh | RM_HOST=$RM_HOST bash"
APP=/home/root/xovi/exthome/appload/g-pad

# --- 1. the bundle ------------------------------------------------------------
say "The pad"
if rm_ssh "test -x $APP/g-pad"; then
    ok "$APP is installed"
    REL="$(rm_ssh "cat $APP/release.txt 2>/dev/null" || true)"
    if [ -n "$REL" ]; then
        VER="$(printf '%s\n' "$REL" | sed -n 's/^version=//p')"
        BUILT_FOR="$(printf '%s\n' "$REL" | sed -n 's/^built_for=//p')"
        ok "  release ${VER:-unknown}${BUILT_FOR:+, built for OS $BUILT_FOR}"
        if [ -n "$BUILT_FOR" ] && [ -n "$OSVER" ] && [ "${OSVER%.*.*}" != "$BUILT_FOR" ]; then
            bad "  the pad was built for OS $BUILT_FOR and the tablet runs $OSVER — the e-ink engine"
            bad "      interface differs between them, so the pad can start and never draw"
            fix "$INSTALL   # a release built for $OSVER, if one exists"
        fi
    else
        warn "  no release.txt — a bundle from before releases were stamped"
    fi
    if rm_ssh "test -f $APP/libquill.so"; then
        ok "  libquill.so is in the bundle"
    else
        bad "  libquill.so is missing from the bundle; the pad cannot open the panel"
        fix "$INSTALL"
    fi
    if rm_ssh "test -f $APP/oracle.env"; then
        ok "  oracle.env exists"
    else
        bad "  no oracle.env — the pad starts but never replies"
        fix "ssh root@$RM_HOST 'cp $APP/oracle.env.example $APP/oracle.env'   # then set RIDDLE_OPENAI_KEY in it"
    fi
else
    bad "no pad under $APP"
    fix "$INSTALL"
fi
rm_ssh 'test -f /usr/lib/plugins/scenegraph/libqsgepaper.so' \
    && ok "the OS's e-ink engine library is where the pad loads it from" \
    || bad "no /usr/lib/plugins/scenegraph/libqsgepaper.so — not a stock reMarkable OS?"

# --- 2. the boot unit -----------------------------------------------------------
# The product path: systemd starts the pad at boot, the stock UI is the
# fallback underneath. An OS update swaps the root partition and takes the
# unit with it; the bundle under /home/root survives.
say "Boot unit"
if rm_ssh 'test -f /etc/systemd/system/g-pad-takeover.service'; then
    ok "g-pad-takeover.service is installed"
    if rm_ssh 'systemctl is-enabled --quiet g-pad-takeover.service 2>/dev/null'; then
        ok "  enabled at boot"
    else
        bad "  not enabled: the pad will not come up at boot"
        fix "ssh root@$RM_HOST 'systemctl enable g-pad-takeover.service'"
    fi
    if rm_ssh 'test -e /home/root/g-pad-boot-off'; then
        warn "  /home/root/g-pad-boot-off exists — the pad is deliberately off at boot"
        warn "      (ssh root@$RM_HOST 'rm /home/root/g-pad-boot-off' to turn it back on)"
    fi
    STATE="$(rm_ssh 'systemctl is-active g-pad-takeover.service 2>/dev/null' || true)"
    case "$STATE" in
        active)   ok "  running now" ;;
        inactive) warn "  not running right now (left with five fingers, the power button, or a stop);"
                  warn "      it returns at the next boot, or: ssh root@$RM_HOST 'systemctl start g-pad-takeover'" ;;
        failed)   bad "  the unit failed on its last run"
                  fix "ssh root@$RM_HOST 'journalctl -u g-pad-takeover -b --no-pager | tail -n 40'" ;;
        *)        warn "  state: ${STATE:-unknown}" ;;
    esac
else
    bad "no boot unit — after an OS update this is expected: the unit lived on the old root partition"
    fix "$INSTALL   # reinstalls the unit and the shutdown cards; the bundle and your pages are kept"
fi

# --- 3. the Anthink shutdown images ---------------------------------------------
# The installer keeps the stock poweroff.png once under /home/root and
# overwrites the OS copy. An OS update replaces the root partition, so the OS
# copy being byte-identical to the kept stock file again means an update has
# undone the install.
say "Anthink shutdown images"
if rm_ssh 'test -e /home/root/g-pad-stock-images/poweroff.png'; then
    if rm_ssh 'cmp -s /usr/share/remarkable/poweroff.png /home/root/g-pad-stock-images/poweroff.png'; then
        bad "the OS shutdown images are back to stock — an OS update restored them"
        fix "$INSTALL   # re-installs the Anthink cards"
    else
        ok "power-off and reboot images are the Anthink cards"
    fi
else
    warn "Anthink shutdown images were never installed (the installer does it)"
fi

# --- 4. xovi and AppLoad, only if they are there ----------------------------------
# The pad does not need either: the boot unit starts it directly. They matter
# only to someone launching from the stock UI's AppLoad entry, and on OS 3.28
# AppLoad v0.5.3 aborts xochitl, so a stale install is worth knowing about.
say "xovi and AppLoad (optional)"
if ! rm_ssh 'test -x /home/root/xovi/start'; then
    ok "xovi is not installed; the boot unit does not need it"
else
    ok "/home/root/xovi is installed"
    XPID="$(rm_ssh 'pidof xochitl 2>/dev/null' || true)"
    if [ -n "$XPID" ] && rm_ssh "grep -q xovi /proc/$XPID/maps 2>/dev/null"; then
        ok "xovi is loaded into the running xochitl (pid $XPID)"
    else
        warn "xochitl is running without xovi (a reboot leaves it off); the pad does not mind"
    fi
    if rm_ssh 'test -f /home/root/xovi/extensions.d/appload.so'; then
        ok "extensions.d/appload.so is present"
    else
        warn "appload.so is not in /home/root/xovi/extensions.d"
    fi
    # AppLoad injects its launcher through qt-resource-rebuilder, whose
    # hashtable names the OS it was built for; qmldiff refuses any other
    # version and AppLoad's hooks then abort xochitl — on 3.28 twice in a
    # row, which makes the OS reboot.
    HASH="$(rm_ssh 'ls /home/root/xovi/exthome/qt-resource-rebuilder/*.dat /home/root/xovi/exthome/qt-resource-rebuilder/hashtab* 2>/dev/null | head -n 1' || true)"
    HASHVER="$([ -n "$HASH" ] && rm_ssh "strings $HASH | grep -m1 -E '^3\\.[0-9]+\\.[0-9]+\\.[0-9]+\$'" || true)"
    if [ -n "$HASH" ] && [ -n "$OSVER" ] && [ -n "$HASHVER" ] && [ "$HASHVER" != "$OSVER" ]; then
        bad "the xovi hashtable was built for OS $HASHVER, tablet runs $OSVER — starting xovi now aborts xochitl"
        fix "ssh root@$RM_HOST '/home/root/xovi/rebuild_hashtable </dev/null'   # and check AppLoad supports $OSVER before /home/root/xovi/start"
    elif [ -n "$HASH" ]; then
        ok "qt-resource-rebuilder hashtable present${HASHVER:+, built for $HASHVER}"
    fi
fi

# --- verdict -----------------------------------------------------------------
printf '\n\033[1mVerdict\033[0m\n'
if [ "${#VERDICT[@]}" -eq 0 ]; then
    echo "  Everything the pad needs is installed. If the screen shows the stock UI,"
    echo "  the pad was left with five fingers or the power button; it returns at the"
    echo "  next boot, or now with:"
    echo "    ssh root@$RM_HOST 'systemctl start g-pad-takeover'"
else
    printf '  %s\n' "${VERDICT[@]}"
fi
if [ "${#FIXES[@]}" -gt 0 ]; then
    printf '\n\033[1mDo this\033[0m\n'
    printf '  %s\n' "${FIXES[@]}"
fi
