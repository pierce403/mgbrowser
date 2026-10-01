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
# Existing installed updaters use the transport's 8 MiB response-body ceiling.
# A payload below the separate 64 MiB unpacked ceiling can still be too large
# to download. Fail before publishing either the archive or its checksum.
archive_bytes=$(stat -c %s "$out/mgbrowser-linux-x86_64.tar.gz")
if (( archive_bytes > 8 * 1024 * 1024 )); then
    printf 'Release archive is %s bytes; existing updaters require at most 8388608 bytes.\n' "$archive_bytes" >&2
    exit 1
fi
(cd "$out" && sha256sum mgbrowser-linux-x86_64.tar.gz > mgbrowser-linux-x86_64.tar.gz.sha256)
