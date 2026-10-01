#!/usr/bin/env bash
# Exercise native menu input and worker re-exec after executable replacement.
set -euo pipefail
payload=${1:-target/release/mgbrowser}
scratch=$(mktemp -d "$PWD/tmp/chrome-update.XXXXXX")
install -m 755 "$payload" "$scratch/mgbrowser"
install -m 755 "$payload" "$scratch/updated-payload"
target/debug/examples/journey_server > "$scratch/server.log" 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true; wait "$server" 2>/dev/null || true' EXIT
for attempt in $(seq 1 50); do
    kill -0 "$server"
    if curl -fsS http://127.0.0.1:7878/ >/dev/null; then break; fi
    sleep 0.1
done
export MGBROWSER_CHROME_SMOKE="$scratch"
timeout 60s xvfb-run -a bash <<'SMOKE'
set -euo pipefail
scratch=$MGBROWSER_CHROME_SMOKE
export MGBROWSER_RESTART_PAYLOAD="$scratch/updated-payload"
XDG_CONFIG_HOME="$scratch/config" DBUS_SESSION_BUS_ADDRESS="unix:path=$scratch/no-bus" \
    "$scratch/mgbrowser" http://127.0.0.1:7878/ --enable-scripts --remote-debugging-port=0 > "$scratch/browser.log" 2>&1 &
browser=$!
restarted=
trap 'kill "$browser" ${restarted:+"$restarted"} 2>/dev/null || true; wait "$browser" 2>/dev/null || true' EXIT
for attempt in $(seq 1 100); do
    kill -0 "$browser"
    window=$(sed -n 's/^WINDOW id=\([0-9]*\).*/\1/p' "$scratch/browser.log")
    port=$(sed -n 's#^CDP listening on ws://127.0.0.1:\([0-9]*\)/.*#\1#p' "$scratch/browser.log")
    if [[ -n $window && -n $port ]] && grep -q '^LOADED ' "$scratch/browser.log"; then break; fi
    sleep 0.1
done
target/debug/examples/chrome_smoke "$window" "$scratch/about.png"
# Replace only this test-owned copy with a non-browser. Subsequent scripted
# navigation must re-exec the running inode, not the replacement at its old path.
install -m 755 /usr/bin/false "$scratch/replacement"
mv -fT "$scratch/replacement" "$scratch/mgbrowser"
target/debug/examples/cdp_journey "ws://127.0.0.1:$port/devtools/page/page-1" http://127.0.0.1:7878/script-boa "$scratch/after-replacement.png"
# A newer on-disk build is recognized locally even when the release API is
# unavailable. No updater URL override or production test flag is introduced.
install -m 755 tests/fixtures/restart-wrapper.sh "$scratch/restart-wrapper"
mv -fT "$scratch/restart-wrapper" "$scratch/mgbrowser"
target/debug/examples/chrome_smoke "$window" "$scratch/unused.png" update
for attempt in $(seq 1 100); do
    kill -0 "$browser"
    if grep -q '^UPDATE: Installed v999.0.0' "$scratch/browser.log"; then break; fi
    sleep 0.1
done
grep -q '^UPDATE: Installed v999.0.0' "$scratch/browser.log"
# Failed spawn keeps the original browser available, with a retryable button.
mv "$scratch/mgbrowser" "$scratch/restart-wrapper"
target/debug/examples/chrome_smoke "$window" "$scratch/restart-ready.png" restart
kill -0 "$browser"
! grep -q '^RESTART: launched' "$scratch/browser.log"
mv "$scratch/restart-wrapper" "$scratch/mgbrowser"
loads=$(grep -c '^LOADED ' "$scratch/browser.log")
target/debug/examples/chrome_smoke "$window" "$scratch/restart-retry.png" restart
for attempt in $(seq 1 100); do
    restarted=$(sed -n 's/^RESTART: launched pid=//p' "$scratch/browser.log")
    if [[ -n $restarted ]] && [[ $(grep -c '^WINDOW id=' "$scratch/browser.log") == 2 ]] && \
        (( $(grep -c '^LOADED ' "$scratch/browser.log") > loads )) && \
        tail -n 5 "$scratch/browser.log" | grep -q 'LOADED .*destination'; then break; fi
    sleep 0.1
done
wait "$browser"
[[ -n $restarted ]]
kill -0 "$restarted"
grep -q '^RESTART_WRAPPER_EXEC' "$scratch/browser.log"
[[ $(grep -c '^WINDOW id=' "$scratch/browser.log") == 2 ]]
(( $(grep -c '^LOADED ' "$scratch/browser.log") > loads ))
tail -n 5 "$scratch/browser.log" | grep -q 'LOADED .*destination'
printf 'NATIVE_UPDATED_BINARY_RESTART_OK\n'
SMOKE
printf 'CHROME_UPDATE_SMOKE_OK %s\n' "$scratch"
