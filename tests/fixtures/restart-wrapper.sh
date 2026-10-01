#!/usr/bin/env bash
# Test-only replacement: advertises a newer installed build without network API.
# All GUI launches exec the real packaged payload at a different inode/path.
set -euo pipefail
if [[ ${1:-} == --version ]]; then
    printf 'mgbrowser 999.0.0\n'
    exit 0
fi
printf 'RESTART_WRAPPER_EXEC\n'
[[ $1 == http://127.0.0.1:7878/destination* ]]
[[ $2 == --enable-scripts ]]
[[ $3 == --no-auto-update ]]
[[ $# == 3 ]]
exec "$MGBROWSER_RESTART_PAYLOAD" "$@"
