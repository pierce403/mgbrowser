#!/usr/bin/env python3
"""Freeze the JSPLAN experiment's selected graph, not the browser release graph.

Run after cargo fetch/build. This records source facts and reviewed license
omissions; it is a regression guard, not an arbitrary-dependency security audit.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
EXPERIMENT = ROOT / "experiments/jsplan"
DOCS = ROOT / "docs/jsplan"
TARGET = "x86_64-unknown-linux-gnu"
BOA_REV = "337a3668a0dc86dd401ea20906e782249a64a228"
UTILITY_REV = "ad8739f5e0b51d20faf7a2cce98afa5c40121438"
BOA = {"boa_ast", "boa_engine", "boa_gc", "boa_interner", "boa_macros",
       "boa_parser", "boa_string"}
UTILITIES = {"small_btree", "tag_ptr"}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cargo(*args):
    return subprocess.check_output(
        ["cargo", *args, "--locked", "--offline", "--manifest-path",
         str(EXPERIMENT / "Cargo.toml")], text=True, cwd=ROOT)


def inventory():
    metadata = json.loads(cargo("metadata", "--format-version", "1",
                                "--filter-platform", TARGET))
    tree = cargo("tree", "--target", TARGET, "--edges", "normal,build",
                 "--prefix", "none", "--format", "{p}\t{f}")
    active = {}
    for line in tree.splitlines():
        package, features = line.split("\t", 1)
        name, version = package.split()[:2]
        feature_set = active.setdefault((name, version.removeprefix("v")), set())
        feature_set.update(f for f in features.removesuffix(" (*)").split(",") if f)
    guard = (ROOT / "tools/check-dependencies.rs").read_text()
    forbidden = set(re.findall(r'"([a-z0-9_-]+)"',
                               guard.split("const FORBIDDEN:", 1)[1].split("];", 1)[0]))
    packages = []
    locked = tomllib.loads((EXPERIMENT / "Cargo.lock").read_text())
    checksums = {(p["name"], p["version"]): p.get("checksum") for p in locked["package"]}
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        key = package["name"], package["version"]
        if key not in active:
            continue
        name, version = key
        if name in forbidden or package.get("links"):
            raise SystemExit(f"Unreviewed native dependency/link: {name}")
        if not package["license"]:
            raise SystemExit(f"Missing license: {name}")
        directory = Path(package["manifest_path"]).parent
        source_files = sorted(p.relative_to(directory).as_posix()
                              for p in directory.rglob("*")
                              if p.is_file() and p.suffix in (".c", ".C", ".cc", ".cpp", ".cxx", ".a", ".so")) if package["source"] else []
        if source_files and package["source"]:
            raise SystemExit(f"New native source/archive requires review: {name}: {source_files}")
        vcs_path = directory / ".cargo_vcs_info.json"
        vcs = json.loads(vcs_path.read_text()) if vcs_path.exists() else None
        if name in BOA | UTILITIES:
            expected = BOA_REV if name in BOA else UTILITY_REV
            expected_version = "0.22.0" if name in BOA else "0.1.0"
            if version != expected_version or vcs["git"]["sha1"] != expected:
                raise SystemExit(f"Unreviewed Boa source revision: {name} {version}")
        license_files = set()
        for pattern in ("LICENSE*", "LICENCE*", "COPYING*", "NOTICE*", "license*", "licenses/*"):
            license_files.update(p for p in directory.glob(pattern) if p.is_file())
        if package.get("license_file"):
            license_files.add(directory / package["license_file"])
        if not license_files:
            if name in BOA | UTILITIES and package["license"] == "Unlicense OR MIT":
                license_files.add(DOCS / "licenses/boa-MIT.txt")
            elif name == "mg-jsplan-probe" and package["license"] == "Apache-2.0":
                license_files.add(ROOT / "LICENSE")
            else:
                raise SystemExit(f"Unreviewed missing license text: {name} {version}")
        licenses = []
        for path in sorted(license_files):
            label = path.relative_to(directory).as_posix() if path.is_relative_to(directory) else path.relative_to(ROOT).as_posix()
            licenses.append({"path": label, "sha256": sha256(path)})
        builds = [{"path": Path(t["src_path"]).relative_to(directory).as_posix(),
                   "sha256": sha256(Path(t["src_path"]))}
                  for t in package["targets"] if "custom-build" in t["kind"]]
        packages.append({"name": name, "version": version,
                         "source": package["source"] or "repository",
                         "archive_sha256": checksums[key],
                         "license": package["license"], "license_files": licenses,
                         "features": sorted(active[key]), "build_scripts": builds,
                         "links": package.get("links"), "native_source_files": source_files,
                         "rust_version": package.get("rust_version"),
                         "vcs_revision": vcs["git"]["sha1"] if vcs else None})
    edges = cargo("tree", "--target", TARGET, "--edges", "normal,build",
                  "--charset", "ascii", "--format", "{p} {f}")
    return {"schema": 1, "scope": "research-only active normal/build graph",
            "target": TARGET, "manifest_sha256": sha256(EXPERIMENT / "Cargo.toml"),
            "lock_sha256": sha256(EXPERIMENT / "Cargo.lock"),
            "normal_build_tree": edges.replace(str(ROOT), "repository").splitlines(),
            "packages": packages}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = inventory()
    rows = ["# JSPLAN research dependency inventory\n\n",
            "Generated by `experiments/jsplan/dependency_audit.py`; see [audit](DEPENDENCY_AUDIT.md).\n",
            "Linux x86_64 active normal/build packages, including build-only Rust tools.\n",
            "This graph does not enter the browser release. Features are the union of selected host/target units.\n\n",
            "| Package | Version | Declared license | Build script |\n| --- | --- | --- | --- |\n"]
    for p in data["packages"]:
        rows.append(f"| {p['name']} | {p['version']} | {p['license']} | {'yes' if p['build_scripts'] else ''} |\n")
    outputs = {DOCS / "DEPENDENCIES.json": json.dumps(data, indent=2) + "\n",
               DOCS / "DEPENDENCY_LICENSES.md": "".join(rows)}
    for path, content in outputs.items():
        if args.check:
            if not path.exists() or path.read_text() != content:
                raise SystemExit(f"Out of date: {path.relative_to(ROOT)}")
        else:
            path.write_text(content)
    print(f"JSPLAN dependency audit: {len(data['packages'])} active packages, "
          f"{sum(bool(p['build_scripts']) for p in data['packages'])} build scripts; "
          "no known prohibited backend or native source archive. Source review remains required.")


if __name__ == "__main__":
    main()
