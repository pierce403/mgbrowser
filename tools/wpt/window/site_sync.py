#!/usr/bin/env python3
"""Synchronize only the Window score region from fixed tracked metadata."""
import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import stat
import sys
import tempfile


ROOT = Path(__file__).absolute().parents[3]
SCORE = ROOT / "tools/wpt/window/current-score.json"
INDEX = ROOT / "index.html"
LIMIT = 4 * 1024 * 1024
SPEC = importlib.util.spec_from_file_location("window_site", Path(__file__).with_name("site.py"))
renderer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(renderer)


def _stamp(value):
    return (value.st_dev, value.st_ino, value.st_mode, value.st_size,
            value.st_mtime_ns, value.st_ctime_ns)


def _read_regular(path, limit=LIMIT):
    path = Path(path).absolute()
    if path.resolve(strict=True) != path:
        raise ValueError("Website score input must not use symlinks or aliases")
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode) or not 0 <= before.st_size <= limit:
        raise ValueError("Website score input is not a bounded regular file")
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        opened = os.fstat(descriptor)
        if _stamp(opened) != _stamp(before):
            raise ValueError("Website score input changed before opening")
        chunks, received = [], 0
        while received <= before.st_size:
            chunk = os.read(descriptor, min(65536, before.st_size + 1 - received))
            if not chunk:
                break
            chunks.append(chunk)
            received += len(chunk)
        if received != before.st_size or _stamp(os.fstat(descriptor)) != _stamp(opened):
            raise ValueError("Website score input changed during reading")
        _unchanged(path, _stamp(opened))
        return b"".join(chunks), _stamp(opened)
    finally:
        os.close(descriptor)


def _unchanged(path, expected):
    if _stamp(Path(path).lstat()) != expected:
        raise ValueError("Website score input changed before publication")


def _decode(raw):
    def pairs(rows):
        value = {}
        for name, item in rows:
            if name in value:
                raise ValueError("Duplicate Window score field")
            value[name] = item
        return value

    def nonfinite(_):
        raise ValueError("Nonfinite Window score number")

    def finite_float(text):
        value = float(text)
        if not math.isfinite(value):
            nonfinite(text)
        return value

    return json.loads(raw.decode("utf-8"), object_pairs_hook=pairs,
                      parse_constant=nonfinite, parse_float=finite_float)


def synchronize(score_path=SCORE, html_path=INDEX, *, check=False):
    """Explicit paths are a unit-test seam; the CLI accepts only --check."""
    score_path, html_path = Path(score_path).absolute(), Path(html_path).absolute()
    score_raw, score_stamp = _read_regular(score_path)
    original, html_stamp = _read_regular(html_path)
    generated = renderer.render(_decode(score_raw))
    updated = renderer.replace_region(original.decode("utf-8"), generated).encode("utf-8")
    if len(updated) > LIMIT:
        raise ValueError("Updated Window website region exceeds its byte limit")
    _unchanged(score_path, score_stamp)
    _unchanged(html_path, html_stamp)
    if updated == original:
        return False
    if check:
        raise ValueError("Website Window score is stale; run tmp/site and commit index.html")
    descriptor, name = tempfile.mkstemp(prefix=".wpt-window-score-", dir=html_path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as destination:
            os.fchmod(destination.fileno(), stat.S_IMODE(html_stamp[2]))
            destination.write(updated)
        _unchanged(score_path, score_stamp)
        _unchanged(html_path, html_stamp)
        os.replace(temporary, html_path)
    finally:
        temporary.unlink(missing_ok=True)
    return True


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    try:
        synchronize(SCORE, INDEX, check=args.check)
    except (OSError, ValueError, RecursionError) as error:
        print(f"Window website score: {error}", file=sys.stderr)
        return 1
    print("Window website score is current." if args.check else "Window website score synchronized.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
