#!/usr/bin/env bash
# Local installed-newer fixture, not a download or a real future release.
# Validate literal relaunch arguments, then exec the actual copied payload.
set -euo pipefail
if [[ ${1:-} == --version ]]; then
    printf 'mgbrowser 999.0.0\n'
    exit 0
fi
[[ $# == 7 ]]
[[ $1 == http://127.0.0.1:7878/ ]]
[[ $2 == --restore-tab ]]
[[ $3 == http://127.0.0.1:7878/script-boa ]]
[[ $4 == --restore-tab ]]
[[ $5 == http://127.0.0.1:7878/destination ]]
[[ $6 == --enable-scripts ]]
[[ $7 == --no-auto-update ]]
printf 'WORKSPACE_RESTART_ARGS_OK tabs=3\n'
exec "$MGBROWSER_RESTART_PAYLOAD" "$@"
