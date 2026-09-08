#!/usr/bin/env python3
"""Optional maintainer utility: system python3 with PyGObject, Rsvg and Cairo.
These are artwork tools only, not browser/build dependencies.
"""
import gi
gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg
import cairo

icon = Rsvg.Handle.new_from_file("assets/mgbrowser.svg")
for size in (32, 256):
    surface = cairo.ImageSurface(cairo.FORMAT_ARGB32, size, size)
    context = cairo.Context(surface)
    viewport = Rsvg.Rectangle()
    viewport.x = viewport.y = 0
    viewport.width = viewport.height = size
    icon.render_document(context, viewport)
    surface.write_to_png(f"assets/mgbrowser-{size}.png")
