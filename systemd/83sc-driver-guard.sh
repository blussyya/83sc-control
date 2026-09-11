#!/usr/bin/env bash
# Make sure the PATCHED legion_laptop is the one actually loaded.
#
# Why this exists: the distro ships its own DKMS module (LenovoLegionLinux/1.0.0)
# built from a commit that predates our upstream fixes. Both register the same
# legion-laptop.ko, so on a kernel upgrade whichever DKMS installs last wins --
# silently. When the stock one wins, fan curve writes fail and the temperature
# points read back 0, which is the original bug this project fixed.
#
# Runs before 83sc-thermal.service, and is a no-op when things are already right.
set -uo pipefail

NAME=LenovoLegionLinux
OURS=83sc
STOCK=1.0.0
KVER=$(uname -r)

log() { echo "83sc-driver-guard: $*"; }

find_hwmon() {
    local h
    for h in /sys/class/hwmon/hwmon*; do
        [[ "$(cat "$h/name" 2>/dev/null)" == legion_hwmon ]] && { echo "$h"; return 0; }
    done
    return 1
}

# The patched module hides the phantom fan2 and reports a per-model fan_max.
# Either check alone is enough to tell the two builds apart.
is_patched() {
    local h; h=$(find_hwmon) || return 1
    [[ -e "$h/fan2_input" ]] && return 1
    [[ "$(cat "$h/fan1_max" 2>/dev/null)" == "5400" ]] || return 1
    return 0
}

if is_patched; then
    log "patched module active, nothing to do"
    exit 0
fi

log "stock module detected (phantom fan2 present or fan1_max != 5400) - repairing"

command -v dkms >/dev/null || { log "dkms not installed, cannot repair"; exit 1; }

# Drop the stock registration so it cannot win the next autoinstall either.
if dkms status -m $NAME -v $STOCK 2>/dev/null | grep -q .; then
    log "removing stock $NAME/$STOCK from dkms"
    dkms remove -m $NAME -v $STOCK --all >/dev/null 2>&1 || true
fi

if ! dkms status -m $NAME -v $OURS -k "$KVER" 2>/dev/null | grep -q "installed"; then
    log "installing $NAME/$OURS for $KVER"
    dkms build -m $NAME -v $OURS -k "$KVER" >/dev/null 2>&1 || true
    dkms install -m $NAME -v $OURS -k "$KVER" --force >/dev/null 2>&1 \
        || { log "dkms install failed"; exit 1; }
fi

log "reloading legion_laptop"
modprobe -r legion_laptop 2>/dev/null || true
modprobe legion_laptop 2>/dev/null || { log "modprobe failed"; exit 1; }

if is_patched; then
    log "repaired: patched module now active"
    exit 0
fi
log "STILL not patched after repair - fan curve will not work"
exit 1
