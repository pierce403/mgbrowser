#!/usr/bin/env bash
# Test the supplied binary through native X11 input on an owned 4K display.
set -euo pipefail
payload=$(realpath "${1:-target/release/mgbrowser}")
mkdir -p tmp
scratch=$(mktemp -d "$PWD/tmp/scale-smoke.XXXXXX")
install -m 755 "$payload" "$scratch/mgbrowser"
sha256sum "$scratch/mgbrowser"
target/debug/examples/journey_server > "$scratch/server.log" 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true; wait "$server" 2>/dev/null || true' EXIT
for attempt in $(seq 1 50); do
    kill -0 "$server"
    if grep -q '^Local fixture service:' "$scratch/server.log" && \
        curl -fsS http://127.0.0.1:7878/ >/dev/null; then break; fi
    sleep 0.1
done
kill -0 "$server"
grep -q '^Local fixture service:' "$scratch/server.log"
# Xresources/XSETTINGS writes affect only this private display. The browser uses
# temporary XDG roots and a nonexistent private bus address, never real settings.
timeout 140s env MGBROWSER_SCALE_PRIVATE_DISPLAY=1 \
    xvfb-run -a -s '-screen 0 3840x2160x24 -nolisten tcp' \
    target/debug/examples/scale_smoke "$scratch/mgbrowser" "$scratch"
printf 'SCALE_SMOKE_OK %s\n' "$scratch"
