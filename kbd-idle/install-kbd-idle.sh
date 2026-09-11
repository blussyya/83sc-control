#!/usr/bin/env bash
# Install the idle dimmer for the current user and start it. No root needed:
# it talks to UPower over D-Bus, so it lives entirely in the user session.
#   ./install-kbd-idle.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$HERE/target/release/83sc-kbd-idle"

[[ $EUID -ne 0 ]] || { echo "run as yourself, not root" >&2; exit 1; }
[[ -x $BIN ]] || { echo "not built yet, run: cargo build --release" >&2; exit 1; }

install -Dm755 "$BIN" ~/.local/bin/83sc-kbd-idle
install -Dm644 "$HERE/../systemd/83sc-kbd-idle.service" ~/.config/systemd/user/83sc-kbd-idle.service
systemctl --user daemon-reload
systemctl --user enable --now 83sc-kbd-idle
echo "installed:"
echo "  ~/.local/bin/83sc-kbd-idle"
echo "  ~/.config/systemd/user/83sc-kbd-idle.service"
echo
systemctl --user --no-pager status 83sc-kbd-idle | head -4
echo
echo "timeout is 5s; change it in ~/.config/83sc-control/kbd-idle.conf:"
echo "  timeout=10"
echo "then: systemctl --user restart 83sc-kbd-idle"
