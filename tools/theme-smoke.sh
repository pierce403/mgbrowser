#!/usr/bin/env bash
# Test the supplied browser executable, with owned display/bus/config and fixture.
# Requires prebuilt examples/theme_smoke and examples/journey_server.
set -euo pipefail
payload=$(realpath "${1:-target/release/mgbrowser}")
mkdir -p tmp
scratch=$(mktemp -d "$PWD/tmp/theme-smoke.XXXXXX")
# Pin the supplied bytes across restarts even if a concurrent build replaces its
# original target path. No changes are made to the supplied installation.
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
# No activation/service directories: portal loss must not start real helpers.
# The mock never owns the user's portal name; browsers use temporary XDG roots.
target/debug/examples/theme_smoke --bus-config "$scratch/bus.conf"
timeout 100s dbus-run-session --config-file "$scratch/bus.conf" -- \
    env MGBROWSER_THEME_PRIVATE_SESSION=1 \
    xvfb-run -a target/debug/examples/theme_smoke "$scratch/mgbrowser" "$scratch"
printf 'THEME_SMOKE_OK %s\n' "$scratch"
