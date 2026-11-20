#!/usr/bin/env python3
"""Static sharing-card checks. Standard library only; no browser or network."""
import json
from html.parser import HTMLParser
from pathlib import Path
import struct
import unittest
import xml.etree.ElementTree as ET
import zlib

ROOT = Path(__file__).resolve().parents[1]


class Head(HTMLParser):
    def __init__(self, html):
        super().__init__()
        self.meta, self.canonical, self.inside = {}, [], False
        self.feed(html)

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if tag == "head":
            self.inside = True
        if not self.inside:
            return
        if tag == "meta":
            key = attrs.get("property", attrs.get("name"))
            self.meta.setdefault(key, []).append(attrs.get("content"))
        if tag == "link" and attrs.get("rel") == "canonical":
            self.canonical.append(attrs.get("href"))

    def handle_endtag(self, tag):
        if tag == "head":
            self.inside = False


class SocialCardTests(unittest.TestCase):
    def test_static_head_has_complete_unique_metadata_and_absolute_image(self):
        head = Head((ROOT / "index.html").read_text())
        expected = {
            "og:type": "website", "og:site_name": "mgbrowser",
            "og:url": "https://mgbrowser.org/",
            "og:title": "mgbrowser: a browser written in Rust",
            "og:image": "https://mgbrowser.org/assets/og-preview.png",
            "og:image:type": "image/png", "og:image:width": "1200",
            "og:image:height": "630", "twitter:card": "summary_large_image",
        }
        for key, value in expected.items():
            with self.subTest(property=key):
                self.assertEqual(head.meta.get(key), [value])
        self.assertEqual(head.canonical, [expected["og:url"]])
        for field in ("title", "description", "image", "image:alt"):
            value = head.meta["og:" + field]
            self.assertEqual(len(value), 1)
            self.assertTrue(value[0])
            self.assertEqual(head.meta.get("twitter:" + field), value)
        self.assertIn("experimental", head.meta["og:description"][0])
        self.assertIn("compatibility is poor", head.meta["og:description"][0])

    def test_committed_png_dimensions_and_complete_chunk_integrity(self):
        image = (ROOT / "assets/og-preview.png").read_bytes()
        self.assertLess(len(image), 1024 * 1024)
        self.assertEqual(image[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(image[12:16], b"IHDR")
        self.assertEqual(struct.unpack(">II", image[16:24]), (1200, 630))
        offset, chunks = 8, []
        while offset < len(image):
            size = struct.unpack(">I", image[offset:offset + 4])[0]
            end = offset + 12 + size
            self.assertLessEqual(end, len(image))
            chunk = image[offset + 4:offset + 8 + size]
            crc = struct.unpack(">I", image[end - 4:end])[0]
            self.assertEqual(zlib.crc32(chunk), crc)
            chunks.append(chunk[:4])
            offset = end
        self.assertIn(b"IDAT", chunks)
        self.assertEqual(chunks[-1], b"IEND")
        self.assertEqual(offset, len(image))

    def test_vector_source_is_self_contained_and_preserves_identity(self):
        svg = ET.parse(ROOT / "assets/og-preview.svg").getroot()
        self.assertEqual((svg.get("width"), svg.get("height")), ("1200", "630"))
        texts = " ".join(svg.itertext())
        for identity in ("mgbrowser", "EXPERIMENTAL PREVIEW", "12", "Mg", "24.305"):
            self.assertIn(identity, texts)
        for element in svg.iter():
            self.assertNotIn(element.tag.rsplit("}", 1)[-1], ("image", "script", "foreignObject"))
            self.assertFalse(any(key.rsplit("}", 1)[-1] == "href" for key in element.attrib))

    def test_hosting_requires_both_preview_assets(self):
        config = json.loads((ROOT / "cloudflare/site-config.json").read_text())
        for path in ("assets/og-preview.png", "assets/og-preview.svg"):
            self.assertIn(path, config["requiredFiles"])
            self.assertTrue((ROOT / path).is_file())


if __name__ == "__main__":
    unittest.main()
