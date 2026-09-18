#!/usr/bin/env bash
# Native acceptance of the supplied bytes on an owned display and ephemeral
# fixture server. Never uses the user's preferences, bookmarks or session bus.
set -euo pipefail
payload=$(realpath "${1:-target/release/mgbrowser}")
mkdir -p tmp
scratch=$(mktemp -d "$PWD/tmp/inspector-smoke.XXXXXX")
install -m 755 "$payload" "$scratch/mgbrowser"
sha256sum "$scratch/mgbrowser"
timeout 140s env MGBROWSER_INSPECTOR_PRIVATE_DISPLAY=1 \
    xvfb-run -a -s '-screen 0 3840x2160x24 -nolisten tcp' \
    target/debug/examples/inspector_smoke "$scratch/mgbrowser" "$scratch"
printf 'INSPECTOR_SMOKE_OK %s\n' "$scratch"
