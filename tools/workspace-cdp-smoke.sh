#!/usr/bin/env bash
# Independent native tab operations and public CDP against supplied release bytes.
set -euo pipefail
payload=$(realpath "${1:-target/release/mgbrowser}")
if [[ ! -x target/debug/examples/workspace_cdp_smoke ]]; then
    printf 'Build helper: cargo +1.91.1 build --locked --example workspace_cdp_smoke\n' >&2
    exit 1
fi
mkdir -p tmp
scratch=$(mktemp -d "$PWD/tmp/workspace-cdp-smoke.XXXXXX")
install -m 755 "$payload" "$scratch/mgbrowser"
sha256sum "$scratch/mgbrowser"
timeout 100s env MGBROWSER_WORKSPACE_CDP_PRIVATE_DISPLAY=1 \
    xvfb-run -a -s '-screen 0 1920x1080x24 -nolisten tcp' \
    target/debug/examples/workspace_cdp_smoke "$scratch/mgbrowser" "$scratch"
printf 'WORKSPACE_CDP_SMOKE_OK %s\n' "$scratch"
