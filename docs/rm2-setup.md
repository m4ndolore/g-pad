# g-pad on the reMarkable 2 — setup from zero

This guide installs the windowed AppLoad build. The direct Quill **takeover**
build is the one to want: it removes the qtfb latency floor and is the only
mode with touch gestures (history, settings, five-finger exit) and power
handling — windowed has none of those. See "Takeover build" below, and keep
SSH available as a recovery path the first time you launch it.

The rM2 needs no "developer mode": SSH as root is built into every unit.
You need: the tablet, its USB-C cable, and ~15 minutes.

## Quick path (one command)

1. On the tablet, read the root password: **Settings → Help → Copyrights and
   licenses → GPLv3 Compliance** (bottom of the page; the IP shown is
   `10.11.99.1`). Note the password somewhere safe.
2. Plug the tablet in over USB.
3. Build and install everything:

```sh
rustup target add armv7-unknown-linux-musleabihf   # once; needs zig + cargo-zigbuild
cd g-pad && ./build-rm2.sh && cd ..
./scripts/install-rm2.sh
```

The installer connects over SSH (asking for that password once, then installing
your key), confirms the device is an rM2, installs
[xovi](https://github.com/asivery/xovi) +
[AppLoad](https://github.com/asivery/rm-appload) from their official arm32
releases, adds power-button persistence via
[xovi-tripletap](https://github.com/rmitchellscott/xovi-tripletap)
(triple-press = toggle xovi), copies the riddle bundle, prompts for your API
key, and verifies the oracle end-to-end.

Then on the tablet: open **AppLoad → g-pad**, write, rest the pen ~3 s.

> ⚠️ Everything here is reversible (`ssh root@10.11.99.1
> /home/root/xovi/stock` or a reboot returns the stock UI), but reMarkable OS
> updates can remove xovi/AppLoad/riddle — reinstallable by re-running the
> installer. Keep the SSH password: it is your escape hatch.

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

Compiling `libquill.so` needs the reMarkable SDK for its Qt headers:

```sh
DEVICE=rm2 ./build-takeover.sh          # SDK at ~/rm-sdk-rm2
```

Once `libquill.so` exists, later builds do not need the SDK at all —
`cargo-zigbuild` supplies the cross-linker:

```sh
./build-takeover-zig.sh                 # brew install zig cargo-zigbuild
DEVICE=rm2 ./scripts/make-bundle.sh
scp -O -r dist/rm2-takeover/g-pad root@10.11.99.1:/home/root/xovi/exthome/appload/
```

The two bundles carry different AppLoad ids (`g-pad` for takeover,
`g-pad-windowed` for qtfb) so both can be installed side by side. Leave
takeover with a **five-finger hold**; if it ever exits badly,
`ssh root@10.11.99.1 'systemctl start xochitl'` restores the stock UI.

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

## Manual path (what the installer does, step by step)

If you prefer to run each step yourself:

1. **xovi** — grab `xovi-arm32.tar.gz` from
   [rm-xovi-extensions releases](https://github.com/asivery/rm-xovi-extensions/releases/latest)
   (it contains the loader, start/stop scripts, and qt-resource-rebuilder), and
   extract on the tablet: `tar -xzf xovi.tar.gz -C /home/root`.
2. **AppLoad** — grab `appload-arm32.zip` from
   [rm-appload releases](https://github.com/asivery/rm-appload/releases/latest).
   `appload.so` goes to `/home/root/xovi/extensions.d/`; the `shims/` folder
   goes to `/home/root/xovi/exthome/appload/shims/` (not extensions.d).
3. **Persistence** — on the tablet:
   `wget -qO- https://raw.githubusercontent.com/rmitchellscott/xovi-tripletap/main/install.sh | bash`
4. **riddle** — `scp -O -r dist/rm2/riddle root@10.11.99.1:/home/root/xovi/exthome/appload/`,
   then create `oracle.env` in that folder (see above).
5. **Start** — `/home/root/xovi/start` on the tablet (or triple-press power).

## Troubleshooting

- **AppLoad is missing from the tablet** — AppLoad is not a stock app. It exists
  only while xovi is loaded into a *running* xochitl, so the entry disappears
  whenever the loader is not in the process: after a reboot without
  xovi-tripletap, after a reMarkable OS update (which restarts xochitl and can
  invalidate the per-version qt hashtable AppLoad's entry is drawn through), or
  because xovi was never installed at all. Ask the tablet which one it is:

  ```sh
  ./scripts/rm2-doctor.sh
  ```

  It is read-only — it finds the tablet, reports what is installed, what is
  loaded into the running xochitl, and prints the one command that fixes what
  it found. The two answers it usually lands on:

  ```sh
  ssh root@<tablet-ip> '/home/root/xovi/start'                       # loader not running
  ssh root@<tablet-ip> '/home/root/xovi/rebuild_hashtable && /home/root/xovi/start'   # after an OS update
  ```

- **You don't know the tablet's IP** — over Wi-Fi it is *not* `10.11.99.1`;
  that address is the USB link only. Two cases need no hunting, and
  `rm2-doctor.sh` scans both before it asks you for anything: USB, and an
  iPhone/iPad Personal Hotspot, which always uses `172.20.10.0/28` — the phone
  is `.1` and clients get `.2` through `.14`, so it is thirteen addresses, not a
  search. On any other network the tablet will tell you: Settings → Wi-Fi, tap
  the connected network. Pass it as `RM_HOST` to the doctor and to
  `install-rm2.sh`.

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
  off for your unit. The qtfb pen fallback (used automatically when the raw
  device can't be opened) is always correctly mapped — compare against it and
  open an issue with what you see.
- **"qtfb server rejected init"** — AppLoad missing or old, or its `shims/`
  never landed under `exthome/appload/`; run `scripts/rm2-doctor.sh` to see
  which, re-run the installer, then Reload in AppLoad.
- **No reply, ink blot pulses forever** — oracle problem: re-run
  `--oracle-test`; check Wi-Fi, key, and that the model supports images.
- **Tablet acting up** — `ssh rm2 'systemctl restart xochitl'` restores the
  stock UI; worst case hold power ~10 s to reboot. riddle in windowed mode
  never stops xochitl, so the blast radius is small.
