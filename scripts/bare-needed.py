#!/usr/bin/env python3
"""Rewrite a DT_NEEDED entry from a build-host path to a bare filename.

libquill.so is built without a SONAME, so a linker records whatever path it
was handed. The SDK's gcc happens to hand it a bare name; ld.lld records the
full path, which cannot exist on the tablet — the binary then dies at startup
with "cannot open shared object file".

.dynstr entries are NUL-terminated, so a shorter name can overwrite a longer
one in place provided the tail is zero-filled: the string offset stays valid
and no other entry moves. That keeps this a byte patch rather than an ELF
rewrite, and it leaves the runtime rpath to do the actual resolving.

    scripts/bare-needed.py <binary> <path-to-shorten>
"""
import sys


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    binary, target = sys.argv[1], sys.argv[2].encode()
    bare = target.rsplit(b"/", 1)[-1]
    if bare == target:
        print(f"already bare: {target.decode()}")
        return 0

    with open(binary, "rb") as fh:
        data = bytearray(fh.read())

    needle = target + b"\x00"
    start = data.find(needle)
    if start == -1:
        print(f"not found in {binary}: {target.decode()}", file=sys.stderr)
        return 1

    data[start : start + len(needle)] = bare + b"\x00" * (len(needle) - len(bare))
    with open(binary, "wb") as fh:
        fh.write(data)

    print(f"patched DT_NEEDED: {target.decode()} -> {bare.decode()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
