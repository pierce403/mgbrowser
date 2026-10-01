#!/usr/bin/env bash
# Test the supplied executable with independent X11 events on an owned display.
# The Rust helper serves ephemeral local fixtures and creates private HOME/XDG roots.
set -euo pipefail
payload=$(realpath "${1:-target/release/mgbrowser}")
if [[ ! -x target/debug/examples/tabs_smoke ]]; then
    printf 'Build the helper first: cargo +1.91.1 build --locked --example tabs_smoke\n' >&2
    exit 1
fi
mkdir -p tmp
scratch=$(mktemp -d "$PWD/tmp/tabs-smoke.XXXXXX")
install -m 755 "$payload" "$scratch/mgbrowser"
sha256sum "$scratch/mgbrowser"
timeout 240s env MGBROWSER_TABS_PRIVATE_DISPLAY=1 \
    xvfb-run -a -s '-screen 0 3840x2160x24 -nolisten tcp -noreset' \
    target/debug/examples/tabs_smoke "$scratch/mgbrowser" "$scratch"
printf 'TABS_SMOKE_OK %s\n' "$scratch"
