#!/usr/bin/env bash
# Lay out the patched legion_laptop as a DKMS source tree.
#   packaging/stage-driver.sh DEST    ->  DEST/{legion-laptop.c,Makefile,dkms.conf}
# Used by install-dkms.sh and every package build, so all of them ship the
# same source. DEST is normally .../usr/src/LenovoLegionLinux-83sc.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${1:?usage: stage-driver.sh DEST}"

install -d -m 0755 "$DEST"
install -m 0644 "$ROOT/driver/legion-laptop.c" "$ROOT/driver/Makefile" "$DEST/"

for p in "$ROOT"/patches/*.patch; do
    [[ -f $p ]] || continue
    patch -d "$DEST" -p2 -N -s --no-backup-if-mismatch < "$p" \
        || { echo "stage-driver: ${p##*/} does not apply" >&2; exit 1; }
done

# No LLVM=1 here: dkms >= 3.0 adds it by itself when the target kernel was
# built with clang (CachyOS), and a gcc kernel must not get it.
cat > "$DEST/dkms.conf" <<'CONF'
PACKAGE_NAME="LenovoLegionLinux"
PACKAGE_VERSION="83sc"
MAKE[0]="make KERNELVERSION=${kernelver}"
CLEAN="make clean"
BUILT_MODULE_NAME[0]="legion-laptop"
DEST_MODULE_NAME[0]="legion-laptop"
DEST_MODULE_LOCATION[0]="/kernel/drivers/platform/x86"
AUTOINSTALL="yes"
CONF
