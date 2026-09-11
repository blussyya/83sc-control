#!/usr/bin/env bash
# Install the boot-time thermal/power service. This is what makes the stutter
# fix persist: at boot it replays /etc/83sc-control/boot.conf (power limits,
# undervolt, fan curve, powermode) written by the GUI or `83sc boot-save`.
#   sudo ./install-thermal.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIBDIR=/usr/local/lib/83sc-control
UNIT=/etc/systemd/system/83sc-thermal.service

[[ $EUID -eq 0 ]] || { echo "run as root: sudo ./install-thermal.sh" >&2; exit 1; }
[[ -f "$HERE/systemd/83sc-thermal.sh" ]] || { echo "missing systemd/83sc-thermal.sh" >&2; exit 1; }

install -Dm755 "$HERE/systemd/83sc-thermal.sh" "$LIBDIR/83sc-thermal.sh"
install -Dm644 "$HERE/systemd/83sc-thermal.service" "$UNIT"
echo "  -> $LIBDIR/83sc-thermal.sh"
echo "  -> $UNIT"

systemctl daemon-reload
systemctl enable 83sc-thermal.service
echo "  -> enabled (runs at boot)"

if [[ ! -r /etc/83sc-control/boot.conf ]]; then
    echo "  !! /etc/83sc-control/boot.conf not found yet."
    echo "     Apply your settings in the GUI (or run: 83sc boot-save) so the"
    echo "     service has a full snapshot to replay. Without it, the service"
    echo "     falls back to the built-in 'game' profile."
fi
echo
echo "start now without a reboot:  sudo systemctl start 83sc-thermal.service"
echo "check it:                    systemctl status 83sc-thermal.service"
