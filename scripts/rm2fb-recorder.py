#!/usr/bin/env python3
"""Dev-machine stand-in for the rm2display server.

Binds the rm2fb control socket, mmaps the shared RGB565 framebuffer, and
renders a PNG snapshot of the panel after each burst of updates — so the
takeover backend can be watched (and screenshotted) without a tablet.

usage: rm2fb-recorder.py [shm_path] [sock_path] [out_png]
"""
import mmap
import os
import socket
import struct
import sys
import time
import zlib

W, H = 1404, 1872
FB_BYTES = W * H * 2
SHM = sys.argv[1] if len(sys.argv) > 1 else "/tmp/swtfb.01"
SOCK = sys.argv[2] if len(sys.argv) > 2 else "/tmp/rm2fb.sock"
OUT = sys.argv[3] if len(sys.argv) > 3 else "/tmp/rm2fb-frame.png"
WAVES = {0xF001: "DU", 0xF002: "GC16", 0xF003: "GL16", 0xF004: "A2"}

# Ensure the shm file exists at full size before mapping.
with open(SHM, "a+b") as f:
    f.truncate(FB_BYTES + W * H)
fd = os.open(SHM, os.O_RDONLY)
mem = mmap.mmap(fd, FB_BYTES, prot=mmap.PROT_READ)

try:
    os.unlink(SOCK)
except FileNotFoundError:
    pass
sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
sock.bind(SOCK)
sock.settimeout(0.5)
print(f"recorder: fb={SHM} sock={SOCK} out={OUT}", flush=True)


def png_chunk(tag, data):
    body = tag + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))


def snapshot():
    mem.seek(0)
    raw = mem.read(FB_BYTES)
    rows = bytearray()
    # RGB565 -> 8-bit gray (the panel is grayscale anyway; use green channel).
    for y in range(H):
        rows.append(0)  # filter: none
        row = raw[y * W * 2 : (y + 1) * W * 2]
        g = bytearray(W)
        for x in range(W):
            px = row[2 * x] | (row[2 * x + 1] << 8)
            g[x] = ((px >> 5) & 0x3F) << 2
        rows += g
    png = (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 0, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(bytes(rows), 6))
        + png_chunk(b"IEND", b"")
    )
    tmp = OUT + ".tmp"
    with open(tmp, "wb") as f:
        f.write(png)
    os.replace(tmp, OUT)


updates = 0
dirty = False
last_snap = 0.0
while True:
    try:
        data, _ = sock.recvfrom(64)
        if len(data) >= 32:
            y1, x1, y2, x2, flags, wave = struct.unpack("<6i", data[:24])
            updates += 1
            dirty = True
            name = WAVES.get(wave, hex(wave))
            print(f"update #{updates}: ({x1},{y1})-({x2},{y2}) wave={name}", flush=True)
    except socket.timeout:
        pass
    now = time.monotonic()
    if dirty and now - last_snap > 0.5:
        snapshot()
        last_snap = now
        dirty = False
        print(f"snapshot -> {OUT}", flush=True)
