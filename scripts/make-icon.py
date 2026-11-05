#!/usr/bin/env python3
"""Renders the app icon (a heart) into assets/icon.png (1024 px) and assets/icon-512.png.

Same implicit curve as `heart_rgba` in src/tray.rs: (x² + y² − 1)³ − x²y³ ≤ 0,
4×4 supersampled. Standard library only. Run from the repository root.
"""
import struct
import zlib

RGB = (0xFF, 0x4D, 0x5E)
FILL = 0.88  # share of the canvas width the heart spans
HALF_WIDTH, Y_CENTER = 1.14, 0.115
SS = 4


def render(size: int) -> bytes:
    k = 2 * HALF_WIDTH / FILL
    rows = []
    for py in range(size):
        row = bytearray([0])  # PNG filter type: none
        for px in range(size):
            hits = 0
            for sy in range(SS):
                y = Y_CENTER + (0.5 - (py + (sy + 0.5) / SS) / size) * k
                for sx in range(SS):
                    x = ((px + (sx + 0.5) / SS) / size - 0.5) * k
                    a = x * x + y * y - 1
                    if a * a * a - x * x * y * y * y <= 0:
                        hits += 1
            row += bytes((*RGB, hits * 255 // (SS * SS)))
        rows.append(bytes(row))
    return png(size, b"".join(rows))


def png(size: int, raw: bytes) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(kind + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", crc)

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


if __name__ == "__main__":
    for size, path in [(1024, "assets/icon.png"), (512, "assets/icon-512.png")]:
        with open(path, "wb") as f:
            f.write(render(size))
        print(path)
