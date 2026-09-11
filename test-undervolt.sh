#!/usr/bin/env bash
#
# Does the undervolt mailbox actually accept writes on this machine?
#
#   sudo ./test-undervolt.sh
#
# Applies a deliberately tiny -25 mV core offset, reads it back, then reverts to
# 0 regardless of outcome. -25 mV is far inside any stable range, so this tests
# whether the write lands, not whether the chip survives it.
set -uo pipefail

CONF=/etc/intel-undervolt.conf
BACKUP=$(mktemp)

[[ $EUID -eq 0 ]] || { echo "run as root: sudo ./test-undervolt.sh" >&2; exit 1; }
command -v intel-undervolt >/dev/null || { echo "intel-undervolt not installed" >&2; exit 1; }

cp "$CONF" "$BACKUP"
# Always put the original config back, even on Ctrl-C or an error.
cleanup() {
    cp "$BACKUP" "$CONF"
    rm -f "$BACKUP"
    intel-undervolt apply >/dev/null 2>&1 || true
    echo
    echo "  reverted to 0 mV and restored $CONF"
}
trap cleanup EXIT INT TERM

echo "=== before ==="
intel-undervolt read

sed -i -E "s/^enable .*/enable yes/; s/^undervolt 0 'CPU' .*/undervolt 0 'CPU' -25/" "$CONF"

echo
echo "=== applying -25 mV to CPU core ==="
intel-undervolt apply 2>&1 | sed 's/^/  /'

echo
echo "=== reading back ==="
OUT=$(intel-undervolt read)
echo "$OUT"

echo
if echo "$OUT" | grep -qE 'CPU \(0\): *-2[0-9]\.'; then
    echo "  RESULT: writes LAND - undervolting is available on this machine"
    echo "  next step would be a stability sweep, -50 then -75 then -100 mV"
else
    echo "  RESULT: write did NOT stick - the mailbox is locked"
    echo "  consistent with issupportcpuoc = 0; no userspace tool can change this"
fi
