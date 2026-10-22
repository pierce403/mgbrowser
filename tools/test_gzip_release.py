#!/usr/bin/env python3
"""Packaging compression controls; no browser, build or package execution."""
import gzip
import importlib.util
import io
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch


sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("release_gzip", Path(__file__).with_name("gzip-release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class Chunked(io.BytesIO):
    def __init__(self, body, chunk):
        super().__init__(body)
        self.chunk = chunk

    def read(self, maximum=-1):
        return super().read(min(maximum, self.chunk))


class Controls(unittest.TestCase):
    def compress(self, body, **options):
        destination = io.BytesIO()
        release.compress_stream(io.BytesIO(body), destination, **options)
        return destination.getvalue()

    def test_empty_input_is_a_real_gzip_member(self):
        compressed = self.compress(b"")
        self.assertEqual(gzip.decompress(compressed), b"")
        self.assertTrue(compressed.startswith(b"\x1f\x8b\x08"))

    def test_binary_input_roundtrips_every_byte_across_multiple_chunks(self):
        body = bytes(range(256)) * 600 + b"\0owned tar suffix\xff"
        self.assertEqual(gzip.decompress(self.compress(body)), body)

    def test_repeated_compression_is_deterministic(self):
        body = b"owned release tar bytes\0" * 5000
        self.assertEqual(self.compress(body), self.compress(body))

    def test_header_has_zero_timestamp_and_no_filename(self):
        header = self.compress(b"owned")[:10]
        self.assertEqual(header[:4], b"\x1f\x8b\x08\x00")
        self.assertEqual(header[4:8], b"\0\0\0\0")
        self.assertEqual(header[8], 2)  # Fixed level 9, maximum-compression XFL.

    def test_input_chunk_boundaries_do_not_change_gzip_bytes(self):
        body = bytes(range(256)) * 600
        expected = self.compress(body)
        for chunk in (1, 7, 65536):
            destination = io.BytesIO()
            release.compress_stream(Chunked(body, chunk), destination)
            self.assertEqual(destination.getvalue(), expected)

    def test_exact_input_bound_accepts_and_one_extra_byte_rejects(self):
        self.assertEqual(gzip.decompress(self.compress(b"abc", max_input_bytes=3)), b"abc")
        self.assertEqual(gzip.decompress(self.compress(b"", max_input_bytes=0)), b"")
        for body, limit in ((b"abcd", 3), (b"x", 0)):
            with self.assertRaisesRegex(ValueError, "tar input exceeds"):
                self.compress(body, max_input_bytes=limit)

    def test_bound_failure_has_friendly_stderr_and_nonzero_status(self):
        error = io.StringIO()
        with patch.object(release, "compress_stream", side_effect=ValueError("owned input bound")), \
                patch.object(release.sys, "stderr", error), \
                patch.object(release.sys, "stdout", SimpleNamespace(buffer=io.BytesIO(), close=lambda: None)):
            self.assertEqual(release.main(), 1)
        self.assertEqual(error.getvalue(), "gzip-release: owned input bound\n")


if __name__ == "__main__":
    unittest.main()
