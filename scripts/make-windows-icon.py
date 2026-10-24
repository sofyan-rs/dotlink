#!/usr/bin/env python3
"""Draws desktop/app/assets/dotlink.ico: the Dotlink icon for Windows (exe, taskbar, Alt-Tab), and
desktop/app/assets/notification-dark.png: the dots alone, for notifications in dark mode (Windows
shows a dark icon there inverted: a white tile, black dots and a cyan one).

Windows icons fill their square, unlike the GNOME-style PNGs (a 416 px tile with a 48 px margin
in 512), which looked small on the taskbar. Same drawing as assets/dotlink.svg, tile edge to
edge. Plain Python (no Pillow): shapes rendered with 4x4 supersampling, PNG-compressed entries.

    python scripts/make-windows-icon.py
"""

import os
import struct
import zlib

SIZES = [16, 24, 32, 48, 64, 128, 256]
RADIUS = 112  # tile corner radius in the 512 grid
DOT_R = 21
WHITE = (255, 255, 255)
RED = (0xE5, 0x21, 0x1C)
# The "d" glyph: (column, row) on a 56 px grid starting at 144, the red one at (2, 2).
DOTS = [(0, 0), (1, 0), (2, 0), (0, 1), (2, 1), (0, 2), (1, 2), (2, 2), (3, 2), (4, 2),
        (2, 3), (4, 3), (2, 4), (3, 4), (4, 4)]
CENTRES = [(144 + 56 * c, 144 + 56 * r, RED if (c, r) == (2, 2) else WHITE) for c, r in DOTS]
SS = 4  # supersamples per axis


def in_tile(x, y):
    """Inside the rounded 512x512 square."""
    cx = min(max(x, RADIUS), 512 - RADIUS)
    cy = min(max(y, RADIUS), 512 - RADIUS)
    return (x - cx) ** 2 + (y - cy) ** 2 <= RADIUS ** 2


# The dots alone: the glyph's square (123..389) with a margin, on a transparent background.
GLYPH_ORIGIN, GLYPH_SPAN = 107, 298


def colour_at(x, y, tile=True):
    """(r, g, b, a) of one sample in the 512 grid."""
    if tile and not in_tile(x, y):
        return (0, 0, 0, 0)
    for cx, cy, col in CENTRES:
        if (x - cx) ** 2 + (y - cy) ** 2 <= DOT_R ** 2:
            return (*col, 255)
    return (0, 0, 0, 255 if tile else 0)


def render(size, tile=True):
    origin, span = (0, 512) if tile else (GLYPH_ORIGIN, GLYPH_SPAN)
    scale = span / size
    rows = []
    for py in range(size):
        row = bytearray([0])  # PNG filter: none
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(SS):
                for sx in range(SS):
                    r, g, b, a = colour_at(
                        origin + (px + (sx + 0.5) / SS) * scale, origin + (py + (sy + 0.5) / SS) * scale, tile
                    )
                    acc[0] += r * a
                    acc[1] += g * a
                    acc[2] += b * a
                    acc[3] += a
            n = SS * SS
            alpha = acc[3] / n
            if acc[3]:
                row += bytes((round(acc[0] / acc[3]), round(acc[1] / acc[3]), round(acc[2] / acc[3]), round(alpha)))
            else:
                row += b"\0\0\0\0"
        rows.append(bytes(row))
    return png(size, b"".join(rows))


def png(size, raw):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def main():
    images = [(s, render(s)) for s in SIZES]
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for s, data in images:
        out += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    out += b"".join(data for _, data in images)
    assets = os.path.join(os.path.dirname(__file__), "..", "desktop", "app", "assets")
    for name, data in [("dotlink.ico", out), ("notification-dark.png", render(64, tile=False))]:
        path = os.path.join(assets, name)
        with open(path, "wb") as f:
            f.write(data)
        print(f"wrote {os.path.normpath(path)} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
