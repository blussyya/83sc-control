#!/usr/bin/env bash
# Install the idle dimmer and start it for the invoking user.
#   sudo ./install-kbd-idle.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$HERE/target/release/83sc-kbd-idle"

[[ $EUID -eq 0 ]] || { echo "run as root: sudo ./install-kbd-idle.sh" >&2; exit 1; }
[[ -x $BIN ]] || { echo "not built yet, run: cargo build --release" >&2; exit 1; }

install -Dm755 "$BIN" /usr/local/bin/83sc-kbd-idle
install -Dm644 "$HERE/../systemd/83sc-kbd-idle.service" /usr/local/lib/systemd/user/83sc-kbd-idle.service
echo "installed:"
echo "  /usr/local/bin/83sc-kbd-idle"
echo "  /usr/local/lib/systemd/user/83sc-kbd-idle.service"

# A user unit has to be enabled from inside that user's session, which sudo
# does not give us; point systemctl at the caller's user manager explicitly.
user="${SUDO_USER:-}"
if [[ -z $user ]]; then
    echo; echo "enable it from your session:  systemctl --user enable --now 83sc-kbd-idle"
    exit 0
fi
uid=$(id -u "$user")
run() { sudo -u "$user" XDG_RUNTIME_DIR="/run/user/$uid" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" "$@"; }
run systemctl --user daemon-reload
run systemctl --user enable --now 83sc-kbd-idle
echo
run systemctl --user --no-pager status 83sc-kbd-idle | head -5
echo
echo "timeout is 5s; change it in ~/.config/83sc-control/kbd-idle.conf:"
echo "  timeout=10"
echo "then: systemctl --user restart 83sc-kbd-idle"
