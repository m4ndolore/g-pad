# g-pad on the reMarkable 2 — setup from zero

This guide installs the Quill **takeover** build: the pad stops the stock UI
and drives the e-ink engine directly, which is what gives it instant ink, the
touch gestures, and power handling. The stock UI stays installed underneath as
the fallback. Keep SSH available as a recovery path the first time you launch
it.

The rM2 needs no "developer mode": SSH as root is built into every unit.
You need: the tablet, its USB-C cable, and ~15 minutes.

## Quick path (one command)

1. On the tablet, read the root password: **Settings → Help → Copyrights and
   licenses → GPLv3 Compliance** (bottom of the page; the IP shown is
   `10.11.99.1`). Note the password somewhere safe.
2. Plug the tablet in over USB.
3. On any machine with `ssh`:

```sh
curl -fsSL https://github.com/m4ndolore/g-pad/releases/latest/download/install.sh | bash
```

The installer (`scripts/install.sh`, also attached to every release) finds the
tablet over USB or an iPhone hotspot, installs your SSH key so the password is
typed once, confirms the device is an rM2 and reads its OS version, downloads
the release bundle and checks it against `SHA256SUMS`, copies it to
`/home/root/xovi/exthome/appload/g-pad`, installs the boot unit and the Anthink
shutdown cards, prompts for your API key, and starts the pad. It never installs
xovi or AppLoad: the boot unit starts the pad directly, and on OS 3.28 AppLoad
does not work anyway (see "After a reMarkable OS update").

Options: `RM_HOST=<ip>` for a tablet on your Wi-Fi, `ANTHINK_VERSION=vX.Y.Z` to
pin a release, `ANTHINK_BUNDLE=dist/rm2-takeover/g-pad` to install a build of
your own, and `--uninstall` to disable the boot unit and restore the stock
shutdown images.

> ⚠️ Everything here is reversible (the five-finger hold, the power button in
> the boot window, `ssh root@10.11.99.1 'systemctl stop g-pad-takeover'`, or
> the installer's `--uninstall`), but a reMarkable OS update removes the boot
> unit and the shutdown cards. Re-run the installer afterwards. Keep the SSH
> password: it is your escape hatch.

### If SSH won't connect

Two rM2 SSH quirks, both handled by the installer for its own connections.
For your own `ssh`/`scp` sessions:

- **`Connection closed by 10.11.99.1 port 22`** — the rM2's dropbear (OS 3.x)
  has a broken RSA host-key path: it hangs up whenever RSA is negotiated,
  though ed25519 works fine. A stale `ssh-rsa` entry for `10.11.99.1` in your
  `~/.ssh/known_hosts` (from an older device or firmware) forces your client
  to request RSA and triggers exactly this. Fix:
  `ssh-keygen -R 10.11.99.1`, then connect again.
- **`no matching host key type found. Their offer: ssh-rsa`** — older firmware
  offers only legacy `ssh-rsa`, which modern OpenSSH refuses by default.

This `~/.ssh/config` block covers both:

```
Host remarkable rm2 10.11.99.1
  HostName 10.11.99.1
  User root
  HostKeyAlgorithms ssh-ed25519,ssh-rsa
  PubkeyAcceptedAlgorithms +ssh-rsa
```

## Takeover build

Takeover drives the vendor e-ink engine directly with xochitl stopped. It needs
two artifacts the repository deliberately does not ship:

- `quill/vendor/armv7-unknown-linux-gnueabihf/libqsgepaper.so` — reMarkable's
  proprietary library, copied from a tablet **you own**. It is gitignored and
  CI-guarded; never commit it.
- `quill/build/armv7-unknown-linux-gnueabihf/libquill.so` — the clean-room
  adapter, compiled from `quill/src`.

```sh
scp -O root@10.11.99.1:/usr/lib/plugins/scenegraph/libqsgepaper.so \
    quill/vendor/armv7-unknown-linux-gnueabihf/
```

Compiling `libquill.so` needs Qt headers and the tablet's libraries. With the
reMarkable SDK installed, `DEVICE=rm2 ./build-takeover.sh` (SDK at
`~/rm-sdk-rm2`) does everything; without it, `./quill/build-zig.sh` builds
the adapter with zig alone (see `quill/README.md`), pulling what it needs from
the tablet over ssh.

Once `libquill.so` exists, the Rust side does not need the SDK at all —
`cargo-zigbuild` supplies the cross-linker:

```sh
./build-takeover-zig.sh                 # brew install zig cargo-zigbuild
DEVICE=rm2 ./scripts/make-bundle.sh     # stages dist/rm2-takeover/g-pad with the cards
ANTHINK_BUNDLE=dist/rm2-takeover/g-pad ./scripts/install.sh
```

The installer with `ANTHINK_BUNDLE` does exactly what the release path does,
from your build instead of a download. Leave takeover with a **five-finger
hold**; if it ever exits badly, `ssh root@10.11.99.1 'systemctl start xochitl'`
restores the stock UI.

## The oracle key

Any OpenAI-compatible, vision-capable endpoint works. The installer writes
`oracle.env` for you; to change it later, edit
`/home/root/xovi/exthome/appload/g-pad/oracle.env` on the tablet. OpenRouter
example:

```sh
RIDDLE_OPENAI_KEY=sk-or-v1-...
RIDDLE_OPENAI_BASE=https://openrouter.ai/api/v1
RIDDLE_OPENAI_MODEL=openai/gpt-4o-mini
```

Test it any time (tablet must be on Wi-Fi — USB gives it no internet):

```sh
ssh rm2 'cd /home/root/xovi/exthome/appload/g-pad && \
  set -a && . ./oracle.env && set +a && ./g-pad --oracle-test icon.png'
```

## In-app SYSTEM page

Routine changes no longer need ssh. The SYSTEM page (top or bottom edge swipe
in Stealth, SETTINGS on the control strip in Guided) handles:

- oracle preset, model, ask model, reasoning effort, max tokens
- palm holdoff; idle-send and its delay
- tutor model and next-page dwell for Learn
- Wi-Fi status, rescan, and joining a saved network
- sleep, reboot, power off, leave to stock UI
- battery, storage, clock, OS version, running build, hub reachability
  (read-only facts)

After REBOOT or POWER OFF (and the next power-on) the pad comes back by
itself: the boot-persistent `g-pad-takeover.service` the installer enables
starts it in place of the stock UI. What does
not come back is AppLoad — xovi is not loaded at boot. That only matters if
you LEAVE TO STOCK UI and want the AppLoad entries; then
`ssh rm2 /home/root/xovi/start`.

Still over ssh:

- a new API key: `RIDDLE_OPENAI_KEY` in `oracle.env`. The page shows SET or
  MISSING and never writes the key.
- a new Wi-Fi network and its password. The page selects among networks the
  tablet already knows; an unsaved network in the scan list is inert.
- new presets: edit `settings.schema.json` beside the binary. It is read at
  start, so a new preset appears after the next stop → settle → start; no
  rebuild.

Page changes are written to `/home/root/g-pad-data/overrides` and beat
`oracle.env` until removed. The file lives under `g-pad-data`, not the app
folder, so a rebuild or redeploy keeps it. RESET OVERRIDES on the page removes
every override and re-spawns the oracle from `oracle.env` at once; the ssh
equivalent is

```sh
ssh rm2 rm -f /home/root/g-pad-data/overrides
```

which takes effect at the next start, since a running pad already holds the
overridden values in its environment.

## Manual path (what the installer does, step by step)

If you prefer to run each step yourself, with `APP=/home/root/xovi/exthome/appload/g-pad`:

1. **The bundle** — unzip `anthink-rm2-<tag>.zip` from the
   [latest release](https://github.com/m4ndolore/g-pad/releases/latest) and
   `scp -O -r g-pad root@10.11.99.1:/home/root/xovi/exthome/appload/`. Or stage
   your own with `DEVICE=rm2 ./scripts/make-bundle.sh` and copy
   `dist/rm2-takeover/g-pad` the same way.
2. **The boot unit** — on the tablet: `cp $APP/g-pad-takeover.service
   /etc/systemd/system/ && systemctl daemon-reload && systemctl enable
   g-pad-takeover`. The unit runs `$APP/g-pad-boot.sh`, which holds the
   three-second escape window and then hands over to `g-pad-takeover.sh`.
3. **The shutdown cards** — keep the stock images once, then copy the pad's:
   `mkdir -p /home/root/g-pad-stock-images && cp /usr/share/remarkable/{poweroff,rebooting}.png /home/root/g-pad-stock-images/`
   and `cp $APP/cards/{poweroff,rebooting}.png /usr/share/remarkable/`.
4. **The key** — create `$APP/oracle.env` (see "The oracle key" above).
5. **Start** — `systemctl start g-pad-takeover` on the tablet, or reboot.

xovi and AppLoad are not part of this. If you already have them and want an
AppLoad entry as well, the bundle's `appload-launch.sh` starts the same unit.

## After a reMarkable OS update

An update installs the new OS on the other root partition and boots it. Your
files under `/home/root` survive; everything that lived in the old root does
not. Seen on the 3.27.3 → 3.28.0.172 update (2026-09-15):

- **The SSH host key changes** (dropbear regenerates it on the fresh root).
  Your client refuses to connect with "REMOTE HOST IDENTIFICATION HAS
  CHANGED", and so does anything scripted on top of ssh, such as the hub
  tunnel. Fix: `ssh-keygen -R 10.11.99.1` (and the Wi-Fi address), then
  connect once to accept the new key.
- **The takeover unit is gone** (`/etc/systemd/system/g-pad-takeover.service`),
  so the pad no longer owns the panel at boot. Re-run the installer
  (`scripts/install.sh`). The bundle and your pages under `/home/root` are
  untouched.
- **The qt-resource-rebuilder hashtable is stale.** It names the OS it was
  built for, qmldiff skips it on any other version, and AppLoad's hooks then
  abort xochitl; on 3.28 the OS answers two aborts with a reboot. Rebuild it
  before starting xovi: `ssh root@10.11.99.1 '/home/root/xovi/rebuild_hashtable </dev/null'`
  (`rm2-doctor.sh` reports the mismatch).
- **AppLoad itself may lag the OS.** 3.28 changed the QML the launcher hooks;
  the fix is merged upstream but, as of 2026-09-15, not in an AppLoad release,
  and AppLoad v0.5.3 aborts xochitl on 3.28 even with a fresh hashtable. Until
  a release ships, do not run `/home/root/xovi/start` on 3.28. The takeover
  does not need AppLoad: the boot unit starts g-pad directly.
- **The shutdown images revert to stock** (`/usr/share/remarkable/poweroff.png`
  and `rebooting.png` live on the root partition). POWER OFF and REBOOT show
  the reMarkable screens again instead of the Anthink cards. Re-run the
  installer, which carries the cards in the bundle and re-installs them
  (`rm2-doctor.sh` reports it).
- **3.28 dropped `timeout` and `pkill` from busybox.** Scripts that relied on
  them stop working silently. The boot escape window lives inside g-pad now;
  `g-pad-boot.sh` holds it itself only when the binary will not start, with
  bash's own timed `read`.
- **The e-ink engine ABI changed.** `libqsgepaper.so` on 3.28 exports a
  different `EPFramebuffer::swapBuffers` signature, so a `libquill.so` built
  for 3.27 initializes but never updates the screen. Rebuild it with
  `./quill/build-zig.sh` (no SDK needed) and ship it with the bundle.

## Troubleshooting

- **The pad is not on the screen** — either it was left (five fingers, the
  power button in the boot window, or a `systemctl stop`) and returns at the
  next boot, or an OS update removed the boot unit, or the bundle was built
  for a different OS than the tablet now runs. Ask the tablet which one it is:

  ```sh
  ./scripts/rm2-doctor.sh
  ```

- **AppLoad is missing from the tablet** — only relevant if you launch from
  the stock UI's AppLoad entry, which the pad does not need. AppLoad exists
  only while xovi is loaded into a *running* xochitl, so the entry disappears
  after a reboot without xovi-tripletap or after an OS update, which can also
  invalidate the per-version qt hashtable its entry is drawn through. The
  doctor reports the state when xovi is installed.

  It is read-only — it finds the tablet, reports what is installed, what is
  loaded into the running xochitl, and prints the one command that fixes what
  it found. The two answers it usually lands on:

  ```sh
  ssh root@<tablet-ip> '/home/root/xovi/start'                       # loader not running
  ssh root@<tablet-ip> '/home/root/xovi/rebuild_hashtable && /home/root/xovi/start'   # after an OS update
  ```

- **You don't know the tablet's IP** — over Wi-Fi it is *not* `10.11.99.1`;
  that address is the USB link only. Two cases need no hunting, and
  `rm2-doctor.sh` and `install.sh` try both before they ask you for anything:
  USB first, and if USB is silent, an iPhone/iPad Personal Hotspot, which
  always uses `172.20.10.0/28` — the phone is `.1` and clients get `.2` through
  `.14`, so it is thirteen addresses, not a search. An answer on USB ends the
  search, so a tablet that is plugged in and on a hotspot at once is found
  once, not twice. On any other network the tablet will tell you: Settings →
  Wi-Fi, tap the connected network. Pass it as `RM_HOST` to the doctor and to
  `install.sh`.

  Whatever the network, *this computer has to be on it too* — a hotspot only
  routes between its own clients, so sharing from a phone the laptop hasn't
  joined reaches nothing. USB sidesteps the whole question and is the easier
  path for fixing AppLoad; the tablet only needs real Wi-Fi for the oracle,
  since USB gives it no internet. The root password is at Settings → General →
  Help → Copyrights and licenses, under GPLv3 Compliance.

- **No computer to hand (iPad or phone only)** — you can still diagnose and fix
  a missing AppLoad. Only *building* g-pad needs a computer; xovi and AppLoad
  install from the tablet itself.

  If the tablet is on a phone/iPad Personal Hotspot, the device sharing that
  hotspot is the gateway and can reach its own clients, so SSH from it works.
  Don't rely on the USB cable here — the rM2 reaches `10.11.99.1` by presenting
  a USB ethernet gadget, and iPadOS will not reliably bring that up as a network
  interface. Install an SSH client (Termius, Blink, a-Shell), then
  `ssh root@172.20.10.2` — password at Settings → General → Help → Copyrights
  and licenses, under GPLv3 Compliance. Paste these on the tablet:

  ```sh
  cat /sys/devices/soc0/machine
  [ -x /home/root/xovi/start ] && echo XOVI-INSTALLED || echo XOVI-MISSING
  grep -q xovi /proc/$(pidof xochitl)/maps && echo XOVI-LOADED || echo XOVI-NOT-LOADED
  [ -f /home/root/xovi/extensions.d/appload.so ] && echo APPLOAD-PRESENT || echo APPLOAD-MISSING
  ls /home/root/xovi/exthome/appload/
  ```

  The same four states `rm2-doctor.sh` reports, and the same fixes:
  `XOVI-INSTALLED` + `XOVI-NOT-LOADED` → `/home/root/xovi/start`. Loaded but no
  launcher on screen → `/home/root/xovi/rebuild_hashtable && /home/root/xovi/start`.
  `XOVI-MISSING` or `APPLOAD-MISSING` → the manual path above installs both with
  nothing but `wget` on the tablet, which has internet through the hotspot. Only
  the g-pad bundle itself has to be cross-compiled elsewhere.

- **Ink lands in the wrong place / mirrored** — the raw digitizer transform is
  off for your unit. Open an issue with what you see and the OS version.
- **The card comes up and nothing draws after it** — the bundle's
  `libquill.so` was built for a different OS than the tablet runs (the e-ink
  engine's interface changed at 3.28). `rm2-doctor.sh` compares the release
  stamp with the OS; install a matching release or rebuild with
  `./quill/build-zig.sh`.
- **No reply, ink blot pulses forever** — oracle problem: re-run
  `--oracle-test`; check Wi-Fi, key, and that the model supports images. The
  SYSTEM page shows the oracle state without ssh.
- **Tablet acting up** — `ssh root@10.11.99.1 'systemctl stop g-pad-takeover'`
  exits the pad and brings the stock UI back; worst case hold power ~10 s to
  reboot, and press the power button once during the boot card to stay on
  the stock UI for that boot. Never `systemctl restart` the pad's unit: stop,
  wait for the stock UI, then start.
