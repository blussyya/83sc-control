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

# The distro ships a competing DKMS module built from a pre-fix commit. On a
# kernel upgrade whichever installs last wins, and when the stock one wins the
# fan curve silently stops working. The guard detects and repairs that.
install -Dm755 "$HERE/systemd/83sc-driver-guard.sh" "$LIBDIR/83sc-driver-guard.sh"
install -Dm644 "$HERE/systemd/83sc-driver-guard.service" /etc/systemd/system/83sc-driver-guard.service
echo "  -> $LIBDIR/83sc-driver-guard.sh"
echo "  -> /etc/systemd/system/83sc-driver-guard.service"

# On Arch-likes, repair right after the upgrade instead of waiting for a reboot.
if [[ -d /etc/pacman.d/hooks || -d /usr/share/libalpm/hooks ]]; then
    install -Dm644 "$HERE/pacman/83sc-driver.hook" /etc/pacman.d/hooks/83sc-driver.hook
    echo "  -> /etc/pacman.d/hooks/83sc-driver.hook"
fi

systemctl daemon-reload
systemctl enable 83sc-driver-guard.service
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
