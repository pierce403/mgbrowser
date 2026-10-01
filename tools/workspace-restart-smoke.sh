#!/usr/bin/env bash
# Exercise native Update/Restart for three committed tabs on an owned display.
# The local wrapper advertises a newer installed build; no release API is used.
set -euo pipefail
payload=$(realpath "${1:-target/release/mgbrowser}")
mkdir -p tmp
scratch=$(mktemp -d "$PWD/tmp/workspace-restart.XXXXXX")
install -m 755 "$payload" "$scratch/mgbrowser"
install -m 755 "$payload" "$scratch/updated-payload"
sha256sum "$scratch/mgbrowser"
target/debug/examples/journey_server > "$scratch/server.log" 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true; wait "$server" 2>/dev/null || true' EXIT
for attempt in $(seq 1 100); do
    kill -0 "$server"
    if grep -q '^Local fixture service:' "$scratch/server.log" && \
        curl -fsS http://127.0.0.1:7878/ >/dev/null; then break; fi
    sleep 0.1
done
kill -0 "$server"
grep -q '^Local fixture service:' "$scratch/server.log"
timeout 90s env MGBROWSER_WORKSPACE_RESTART_SMOKE="$scratch" \
    xvfb-run -a -s '-screen 0 1920x1080x24 -nolisten tcp' bash <<'SMOKE'
set -euo pipefail
scratch=$MGBROWSER_WORKSPACE_RESTART_SMOKE
export MGBROWSER_RESTART_PAYLOAD="$scratch/updated-payload"
mkdir -p "$scratch/home" "$scratch/config" "$scratch/data" "$scratch/cache"
env HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" \
    XDG_DATA_HOME="$scratch/data" XDG_CACHE_HOME="$scratch/cache" \
    DBUS_SESSION_BUS_ADDRESS="unix:path=$scratch/no-bus" \
    "$scratch/mgbrowser" http://127.0.0.1:7878/ \
    --restore-tab http://127.0.0.1:7878/script-boa \
    --restore-tab http://127.0.0.1:7878/destination \
    --enable-scripts --no-auto-update > "$scratch/browser.log" 2>&1 &
browser=$!
restarted=
trap 'kill "$browser" ${restarted:+"$restarted"} 2>/dev/null || true; wait "$browser" 2>/dev/null || true' EXIT
urls=(http://127.0.0.1:7878/ http://127.0.0.1:7878/script-boa http://127.0.0.1:7878/destination)
for attempt in $(seq 1 150); do
    kill -0 "$browser"
    window=$(sed -n 's/^WINDOW id=\([0-9]*\).*/\1/p' "$scratch/browser.log" | head -n 1)
    ready=1
    for url in "${urls[@]}"; do
        if ! grep -Fq "LOADED $url HTTP 200" "$scratch/browser.log"; then ready=0; fi
    done
    if [[ -n $window && $ready == 1 ]]; then break; fi
    sleep 0.1
done
[[ -n $window && $ready == 1 ]]
[[ $(grep -c '^WINDOW id=' "$scratch/browser.log") == 1 ]]
grep -q '^TABS .*count=3' "$scratch/browser.log"
target/debug/examples/chrome_smoke "$window" "$scratch/about-before.png" about 34
install -m 755 tests/fixtures/workspace-restart-wrapper.sh "$scratch/replacement"
mv -fT "$scratch/replacement" "$scratch/mgbrowser"
target/debug/examples/chrome_smoke "$window" "$scratch/unused.png" update 34
for attempt in $(seq 1 100); do
    kill -0 "$browser"
    if grep -q '^UPDATE: Installed v999.0.0' "$scratch/browser.log"; then break; fi
    sleep 0.1
done
grep -q '^UPDATE: Installed v999.0.0' "$scratch/browser.log"
target/debug/examples/chrome_smoke "$window" "$scratch/restart-three-tabs.png" restart 34
for attempt in $(seq 1 150); do
    restarted=$(sed -n 's/^RESTART: launched pid=//p' "$scratch/browser.log")
    ready=1
    for url in "${urls[@]}"; do
        if [[ $(grep -Fc "LOADED $url HTTP 200" "$scratch/browser.log") -ne 2 ]]; then ready=0; fi
    done
    if [[ -n $restarted && $ready == 1 ]] && \
        grep -q '^WORKSPACE_RESTART_ARGS_OK tabs=3' "$scratch/browser.log"; then break; fi
    sleep 0.1
done
wait "$browser"
[[ -n $restarted && $ready == 1 ]]
kill -0 "$restarted"
grep -q '^WORKSPACE_RESTART_ARGS_OK tabs=3' "$scratch/browser.log"
[[ $(grep -c '^WINDOW id=' "$scratch/browser.log") == 2 ]]
new_window=$(sed -n 's/^WINDOW id=\([0-9]*\).*/\1/p' "$scratch/browser.log" | tail -n 1)
target/debug/examples/chrome_smoke "$new_window" "$scratch/about-after.png" about 34
! grep -q 'SCRIPT_SHUTDOWN_ERROR\|thread .*panicked' "$scratch/browser.log"
printf 'NATIVE_WORKSPACE_RESTART_OK tabs=3 new_windows=1\n'
SMOKE
printf 'WORKSPACE_RESTART_SMOKE_OK %s\n' "$scratch"
