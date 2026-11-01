#!/usr/bin/env python3
"""Draws desktop/app/assets/dotlink.icns: the Dotlink icon for macOS (Finder, Dock, the app
switcher). The dots of make-windows-icon.py, on Apple's icon grid so it sits with other Mac
apps' icons: the tile takes 824 of 1024 px, centred, in Apple's rounder shape (a superellipse,
not circular corners), with the template's drop shadow below it and a faint light rim, so the
black tile doesn't melt into a dark Dock. Needs macOS (`iconutil`).

    python3 scripts/make-macos-icon.py
"""

import importlib.util
import math
import os
import subprocess
import sys
import tempfile

# Importing the Windows script would leave a __pycache__ in scripts/.
sys.dont_write_bytecode = True

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("windows_icon", os.path.join(HERE, "make-windows-icon.py"))
icon = importlib.util.module_from_spec(spec)
spec.loader.exec_module(icon)

# On the 1024 canvas: the tile's half-width (824 / 2) and centre.
HALF, CENTRE = 412, 512
# Apple's icon shape is close to a superellipse of this exponent.
EXPONENT = 5
# The template's shadow: 12 px down, blurred, half-black.
SHADOW_Y, SHADOW_BLUR, SHADOW_ALPHA = 12, 14, 0.5
# The rim inside the tile's edge: white at this opacity, this many px wide.
RIM_ALPHA, RIM_WIDTH = 0.16, 3


def edge_distance(x, y):
    """About how far (x, y) is outside the tile edge, in px; negative inside."""
    nx, ny = abs(x - CENTRE) / HALF, abs(y - CENTRE) / HALF
    return HALF * ((nx**EXPONENT + ny**EXPONENT) ** (1 / EXPONENT) - 1)


def colour_at(x, y):
    """(r, g, b, a) of one sample on the 1024 canvas."""
    d = edge_distance(x, y)
    if d > 0:
        # Outside: the shadow of the tile moved down.
        ds = max(edge_distance(x, y - SHADOW_Y), 0)
        return (0, 0, 0, round(255 * SHADOW_ALPHA * math.exp(-(ds * ds) / (2 * SHADOW_BLUR**2))))
    # The dots, drawn in make-windows-icon.py's 512 grid.
    to_grid = 512 / (2 * HALF)
    r, g, b, _ = icon.colour_at(256 + (x - CENTRE) * to_grid, 256 + (y - CENTRE) * to_grid, tile=False)
    if (r, g, b) != (0, 0, 0):
        return (r, g, b, 255)
    if d > -RIM_WIDTH:
        grey = round(255 * RIM_ALPHA)
        return (grey, grey, grey, 255)
    return (0, 0, 0, 255)
# iconutil's names: icon_<size>x<size>[@2x].png
ENTRIES = [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)]


def render(size):
    """The tile on the Mac grid, as a PNG of size×size."""
    scale = 1024 / size
    # The big sizes have pixels small enough that 2×2 samples are smooth, and 4× as fast.
    ss = icon.SS if size <= 256 else 2
    rows = []
    for py in range(size):
        row = bytearray([0])
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(ss):
                for sx in range(ss):
                    r, g, b, a = colour_at((px + (sx + 0.5) / ss) * scale, (py + (sy + 0.5) / ss) * scale)
                    acc[0] += r * a
                    acc[1] += g * a
                    acc[2] += b * a
                    acc[3] += a
            if acc[3]:
                n = ss * ss
                row += bytes((round(acc[0] / acc[3]), round(acc[1] / acc[3]), round(acc[2] / acc[3]), round(acc[3] / n)))
            else:
                row += b"\0\0\0\0"
        rows.append(bytes(row))
    return icon.png(size, b"".join(rows))


def main():
    rendered = {}
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "dotlink.iconset")
        os.mkdir(iconset)
        for size, scale in ENTRIES:
            px = size * scale
            if px not in rendered:
                rendered[px] = render(px)
            name = f"icon_{size}x{size}{'@2x' if scale == 2 else ''}.png"
            with open(os.path.join(iconset, name), "wb") as f:
                f.write(rendered[px])
        out = os.path.normpath(os.path.join(HERE, "..", "desktop", "app", "assets", "dotlink.icns"))
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", out], check=True)
    print(f"wrote {out} ({os.path.getsize(out)} bytes)")


if __name__ == "__main__":
    main()
