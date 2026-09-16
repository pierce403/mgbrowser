#!/usr/bin/env python3
"""Check the production dependency direction of the four reusable components."""
import json
import re
import subprocess


def production_graph(tree):
    """Keep versioned direct edges, including repeated Cargo (*) leaf entries."""
    graph, parents, stack = set(), {}, []
    for line in tree.splitlines():
        if not line.strip():
            continue
        match = re.match(r"^(\d+)(\S+) v(\S+)", line)
        assert match, f"Unrecognized cargo tree entry: {line}"
        depth, name, version = int(match[1]), match[2], match[3]
        package = (name, version)
        assert depth <= len(stack), f"Invalid cargo tree depth: {line}"
        stack = stack[:depth]
        graph.add(name)
        if stack:
            parents.setdefault(package, set()).add(stack[-1])
        stack.append(package)
    return graph, parents


def reviewed_sparkle_os_primitives(parents):
    """Allow only pinned Stylo CPU-count/futex OS declarations, not a backend."""
    libc = {package for package in parents if package[0] == "libc"}
    if not libc:
        return
    assert libc == {("libc", "0.2.189")}, f"Review changed libc version: {libc}"
    allowed_edges = {
        ("libc", "0.2.189"): {("num_cpus", "1.17.0"), ("parking_lot_core", "0.9.12")},
        ("num_cpus", "1.17.0"): {("stylo", "0.21.0")},
        ("parking_lot_core", "0.9.12"): {("parking_lot", "0.12.5")},
        ("parking_lot", "0.12.5"): {("stylo", "0.21.0"), ("string_cache", "0.9.0")},
        ("string_cache", "0.9.0"): {
            ("stylo", "0.21.0"), ("stylo_atoms", "0.21.0"),
            ("stylo_malloc_size_of", "0.21.0"), ("to_shmem", "0.5.0"),
            ("web_atoms", "0.2.6"),
        },
    }
    for package, allowed_parents in allowed_edges.items():
        actual = parents.get(package, set())
        assert actual <= allowed_parents, (
            f"Review new OS-primitive path to {package}: {actual - allowed_parents}"
        )

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

assert all(d["name"] != "libc" for d in packages["mg-sparkle"]["dependencies"]), (
    "mg-sparkle must not depend directly on libc; only reviewed transitive OS primitives are allowed"
)

# Inspect each production graph separately so root development dependencies and
# feature unification cannot hide a platform dependency in a reusable engine.
for name in ("mg-butane", "mg-sparkle", "mg-chassis"):
    tree = subprocess.check_output([
        "cargo", "tree", "--locked", "--package", name, "--no-default-features",
        "--edges", "normal,build", "--prefix", "depth", "--format", "{p}",
    ], text=True)
    graph, parents = production_graph(tree)
    forbidden = {"mg-browser", "x11rb", "x11rb-protocol"}
    if name in ("mg-butane", "mg-sparkle"):
        forbidden |= {"mg-chassis", "libc", "rustls", "tungstenite", "webpki-roots"}
    if name == "mg-butane":
        forbidden |= {"mg-sparkle", "fontdue", "rustybuzz", "image", "url"}
    if name == "mg-sparkle":
        reviewed_sparkle_os_primitives(parents)
        forbidden.remove("libc")
    assert not graph & forbidden, f"{name} acquired forbidden dependencies: {graph & forbidden}"
    print(f"{name}: production component boundary passed")
