#!/usr/bin/env bash
set -euo pipefail
# Run from the repository root after cargo build --locked --release --bin mgbrowser.
out=${1:-tmp/release}
mkdir -p "$out/mgbrowser-linux-x86_64"
payload="$out/mgbrowser-linux-x86_64"
install -m 755 target/release/mgbrowser "$payload/mgbrowser"
install -m 644 assets/mgbrowser.svg LICENSE docs/RELEASE-v0.1.0.md "$payload/"
install -m 644 assets/mgbrowser-256.png "$payload/"
mv "$payload/RELEASE-v0.1.0.md" "$payload/README.md"
python3 tools/license-inventory.py "$payload/THIRD_PARTY_LICENSES.txt"
tar -czf "$out/mgbrowser-linux-x86_64.tar.gz" -C "$out" mgbrowser-linux-x86_64
(cd "$out" && sha256sum mgbrowser-linux-x86_64.tar.gz > mgbrowser-linux-x86_64.tar.gz.sha256)
