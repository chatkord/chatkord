#!/usr/bin/env bash
# Installs the window's desktop entry and icons for this user (Linux), so the desktop's launcher, dock and task
# switcher show Chatkord's name and icon: Wayland takes no icon from a window, only from the entry its app id names.
# It is for a build run from the tree: the AppImage puts itself in the launcher, and the Windows setup makes its own
# entry. The entry starts BINARY, by default the release window if one is built, else the debug one.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

binary="${1:-}"
if [[ -z "$binary" ]]; then
    for built in client/target/release/shell client/target/debug/shell; do
        [[ -x "$built" ]] && { binary="$built"; break; }
    done
fi
[[ -n "$binary" && -x "$binary" ]] || { echo "FAIL: no window built; cargo build -p shell in client, or name one"; exit 1; }
binary="$(realpath "$binary")"

data="${XDG_DATA_HOME:-$HOME/.local/share}"
for png in client/shell/assets/icon/chatkord-*.png; do
    side="${png##*-}"; side="${side%.png}"
    install -D -m 644 "$png" "$data/icons/hicolor/${side}x${side}/apps/chatkord.png"
done
mkdir -p "$data/applications"
sed "s|^Exec=.*|Exec=$binary|" client/shell/assets/chatkord.desktop > "$data/applications/chatkord.desktop"
command -v gtk-update-icon-cache > /dev/null && gtk-update-icon-cache -q -t "$data/icons/hicolor" || true
command -v update-desktop-database > /dev/null && update-desktop-database -q "$data/applications" || true
echo "installed chatkord.desktop and its icons in $data, starting $binary"
