#!/usr/bin/env python3
"""Export Hrungnir's approved transparent logo to Tauri and macOS icon sizes.

Requires ImageMagick (`magick`). The supplied artwork is scaled onto transparent
canvases without recoloring or adding a background. Generated files are committed,
so application builds do not need ImageMagick.
"""
from pathlib import Path
import struct
import subprocess
import tempfile

APP = Path(__file__).resolve().parents[1]


def magick(*args):
    subprocess.run(["magick", *map(str, args)], check=True)


def main():
    source = APP / "assets/hrungnir-logo.png"
    icons = APP / "src-tauri/icons"
    icons.mkdir(parents=True, exist_ok=True)
    opaque = subprocess.check_output(
        ["magick", "identify", "-format", "%[opaque]", source], text=True
    ).strip().lower()
    if opaque == "true":
        raise SystemExit("logo source must contain transparency; refusing to export a nested black square")
    with tempfile.TemporaryDirectory() as directory:
        tmp = Path(directory)
        # Preserve the complete transparent figure. Fit it to a square with room
        # around the silhouette so none of the long staff or hem is clipped.
        magick(source, "-resize", "900x900", "-gravity", "center", "-background",
               "none", "-extent", "1024x1024", "PNG32:" + str(icons / "icon.png"))
        magick(source, "-resize", "512x512", "PNG32:" + str(APP / "reader/hrungnir.png"))

        chunks = []
        for size, kind in [
            (16, b"icp4"),
            (32, b"icp5"),
            (64, b"icp6"),
            (128, b"ic07"),
            (256, b"ic08"),
            (512, b"ic09"),
            (1024, b"ic10"),
        ]:
            path = tmp / f"{size}.png"
            magick(icons / "icon.png", "-resize", f"{size}x{size}", "PNG32:" + str(path))
            png = path.read_bytes()
            chunks.append(kind + struct.pack(">I", len(png) + 8) + png)

        payload = b"".join(chunks)
        (icons / "icon.icns").write_bytes(b"icns" + struct.pack(">I", len(payload) + 8) + payload)
        for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png")]:
            magick(icons / "icon.png", "-resize", f"{size}x{size}", "PNG32:" + str(icons / name))
        magick(icons / "icon.png", "-define", "icon:auto-resize=256,128,64,48,32,16",
               icons / "icon.ico")


if __name__ == "__main__":
    main()
