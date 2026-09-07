#!/usr/bin/env bash
# Put the CLI tools on PATH so they work from any directory.
#   sudo ./install-cli.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ $EUID -eq 0 ]] || { echo "run as root: sudo ./install-cli.sh" >&2; exit 1; }

for t in 83sc 83sc-diag 83sc-fan 83sc-snap; do
    [[ -f "$HERE/bin/$t" ]] || { echo "missing $HERE/bin/$t" >&2; exit 1; }
    install -Dm755 "$HERE/bin/$t" "/usr/local/bin/$t"
    echo "  -> /usr/local/bin/$t"
done


# Without this the fan curve is the one setting that does not survive a reboot:
# 83sc-thermal.service restores power limits from its own script, but reads the
# curve from this file and silently skips it when absent.
CURVE=/etc/83sc-control/curve.conf
if [[ -f $CURVE ]]; then
    info() { :; }
    echo "  -> $CURVE already exists, left alone"
else
    if /usr/local/bin/83sc-fan save-boot >/dev/null 2>&1 && [[ -f $CURVE ]]; then
        echo "  -> $CURVE written from the live curve"
    else
        echo "  !! could not write $CURVE - the fan curve will NOT survive reboot." >&2
        echo "     apply a curve, then run: sudo 83sc-fan save-boot" >&2
    fi
fi

echo
echo "now works from anywhere:  83sc-fan show"
