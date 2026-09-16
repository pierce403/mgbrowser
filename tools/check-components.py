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


def reviewed_engine_os_primitives(parents, butane_version, *, stylo):
    """Allow the pinned Boa/Stylo OS substrate, never an implementation backend.

    Check every ancestor edge on these paths, not merely a blanket libc allow.
    The exact Linux graph/source review is in docs/jsplan/BOA_DEPENDENCIES.md.
    """
    libc = {package for package in parents if package[0] == "libc"}
    if not libc:
        return
    assert libc == {("libc", "0.2.189")}, f"Review changed libc version: {libc}"
    allowed_edges = {
        ("libc", "0.2.189"): {
            ("getrandom", "0.4.3"), ("parking_lot_core", "0.9.12"),
            ("time", "0.3.55"),
        },
        ("getrandom", "0.4.3"): {("rand", "0.10.2")},
        ("rand", "0.10.2"): {("boa_engine", "0.22.0")},
        ("parking_lot_core", "0.9.12"): {("dashmap", "6.2.1")},
        ("dashmap", "6.2.1"): {("boa_engine", "0.22.0")},
        ("time", "0.3.55"): {("boa_engine", "0.22.0")},
        ("boa_engine", "0.22.0"): {("mg-butane", butane_version)},
    }
    if stylo:
        # Boa's Trace/JsData derives expand to these names in Sparkle's DOM
        # wrappers. This exact direct dependency grants no new host capability.
        allowed_edges[("boa_engine", "0.22.0")].add(("mg-sparkle", butane_version))
        allowed_edges[("libc", "0.2.189")].add(("num_cpus", "1.17.0"))
        allowed_edges[("parking_lot_core", "0.9.12")].add(("parking_lot", "0.12.5"))
        allowed_edges.update({
            ("num_cpus", "1.17.0"): {("stylo", "0.21.0")},
            ("parking_lot", "0.12.5"): {("stylo", "0.21.0"), ("string_cache", "0.9.0")},
            ("string_cache", "0.9.0"): {
                ("stylo", "0.21.0"), ("stylo_atoms", "0.21.0"),
                ("stylo_malloc_size_of", "0.21.0"), ("to_shmem", "0.5.0"),
                ("web_atoms", "0.2.6"),
            },
        })
    for package, allowed_parents in allowed_edges.items():
        actual = parents.get(package, set())
        assert actual <= allowed_parents, (
            f"Review new OS-primitive path to {package}: {actual - allowed_parents}"
        )

def reviewed_boa_features():
    """Feature changes need review even when they do not add native libraries."""
    tree = subprocess.check_output([
        "cargo", "tree", "--locked", "--package", "mg-butane",
        "--no-default-features", "--features", "modern",
        "--target", "x86_64-unknown-linux-gnu", "--edges", "normal,build",
        "--prefix", "none", "--format", "{p}\t{f}",
    ], text=True)
    expected = {
        ("boa_engine", "0.22.0"): {"fuzz"},
        ("boa_ast", "0.22.0"): {"arbitrary"},
        ("boa_gc", "0.22.0"): {"arrayvec", "boa_string", "thin-vec"},
        ("boa_interner", "0.22.0"): {"arbitrary"},
        ("boa_macros", "0.22.0"): set(),
        ("boa_parser", "0.22.0"): set(),
        ("boa_string", "0.22.0"): set(),
        ("arbitrary", "1.4.2"): {"derive", "derive_arbitrary"},
        ("derive_arbitrary", "1.4.2"): set(),
    }
    observed = {}
    names = {name for name, _ in expected}
    for line in tree.splitlines():
        package, features = line.split("\t", 1)
        name, version = package.split()[:2]
        if name.startswith("boa_") or name in names:
            selected = observed.setdefault((name, version.removeprefix("v")), set())
            selected.update(filter(None, features.removesuffix(" (*)").split(",")))
    assert observed == expected, f"Review changed Boa dependency/features: {observed}"


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

for name in ("mg-butane", "mg-sparkle"):
    assert all(d["name"] != "libc" for d in packages[name]["dependencies"]), (
        f"{name} must not depend directly on libc; only reviewed transitive OS primitives are allowed"
    )

reviewed_boa_features()

# Inspect each production graph separately so root development dependencies and
# feature unification cannot hide a platform dependency in a reusable engine.
for name in ("mg-butane", "mg-sparkle", "mg-chassis"):
    tree = subprocess.check_output([
        "cargo", "tree", "--locked", "--package", name, "--no-default-features",
        *(["--features", "modern"] if name in ("mg-butane", "mg-sparkle") else []),
        "--target", "x86_64-unknown-linux-gnu",
        "--edges", "normal,build", "--prefix", "depth", "--format", "{p}",
    ], text=True)
    graph, parents = production_graph(tree)
    forbidden = {"mg-browser", "x11rb", "x11rb-protocol"}
    if name in ("mg-butane", "mg-sparkle"):
        forbidden |= {"mg-chassis", "libc", "rustls", "tungstenite", "webpki-roots"}
    if name == "mg-butane":
        forbidden |= {"mg-sparkle", "fontdue", "rustybuzz", "image", "url"}
    if name in ("mg-butane", "mg-sparkle"):
        reviewed_engine_os_primitives(parents, packages["mg-butane"]["version"],
                                      stylo=name == "mg-sparkle")
        forbidden.remove("libc")
    assert not graph & forbidden, f"{name} acquired forbidden dependencies: {graph & forbidden}"
    print(f"{name}: production component boundary passed")
