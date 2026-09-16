#!/usr/bin/env bash
# Exercise native menu input and worker re-exec after executable replacement.
set -euo pipefail
payload=${1:-target/release/mgbrowser}
scratch=$(mktemp -d "$PWD/tmp/chrome-update.XXXXXX")
install -m 755 "$payload" "$scratch/mgbrowser"
target/debug/examples/journey_server > "$scratch/server.log" 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true; wait "$server" 2>/dev/null || true' EXIT
for attempt in $(seq 1 50); do
    kill -0 "$server"
    if curl -fsS http://127.0.0.1:7878/ >/dev/null; then break; fi
    sleep 0.1
done
export MGBROWSER_CHROME_SMOKE="$scratch"
timeout 45s xvfb-run -a bash <<'SMOKE'
set -euo pipefail
scratch=$MGBROWSER_CHROME_SMOKE
"$scratch/mgbrowser" http://127.0.0.1:7878/ --enable-scripts --remote-debugging-port=0 > "$scratch/browser.log" 2>&1 &
browser=$!
trap 'kill "$browser" 2>/dev/null || true; wait "$browser" 2>/dev/null || true' EXIT
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
SMOKE
printf 'CHROME_UPDATE_SMOKE_OK %s\n' "$scratch"
