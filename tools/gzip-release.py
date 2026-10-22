#!/usr/bin/env python3
"""Stream release tar bytes to reproducible gzip without changing the payload."""
import sys
import zlib


CHUNK_BYTES = 64 * 1024
MAX_INPUT_BYTES = 64 * 1024 * 1024


def compress_stream(source, destination, *, max_input_bytes=MAX_INPUT_BYTES):
    if type(max_input_bytes) is not int or max_input_bytes < 0:
        raise ValueError("input bound must be a nonnegative integer")
    compressor = zlib.compressobj(9, zlib.DEFLATED, 31, 8, zlib.Z_DEFAULT_STRATEGY)
    received = 0
    while True:
        chunk = source.read(min(CHUNK_BYTES, max_input_bytes - received + 1))
        if not chunk:
            break
        received += len(chunk)
        if received > max_input_bytes:
            raise ValueError(f"tar input exceeds {max_input_bytes} bytes")
        destination.write(compressor.compress(chunk))
    destination.write(compressor.flush())
    destination.flush()


def main():
    try:
        compress_stream(sys.stdin.buffer, sys.stdout.buffer)
    except (OSError, ValueError, zlib.error) as error:
        print(f"gzip-release: {error}", file=sys.stderr)
        try:
            sys.stdout.close()
        except OSError:
            pass
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
