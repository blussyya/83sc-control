#!/usr/bin/env bash
# Install the GUI and its app-menu entry.
#   sudo ./install-gui.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$HERE/target/release/legion83-gui"

[[ $EUID -eq 0 ]] || { echo "run as root: sudo ./install-gui.sh" >&2; exit 1; }
[[ -x $BIN ]] || { echo "not built yet, run: cargo build --release" >&2; exit 1; }

install -Dm755 "$BIN" /usr/local/bin/legion83-gui
install -Dm644 "$HERE/83sc-control.desktop" /usr/share/applications/83sc-control.desktop
update-desktop-database /usr/share/applications 2>/dev/null || true

echo "installed:"
echo "  /usr/local/bin/legion83-gui"
echo "  /usr/share/applications/83sc-control.desktop"
echo
echo "'83SC Control' should now appear in the application menu."
