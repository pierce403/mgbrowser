#!/usr/bin/env bash
# Build this checkout and install a user-owned XQuartz app. No public downloads
# of Mg binaries are implied: the website installer still targets Linux only.
set -euo pipefail
[[ $(uname -s) == Darwin ]] || { echo 'This source installer requires macOS.' >&2; exit 1; }
command -v cargo >/dev/null || { echo 'Install Rust 1.91 or newer first.' >&2; exit 1; }
[[ -d /Applications/Utilities/XQuartz.app ]] || {
    echo 'Install XQuartz from https://www.xquartz.org/ first, then log out and back in.' >&2
    exit 1
}
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
cargo build --locked --release --bin mgbrowser
app="$HOME/Applications/mgbrowser.app"
[[ ! -e "$app" ]] || { echo "Already exists: $app. Move it aside before installing." >&2; exit 1; }
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 target/release/mgbrowser "$app/Contents/MacOS/mgbrowser"
install -m 644 LICENSE NOTICE "$app/Contents/Resources/"
cat > "$app/Contents/MacOS/launch" <<'LAUNCH'
#!/bin/bash
set -euo pipefail
if [[ -z ${DISPLAY:-} ]]; then
    export DISPLAY=$(/bin/launchctl getenv DISPLAY)
fi
/usr/bin/open -a XQuartz
# XQuartz normally uses an on-demand launchd socket; wait for first launch to
# publish it when Finder's environment predates the XQuartz installation.
for ((attempt=0; attempt<30 && ${#DISPLAY}==0; attempt++)); do
    sleep 1
    export DISPLAY=$(/bin/launchctl getenv DISPLAY)
done
if [[ -z $DISPLAY ]]; then
    /usr/bin/osascript -e 'display alert "mgbrowser needs XQuartz" message "Log out and back in after installing XQuartz, then open mgbrowser again."'
    exit 1
fi
cd -- "$(dirname -- "$0")"
exec ./mgbrowser "${@:-https://example.com/}"
LAUNCH
chmod 755 "$app/Contents/MacOS/launch"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>mgbrowser</string>
<key>CFBundleIdentifier</key><string>org.mgbrowser.local</string>
<key>CFBundleExecutable</key><string>launch</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf 'Installed source build: %s\nOpen with: open "%s"\n' "$app" "$app"
