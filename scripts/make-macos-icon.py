#!/usr/bin/env python3
"""Draws desktop/app/assets/dotlink.icns: the Dotlink icon for macOS (Finder, Dock, the app
switcher). Same drawing as make-windows-icon.py, on Apple's icon grid: the tile takes 824 of
1024 px, centred, so it lines up with other Mac apps' icons. Needs macOS (`iconutil`).

    python3 scripts/make-macos-icon.py
"""

import importlib.util
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

# The 512 tile within the canvas: 824/1024 of it, centred.
SPAN = 512 * 1024 / 824
ORIGIN = (512 - SPAN) / 2
# iconutil's names: icon_<size>x<size>[@2x].png
ENTRIES = [(16, 1), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)]


def render(size):
    """The tile on the Mac grid, as a PNG of size×size."""
    scale = SPAN / size
    # The big sizes have pixels small enough that 2×2 samples are smooth, and 4× as fast.
    ss = icon.SS if size <= 256 else 2
    rows = []
    for py in range(size):
        row = bytearray([0])
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(ss):
                for sx in range(ss):
                    r, g, b, a = icon.colour_at(
                        ORIGIN + (px + (sx + 0.5) / ss) * scale, ORIGIN + (py + (sy + 0.5) / ss) * scale
                    )
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
