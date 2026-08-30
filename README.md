# Anthink

An e-ink writing pad. Write with the pen. Rule a line beneath your words.
A reply writes itself back in a flowing hand.

**Anthink** is the project. The device is the **Anthink Slate**; the pen
surface this repository began as is **g-pad mode**; agent sessions arrive over
the **Claude bridge**. The binary, its data directory, and the `RIDDLE_*`
environment are still named for their history and are not being churned — the
names above are how the surfaces are described, not a rename of the install
path.

A vision-capable API key is enough to get a reply. gstack and gbrain are
optional: when they are there, the pad is I/O for a real corpus; when they
are not, pages still land on the tablet.

Derived from [riddle](https://github.com/MaximeRivest/riddle) (MIT). See
[NOTICE](NOTICE).

## What this is

```
 pen (raw evdev)
   │ strokes
   ▼
 g-pad ── underline → commit page → PNG ──► oracle (any vision LLM,
   │                                         streams reply sentence-by-sentence)
   ▼ strokes (handwriting face → skeletonized pen paths)
 display
   └── quill — full takeover: xochitl stopped, vendor e-ink engine
```

- **This repository** — the pad (Rust). Pen input, ink surface, handwriting
  synthesis, the oracle process, takeover display.
- **[Quill](https://github.com/MaximeRivest/quill)** — clean-room MIT adapter
  over the vendor `libqsgepaper.so` waveform engine. We vendor the source;
  you extract the proprietary `.so` from a device you own. Never committed
  here (`scripts/ci/check-no-vendor-blob.sh`).

The pad is a capture surface. Categories, retrieval, and answers belong to
the brain behind it. Offline answering is not in this tree: disconnected
means capture-and-queue, not unplugged-and-mute.

## Install on a reMarkable 2 (takeover)

You need a **reMarkable 2** with SSH as root (stock; no developer mode),
xovi + AppLoad, and this bundle.

1. Build:

```sh
rustup target add armv7-unknown-linux-gnueabihf
DEVICE=rm2 ./build-takeover.sh
DEVICE=rm2 ./scripts/make-bundle.sh
```

2. Copy `dist/rm2-takeover/g-pad/` to the tablet:

```sh
scp -O -r dist/rm2-takeover/g-pad root@10.11.99.1:/home/root/xovi/exthome/appload/
```

3. Add a key: `cp oracle.env.example oracle.env` in that folder and set
   `RIDDLE_OPENAI_KEY` (any OpenAI-compatible vision key).

4. In **AppLoad**: Reload, then **g-pad**. Write, then rule a line beneath it.

> This modifies your device. Takeover stops the reMarkable UI and drives the
> e-ink engine directly as root. Leave with a **5-finger tap** — xochitl
> restarts. Keep SSH working: `ssh root@10.11.99.1 'systemctl start xochitl'`.
> Not affiliated with reMarkable AS.

`libqsgepaper.so` is pulled from *your* tablet by `quill/build.sh`. It is
gitignored. Do not add it.

## Gestures

| Do this | And |
|---------|-----|
| Write, then draw a long rule beneath it | The pad reads your ink and replies |
| Flip the marker | Erase |
| Swipe from the left edge | History: current sitting as a thread; back to conversation selector. HISTORY / CORPUS / AGENTS across the top |
| Swipe down from the top | Settings (Stealth) or controls (Guided) |
| Draw a large `?` | Gesture guide |
| Hold five fingers, then release | Leave *(takeover)* |
| Power button | Sleep; press again to wake where you were *(takeover)* |

HISTORY splits sittings after six hours of silence. Long replies page with
swipe up (next) and swipe down (prev).

## Reading

The pad reads as well as it writes. Two reading surfaces, both bounded: they
show what fits and say what they left out.

- **The daily brief** — one page, one day, no navigation and no article bodies.
  See [docs/daily-brief.md](docs/daily-brief.md).
- **The Claude bridge** — agent sessions under the **AGENTS** tab: what is
  running, and the last exchange of each. Reading only; the pad has no keyboard
  and is not growing one. See [docs/claude-bridge.md](docs/claude-bridge.md).

Agent output is split where it matters. Prose is commentary and never ranks as
evidence; a commit sha or a path is a fact another system can check, and is
recorded as an artifact. See [docs/capture-record.md](docs/capture-record.md).

## Memory

Finished pages stay on the tablet under `/home/root/g-pad-data/memories`
(strokes, transcript, reply). Recent pages ride along with the next request.
Writing *"show me what I wrote about…"* restores that page in your own hand.

The catalog the model sees is private. It is not on the page. `RIDDLE_MEMORY=off`
turns this off.

## Oracle

HTTP if `RIDDLE_OPENAI_KEY` is set, otherwise `pi`.

```sh
export RIDDLE_OPENAI_KEY="sk-..."
export RIDDLE_OPENAI_BASE="https://api.openai.com/v1"   # optional
export RIDDLE_OPENAI_MODEL="gpt-4o-mini"                # must see images
```

Put a prompt in `persona.txt` next to the binary (or `RIDDLE_PERSONA_FILE`)
to replace the default pad voice. The default is not a character.

Verify: `g-pad --oracle-test icon.png` on the device with `oracle.env` loaded.

## Build from source (host tests)

```sh
cargo test
```

Takeover build needs the reMarkable SDK sysroot and a device to extract
`libqsgepaper.so`. See [docs/rm2-setup.md](docs/rm2-setup.md) and
[quill/README.md](quill/README.md).

## License

MIT. Retain the copyright notices in LICENSE. Quill has its own MIT LICENSE
under `quill/`.
