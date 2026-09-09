#!/usr/bin/env bash
set -euo pipefail
# Run from the repository root after cargo build --locked --release --bin mgbrowser.
out=${1:-tmp/release}
version=$(target/release/mgbrowser --version)
version=${version#mgbrowser }
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
mkdir -p "$out/mgbrowser-linux-x86_64"
payload="$out/mgbrowser-linux-x86_64"
install -m 755 target/release/mgbrowser "$payload/mgbrowser"
install -m 644 assets/mgbrowser.svg LICENSE NOTICE "$payload/"
install -m 644 "docs/RELEASE-v$version.md" "$payload/README.md"
install -m 644 assets/mgbrowser-256.png "$payload/"
python3 tools/license-inventory.py "$payload/THIRD_PARTY_LICENSES.txt"
tar -czf "$out/mgbrowser-linux-x86_64.tar.gz" -C "$out" mgbrowser-linux-x86_64
(cd "$out" && sha256sum mgbrowser-linux-x86_64.tar.gz > mgbrowser-linux-x86_64.tar.gz.sha256)
