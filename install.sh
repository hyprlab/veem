#!/usr/bin/env bash
#
# Install Veem for the current user: builds the release binary and installs it
# along with the app icon and desktop entry into the XDG user prefix, so it
# appears (with its icon) in the GNOME dash, overview, and alt-tab.
#
# Usage:  ./install.sh            # installs into ~/.local
#         PREFIX=/usr ./install.sh   # system-wide (run with sudo)
set -euo pipefail

APP_ID="com.getveem.Veem"
PREFIX="${PREFIX:-$HOME/.local}"
ROOT="$(cd "$(dirname "$0")" && pwd)"

echo "==> Building release binary"
( cd "$ROOT" && cargo build --release )

echo "==> Installing binary to $PREFIX/bin/veem"
install -Dm755 "$ROOT/target/release/veem" "$PREFIX/bin/veem"

echo "==> Installing icons"
for size in 256x256 512x512; do
    install -Dm644 "$ROOT/data/icons/hicolor/$size/apps/$APP_ID.png" \
        "$PREFIX/share/icons/hicolor/$size/apps/$APP_ID.png"
done
# Bundled symbolic icons (not in all icon themes).
for sym in "$ROOT"/data/icons/hicolor/scalable/actions/*-symbolic.svg; do
    [ -e "$sym" ] || continue
    install -Dm644 "$sym" "$PREFIX/share/icons/hicolor/scalable/actions/$(basename "$sym")"
done

echo "==> Installing desktop entry"
install -Dm644 "$ROOT/data/$APP_ID.desktop" \
    "$PREFIX/share/applications/$APP_ID.desktop"

echo "==> Updating caches"
gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true

echo "==> Done. Launch 'veem' (you may need to log out/in for the shell icon)."
