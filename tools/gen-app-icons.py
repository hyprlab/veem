#!/usr/bin/env python3
"""Regenerate the app icons from their sources in data/icons/src.

Every gallery entry is a 512² PNG in data/icons/alt/<id>.png, embedded into
the binary by src/app_icon.rs. `default` and `beta` become the hicolor icons
this build installs under its app ID (512 and 256 PNG, plus the SVG itself
as the scalable icon), and `default` also refreshes docs/logo.png.

Sources are either an SVG (rendered with librsvg, the same renderer GNOME
uses, so what ships matches what the desktop would draw) or a PNG master
for icons whose artwork librsvg cannot render (a 1024² export; a stray
trailing column is cropped off).

Requires python3-gobject with the Rsvg typelib, pycairo and ImageMagick.
"""
import pathlib, shutil, subprocess, sys

import cairo, gi
gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "data/icons/src"
ALT = ROOT / "data/icons/alt"
HICOLOR = ROOT / "data/icons/hicolor"
APP_ID = {"default": "co.hyprlab.Vireo", "beta": "co.hyprlab.Vireo.Beta"}


def render_svg(svg: pathlib.Path, png: pathlib.Path, size: int) -> None:
    handle = Rsvg.Handle.new_from_file(str(svg))
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    rect = Rsvg.Rectangle()
    rect.x, rect.y, rect.width, rect.height = 0, 0, size, size
    if not handle.render_document(cairo.Context(surface), rect):
        sys.exit(f"librsvg could not render {svg}")
    surface.write_to_png(str(png))


def resize_png(master: pathlib.Path, png: pathlib.Path, size: int) -> None:
    subprocess.run(
        ["magick", str(master), "-crop", "1024x1024+0+0", "+repage",
         "-filter", "Lanczos", "-resize", f"{size}x{size}", str(png)],
        check=True,
    )


def make(src: pathlib.Path, png: pathlib.Path, size: int) -> None:
    png.parent.mkdir(parents=True, exist_ok=True)
    (render_svg if src.suffix == ".svg" else resize_png)(src, png, size)


for src in sorted(SRC.iterdir()):
    name = src.stem
    if name in APP_ID:
        app_id = APP_ID[name]
        for size in (512, 256):
            make(src, HICOLOR / f"{size}x{size}/apps/{app_id}.png", size)
        if src.suffix == ".svg":
            scalable = HICOLOR / f"scalable/apps/{app_id}.svg"
            scalable.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(src, scalable)
        if name == "default":
            shutil.copyfile(HICOLOR / "512x512/apps/co.hyprlab.Vireo.png", ROOT / "docs/logo.png")
    else:
        make(src, ALT / f"{name}.png", 512)
    print(f"{name:<22} <- {src.name}")
