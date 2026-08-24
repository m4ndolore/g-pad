# Vendored Quill core

This directory vendors the minimal display adapter from
[`MaximeRivest/quill` v0.1.0](https://github.com/MaximeRivest/quill/tree/v0.1.0),
commit `39262ee`, under its MIT license. It intentionally excludes demos and
the proprietary `libqsgepaper.so`.

The local change generalizes the Qt external-buffer constructor boundary from
an AArch64-only `qint64` stride to Qt's platform-sized `qsizetype`. The build
therefore exports the AArch64 constructor ABI on the Paper Pro and the ARMv7
constructor ABI on the reMarkable 2. The stable Quill C ABI is unchanged.

Build just the adapter:

```sh
DEVICE=rm2 SDK="$HOME/rm-sdk-rm2" ./quill/build.sh
DEVICE=rmpp SDK="$HOME/rm-sdk-3.26" ./quill/build.sh
```

The build pulls `libqsgepaper.so` from SSH host `rm` for Paper Pro or `rm2`
for reMarkable 2, unless the matching target cache already exists under
`quill/vendor/`. Override the host with `QUILL_DEVICE_HOST`. The caches are
architecture-specific. Never commit or redistribute that library.

The original clean-room provenance statement is retained in `CLEANROOM.md`.
The ARM32 extension is a portability change to that MIT implementation based
on Qt's public platform types and ELF ABI names.
