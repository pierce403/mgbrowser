#!/usr/bin/env python3
"""Check the production dependency direction of the four reusable components."""
import json
import subprocess

metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--no-deps", "--format-version", "1",
]))
allowed = {
    "mg-butane": set(),
    "mg-sparkle": {"mg-butane"},
    "mg-chassis": {"mg-sparkle"},
    "mg-browser": {"mg-butane", "mg-sparkle", "mg-chassis"},
}
packages = {p["name"]: p for p in metadata["packages"]}
assert set(packages) == set(allowed), "Update the explicit component contract for new packages"
for name, edges in allowed.items():
    actual = {d["name"] for d in packages[name]["dependencies"]
              if d["kind"] != "dev" and d["name"] in allowed}
    assert actual == edges, f"{name}: expected {edges}, got {actual}"

# Inspect each production graph separately so root development dependencies and
# feature unification cannot hide a platform dependency in a reusable engine.
for name in ("mg-butane", "mg-sparkle", "mg-chassis"):
    tree = subprocess.check_output([
        "cargo", "tree", "--locked", "--package", name, "--no-default-features",
        "--edges", "normal,build", "--prefix", "none", "--format", "{p}",
    ], text=True)
    graph = {line.split()[0] for line in tree.splitlines() if line.strip()}
    forbidden = {"mg-browser", "x11rb", "x11rb-protocol"}
    if name in ("mg-butane", "mg-sparkle"):
        forbidden |= {"mg-chassis", "libc", "rustls", "tungstenite", "webpki-roots"}
    if name == "mg-butane":
        forbidden |= {"mg-sparkle", "fontdue", "rustybuzz", "image", "url"}
    assert not graph & forbidden, f"{name} acquired forbidden dependencies: {graph & forbidden}"
    print(f"{name}: production component boundary passed")
