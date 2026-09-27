#!/usr/bin/env bash
#
# Install the patched legion_laptop as its own DKMS module so kernel updates
# rebuild it automatically instead of silently reverting to the stock driver.
# (The distro packages do the same thing; this is the from-a-clone path.)
#
#   sudo ./install-dkms.sh            install / update
#   sudo ./install-dkms.sh --remove   uninstall and restore the stock module
#
set -euo pipefail

NAME=LenovoLegionLinux
VER=83sc
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST=/usr/src/$NAME-$VER

die() { echo "install-dkms: $*" >&2; exit 1; }
info() { echo "  -> $*"; }

[[ $EUID -eq 0 ]] || die "must run as root: sudo ./install-dkms.sh"
command -v dkms >/dev/null || die "dkms is not installed"

if [[ "${1:-}" == "--remove" ]]; then
    "$HERE/packaging/driver-setup.sh" remove
    rm -rf "$DEST"
    info "removed $NAME/$VER"
    exit 0
fi

if command -v pacman >/dev/null && pacman -Qo "$DEST" >/dev/null 2>&1; then
    die "$DEST belongs to the 83sc-control package; update the package instead"
fi
[[ -d /lib/modules/$(uname -r)/build ]] || die "no kernel headers for $(uname -r)"

info "staging driver/ + patches/ into $DEST"
rm -rf "$DEST"
"$HERE/packaging/stage-driver.sh" "$DEST"
"$HERE/packaging/driver-setup.sh" install

echo
dkms status -m $NAME
H=$(for h in /sys/class/hwmon/hwmon*; do
        [[ "$(cat "$h/name" 2>/dev/null)" == legion_hwmon ]] && echo "$h" && break
    done)
if [[ -n "$H" ]]; then
    echo
    info "fan1_max = $(cat "$H/fan1_max" 2>/dev/null) (patched build reports 5400)"
    info "point1 temp = $(cat "$H/pwm1_auto_point1_temp" 2>/dev/null) (0 means the fancurve fix is not active)"
    info "fan2_input present: $([[ -e $H/fan2_input ]] && echo yes || echo 'no - phantom fan hidden')"
fi
echo
info "done - kernel updates will now rebuild the patched module"
