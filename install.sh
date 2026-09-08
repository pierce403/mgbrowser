#!/usr/bin/env bash
# Downloads only the published binary, verifies its checksum, installs without sudo.
set -euo pipefail
die() { printf 'mgbrowser: %s\n' "$*" >&2; exit 1; }
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || die 'Supported target: Linux x86_64 with X11/XWayland.'
for tool in curl sha256sum tar install mktemp; do
    command -v "$tool" >/dev/null || die "Required command not found: $tool"
done
install_dir=${MGBROWSER_INSTALL_DIR:-"$HOME/.local/bin"}
data_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}
[[ $install_dir == /* && $data_dir == /* ]] || die 'Install and data directories must be absolute paths.'
# Desktop Exec has its own escaping rules. Reject uncommon path metacharacters
# rather than letting a user-selected directory become a launcher command.
case "$install_dir$data_dir" in *[\"\`\$\\%]*|*$'\n'*|*$'\r'*) die 'Install paths cannot contain quotes, backticks, $, backslash, %, or newlines.';; esac
asset=mgbrowser-linux-x86_64.tar.gz
base=https://github.com/pierce403/mgbrowser/releases/latest/download
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
curl -fsSL --proto '=https' --tlsv1.2 "$base/$asset" -o "$scratch/$asset"
curl -fsSL --proto '=https' --tlsv1.2 "$base/$asset.sha256" -o "$scratch/$asset.sha256"
read -r digest filename extra < "$scratch/$asset.sha256"
[[ $digest =~ ^[[:xdigit:]]{64}$ && $filename == "$asset" && -z $extra ]] || die 'Invalid release checksum file.'
(cd "$scratch" && sha256sum --check --strict "$asset.sha256")
tar -xzf "$scratch/$asset" -C "$scratch" --no-same-owner --no-same-permissions
payload="$scratch/mgbrowser-linux-x86_64"
install -d "$install_dir" "$data_dir/applications" "$data_dir/icons/hicolor/scalable/apps" "$data_dir/icons/hicolor/256x256/apps" "$data_dir/mgbrowser"
install -m 755 "$payload/mgbrowser" "$install_dir/mgbrowser"
install -m 644 "$payload/mgbrowser.svg" "$data_dir/icons/hicolor/scalable/apps/mgbrowser.svg"
install -m 644 "$payload/mgbrowser-256.png" "$data_dir/icons/hicolor/256x256/apps/mgbrowser.png"
install -m 644 "$payload/LICENSE" "$payload/README.md" "$payload/THIRD_PARTY_LICENSES.txt" "$data_dir/mgbrowser/"
printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=mgbrowser' \
    'Comment=Experimental Rust browser : limited web compatibility' \
    "Exec=\"$install_dir/mgbrowser\" %u" 'Icon=mgbrowser' 'Terminal=false' \
    'Categories=Network;WebBrowser;' 'MimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;' \
    'StartupWMClass=mgbrowser' > "$data_dir/applications/mgbrowser.desktop"
if command -v update-desktop-database >/dev/null; then update-desktop-database "$data_dir/applications" || true; fi
if command -v gtk-update-icon-cache >/dev/null; then gtk-update-icon-cache -f -t "$data_dir/icons/hicolor" >/dev/null 2>&1 || true; fi
printf 'Installed %s\n' "$("$install_dir/mgbrowser" --version)"
case ":$PATH:" in *":$install_dir:"*) ;; *) printf 'Add this directory to your PATH: %s\nFor this shell: export PATH="%s:$PATH"\n' "$install_dir" "$install_dir";; esac
font_found=false
for font in "${MGBROWSER_FONT:-/nonexistent}" \
    /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf \
    /usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf \
    /usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf; do
    if [[ -r $font ]]; then font_found=true; break; fi
done
if [[ $font_found == false ]]; then
    printf 'No supported font found. Set MGBROWSER_FONT to a readable TrueType/OpenType font file before launching.\n'
fi
printf 'Requires an X11/XWayland desktop. Try: mgbrowser https://example.com/\n'
