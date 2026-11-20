#!/usr/bin/env python3
"""Optional artwork utility, using the same system Rsvg/Cairo tools as render-icon.

Run from the repository root after editing assets/og-preview.svg. The resulting
PNG is committed: no renderer, fonts or native tools are required by the site,
browser, installer or CI. No external fonts or images are downloaded.
"""
from pathlib import Path

import gi
gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg
import cairo

root = Path(__file__).resolve().parents[1]
width, height = 1200, 630
source = Rsvg.Handle.new_from_file(str(root / "assets/og-preview.svg"))
surface = cairo.ImageSurface(cairo.FORMAT_RGB24, width, height)
context = cairo.Context(surface)
viewport = Rsvg.Rectangle()
viewport.x = viewport.y = 0
viewport.width, viewport.height = width, height
if not source.render_document(context, viewport):
    raise SystemExit("Social preview SVG could not be rendered")
surface.write_to_png(str(root / "assets/og-preview.png"))
print("Rendered assets/og-preview.png (1200 x 630)")
