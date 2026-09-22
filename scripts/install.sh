#!/usr/bin/env bash
# Anthink on a reMarkable 2: one command, from a stock tablet to the pad
# owning the screen. Nothing to build, no developer mode, no launcher.
#
#   curl -fsSL https://github.com/m4ndolore/g-pad/releases/latest/download/install.sh | bash
#
# What it does, over SSH:
#   1. finds the tablet (USB, then an iPhone hotspot; or RM_HOST=<ip>)
#   2. installs this machine's SSH key, so the root password is typed once
#   3. checks it is a reMarkable 2 on the OS this release was built for
#   4. downloads the release bundle, verifies its checksum, copies it over
#   5. installs the boot unit: the pad owns the panel from power-on
#   6. asks for an API key and writes oracle.env
#   7. starts the pad
#
# Options, as environment variables or flags:
#   RM_HOST=192.168.1.42        tablet on your Wi-Fi (Settings > Wi-Fi shows the IP)
#   ANTHINK_VERSION=v0.2.0      pin a release (default: the latest)
#   ANTHINK_BUNDLE=<dir>        install a local build instead of downloading
#   --uninstall                 disable the boot unit and put the stock UI back
#
# Leaving the pad once it runs: hold five fingers on the glass, or press the
# power button within three seconds of the boot card. Over SSH:
#   systemctl stop g-pad-takeover        exits this session
#   touch /home/root/g-pad-boot-off      keeps it off at boot (rm to re-enable)
set -euo pipefail

REPO="m4ndolore/g-pad"
RM_HOST="${RM_HOST:-}"
ANTHINK_VERSION="${ANTHINK_VERSION:-latest}"
ANTHINK_BUNDLE="${ANTHINK_BUNDLE:-}"
APP=/home/root/xovi/exthome/appload/g-pad
UNIT=g-pad-takeover.service
USB_HOST=10.11.99.1
HOTSPOT_NET=172.20.10
MODE=install
for arg in "$@"; do
    case "$arg" in
        --uninstall) MODE=uninstall ;;
        -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

say()  { printf '\n\033[1m== %s\033[0m\n' "$*"; }
note() { printf '   %s\n' "$*"; }
die()  { printf '\nerror: %s\n' "$*" >&2; exit 1; }
# Prompts read the terminal, not stdin: under `curl | bash`, stdin is the script.
ask()  { if [ -r /dev/tty ]; then printf '%s' "$1" >/dev/tty; IFS= read -r REPLY </dev/tty; else REPLY=""; fi; }

for tool in ssh scp curl; do
    command -v "$tool" >/dev/null || die "this machine has no '$tool'"
done
SHA256="$(command -v sha256sum || command -v shasum || true)"
[ -n "$SHA256" ] || die "this machine has neither sha256sum nor shasum"
case "$SHA256" in *shasum) SHA256="$SHA256 -a 256" ;; esac

WORK="$(mktemp -d)"
# rM2 SSH quirks: dropbear on OS 3.x hangs up when RSA is negotiated, and
# older firmware offers nothing but ssh-rsa. One multiplexed connection
# carries the whole install, so the password is typed at most once.
SSH_OPTS=(-o HostKeyAlgorithms=ssh-ed25519,ssh-rsa -o PubkeyAcceptedAlgorithms=+ssh-rsa
          -o ControlMaster=auto -o "ControlPath=$WORK/ssh-ctrl" -o ControlPersist=300
          -o ConnectTimeout=10)
rm_ssh()       { ssh -n "${SSH_OPTS[@]}" "root@$RM_HOST" "$@"; }
rm_ssh_stdin() { ssh "${SSH_OPTS[@]}" "root@$RM_HOST" "$@"; }
rm_scp()       { scp -O -q "${SSH_OPTS[@]}" "$@"; }
cleanup() { [ -n "$RM_HOST" ] && ssh "${SSH_OPTS[@]}" -O exit "root@$RM_HOST" 2>/dev/null || true; rm -rf "$WORK"; }
trap cleanup EXIT

# --- 1. find the tablet -------------------------------------------------------
# Over USB the rM2 is always 10.11.99.1, and nothing else ever is, so an
# answer there ends the search. Only when USB is silent do we try an iPhone
# or iPad Personal Hotspot, whose clients are 172.20.10.2 through .14.
# Anything else needs RM_HOST.
#
# macOS ships a netcat whose -w is an idle timeout: a probe of a dead address
# waits the kernel's 75 s for the connect. Its -G flag caps the connect
# instead. Every other netcat applies -w to the connect already.
NC_PROBE=(-z -w 1)
if command -v nc >/dev/null && { nc -h 2>&1 || true; } | grep -qE '(^|[[:space:]])-G[[:space:]]'; then
    NC_PROBE=(-z -G 1 -w 1)
fi
port22() {
    if command -v nc >/dev/null; then
        nc "${NC_PROBE[@]}" "$1" 22 >/dev/null 2>&1
    else
        out="$(ssh -n -o BatchMode=yes -o ConnectTimeout=2 -o StrictHostKeyChecking=no "root@$1" true 2>&1)" && return 0
        printf '%s' "$out" | grep -qiE 'denied|publickey|password'
    fi
}
if [ -z "$RM_HOST" ]; then
    say "Looking for the tablet"
    printf '   USB '
    if port22 "$USB_HOST"; then
        RM_HOST="$USB_HOST"
        printf 'answered\n'
    else
        printf 'is silent; the iPhone hotspot range '
        FOUND=()
        for h in $(seq 2 14 | sed "s|^|$HOTSPOT_NET.|"); do
            printf '.'
            port22 "$h" && FOUND+=("$h")
        done
        printf '\n'
        case "${#FOUND[@]}" in
            0) die "no tablet answered. Plug it in over USB and make sure it is awake, or run with RM_HOST=<ip> (Settings > Wi-Fi shows it)" ;;
            1) RM_HOST="${FOUND[0]}" ;;
            *) die "more than one host answered (${FOUND[*]}); re-run with RM_HOST=<the tablet>" ;;
        esac
    fi
fi
note "using root@$RM_HOST"

# --- 2. this machine's key on the tablet -------------------------------------
if ! ssh -n "${SSH_OPTS[@]}" -o BatchMode=yes "root@$RM_HOST" true 2>/dev/null; then
    say "Installing your SSH key"
    if ! ls "$HOME"/.ssh/id_*.pub >/dev/null 2>&1; then
        note "no SSH key on this machine yet; making one"
        ssh-keygen -q -t ed25519 -N '' -f "$HOME/.ssh/id_ed25519"
    fi
    note "the tablet's password is under Settings > Help > Copyrights and licenses,"
    note "at the bottom of the GPLv3 Compliance page. Type it once:"
    ssh-copy-id "${SSH_OPTS[@]}" "root@$RM_HOST" >/dev/null 2>&1 \
        || die "could not log in to root@$RM_HOST. If ssh hung up on you, run: ssh-keygen -R $RM_HOST"
fi

# --- uninstall ----------------------------------------------------------------
if [ "$MODE" = uninstall ]; then
    say "Putting the stock UI back"
    rm_ssh "systemctl stop $UNIT 2>/dev/null; systemctl disable $UNIT 2>/dev/null; \
            rm -f /etc/systemd/system/$UNIT; systemctl daemon-reload; \
            for f in poweroff rebooting; do \
              [ -e /home/root/g-pad-stock-images/\$f.png ] && cp -f /home/root/g-pad-stock-images/\$f.png /usr/share/remarkable/\$f.png; \
            done; systemctl start --no-block xochitl; true"
    note "the boot unit is gone and the shutdown screens are stock again."
    note "your pages and settings are still under $APP and /home/root/g-pad-data;"
    note "remove them with: ssh root@$RM_HOST 'rm -rf $APP /home/root/g-pad-data'"
    exit 0
fi

# --- 3. the right tablet, the right OS ---------------------------------------
say "Checking the tablet"
MACHINE="$(rm_ssh 'cat /sys/devices/soc0/machine 2>/dev/null' || true)"
case "$MACHINE" in
    "reMarkable 2.0") note "reMarkable 2" ;;
    *) die "this release is for the reMarkable 2; the device says '${MACHINE:-unknown}'" ;;
esac
rm_ssh 'test -f /usr/lib/plugins/scenegraph/libqsgepaper.so' \
    || die "the tablet has no e-ink engine library at /usr/lib/plugins/scenegraph; is this a stock OS?"
OSVER="$(rm_ssh 'sed -n "s/^REMARKABLE_RELEASE_VERSION=//p" /usr/share/remarkable/update.conf 2>/dev/null' || true)"
note "OS ${OSVER:-unknown}"

# --- 4. the bundle ---------------------------------------------------------------
say "Getting the bundle"
if [ -n "$ANTHINK_BUNDLE" ]; then
    BUNDLE="$ANTHINK_BUNDLE"
    note "local build at $BUNDLE"
else
    if [ "$ANTHINK_VERSION" = latest ]; then
        ANTHINK_VERSION="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
            | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
        [ -n "$ANTHINK_VERSION" ] || die "could not read the latest release from GitHub"
    fi
    BASE="https://github.com/$REPO/releases/download/$ANTHINK_VERSION"
    ZIP="anthink-rm2-$ANTHINK_VERSION.zip"
    note "release $ANTHINK_VERSION"
    curl -fsSL --retry 3 -o "$WORK/$ZIP" "$BASE/$ZIP" || die "no $ZIP in release $ANTHINK_VERSION"
    curl -fsSL --retry 3 -o "$WORK/SHA256SUMS" "$BASE/SHA256SUMS" || die "release $ANTHINK_VERSION has no SHA256SUMS"
    WANT="$(sed -n "s/^\([0-9a-f]\{64\}\)  *$ZIP\$/\1/p" "$WORK/SHA256SUMS")"
    [ -n "$WANT" ] || die "SHA256SUMS does not list $ZIP"
    GOT="$($SHA256 "$WORK/$ZIP" | cut -d' ' -f1)"
    [ "$GOT" = "$WANT" ] || die "checksum mismatch for $ZIP: the download is corrupt or tampered with"
    note "checksum verified"
    command -v unzip >/dev/null || die "this machine has no 'unzip'"
    unzip -q "$WORK/$ZIP" -d "$WORK/bundle"
    BUNDLE="$WORK/bundle/g-pad"
fi
for f in g-pad libquill.so g-pad-boot.sh g-pad-takeover.sh g-pad-restore.sh "$UNIT" cards/poweroff.png cards/rebooting.png; do
    [ -e "$BUNDLE/$f" ] || die "the bundle is missing $f"
done
[ ! -e "$BUNDLE/libqsgepaper.so" ] || die "this bundle contains reMarkable's proprietary library; refusing to install it"
BUILT_FOR="$(sed -n 's/^built_for=//p' "$BUNDLE/release.txt" 2>/dev/null || true)"
if [ -n "$BUILT_FOR" ] && [ -n "$OSVER" ] && [ "${OSVER%.*.*}" != "$BUILT_FOR" ]; then
    note "this release was built for OS $BUILT_FOR and the tablet runs $OSVER."
    note "the e-ink engine's interface changes between versions; the pad may start and never draw."
    ask "   continue anyway? [y/N] "
    case "$REPLY" in y|Y|yes) ;; *) die "stopped. A release for $OSVER may exist at https://github.com/$REPO/releases" ;; esac
fi

# --- 5. onto the tablet ----------------------------------------------------------
say "Installing"
if rm_ssh "systemctl is-active --quiet $UNIT"; then
    note "the pad is running; stopping it first (the stock UI comes back for a moment)"
    rm_ssh "systemctl stop $UNIT"
    for _ in $(seq 1 30); do
        rm_ssh "systemctl is-active --quiet xochitl" && break
        sleep 1
    done
fi
rm_ssh "rm -rf /tmp/anthink-bundle && mkdir -p /tmp/anthink-bundle $APP"
rm_scp -r "$BUNDLE/." "root@$RM_HOST:/tmp/anthink-bundle/"
# oracle.env is not in the bundle, so an existing key survives the copy.
rm_ssh "cp -Rf /tmp/anthink-bundle/. $APP/ && rm -rf /tmp/anthink-bundle && chmod +x $APP/g-pad $APP/*.sh"
rm_ssh "cp -f $APP/$UNIT /etc/systemd/system/$UNIT && systemctl daemon-reload && systemctl enable --quiet $UNIT"
# The OS's own power-off and reboot screens become the Anthink cards. The
# stock images are kept once under /home/root, which survives OS updates.
rm_ssh 'mkdir -p /home/root/g-pad-stock-images && for f in poweroff rebooting; do
    [ -e /home/root/g-pad-stock-images/$f.png ] || cp /usr/share/remarkable/$f.png /home/root/g-pad-stock-images/; done'
rm_ssh "cp -f $APP/cards/poweroff.png $APP/cards/rebooting.png /usr/share/remarkable/"
note "bundle in $APP, boot unit enabled"

# --- 6. the key --------------------------------------------------------------------
if ! rm_ssh "test -f $APP/oracle.env"; then
    say "The model"
    note "any OpenAI-compatible key whose model can see images. OpenAI and OpenRouter keys"
    note "are recognised; leave it empty to add one later in $APP/oracle.env."
    ask "   API key: "
    KEY="$REPLY"
    if [ -n "$KEY" ]; then
        case "$KEY" in
            sk-or-*) DEF_BASE="https://openrouter.ai/api/v1"; DEF_MODEL="openai/gpt-4o-mini" ;;
            *)       DEF_BASE="https://api.openai.com/v1";    DEF_MODEL="gpt-4o-mini" ;;
        esac
        ask "   API base URL [$DEF_BASE]: "; BASE="${REPLY:-$DEF_BASE}"
        ask "   vision model [$DEF_MODEL]: ";  MODEL="${REPLY:-$DEF_MODEL}"
        rm_ssh_stdin "cat > $APP/oracle.env" <<EOF
RIDDLE_OPENAI_KEY=$KEY
RIDDLE_OPENAI_BASE=$BASE
RIDDLE_OPENAI_MODEL=$MODEL
EOF
        note "checking the key from the tablet (needs the tablet on Wi-Fi; USB alone has no internet)"
        if rm_ssh "cd $APP && set -a && . ./oracle.env && set +a && \
                   LD_LIBRARY_PATH=$APP:/usr/lib/plugins/scenegraph ./g-pad --oracle-test icon.png >/dev/null 2>&1"; then
            note "the model answered"
        else
            note "no answer yet. Once the tablet is on Wi-Fi, the SYSTEM page shows the oracle state,"
            note "or test over ssh: cd $APP && . ./oracle.env && ./g-pad --oracle-test icon.png"
        fi
    fi
fi

# --- 7. start ------------------------------------------------------------------------
say "Starting the pad"
rm_ssh "systemctl start --no-block $UNIT"
cat <<EOF

   The Anthink card comes up, then a blank page. Write on it, and rule a
   long line beneath your words. The reply writes itself back.

   Leave:          hold five fingers on the glass
   Skip at boot:   press the power button within 3 s of the card
   Settings:       swipe down from the top edge (Wi-Fi, model, power)
   Turn it off:    ssh root@$RM_HOST 'touch /home/root/g-pad-boot-off'
   Uninstall:      re-run this installer with --uninstall

EOF
