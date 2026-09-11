#!/usr/bin/env bash
#
# Install the patched legion_laptop as its own DKMS module so kernel updates
# rebuild it automatically instead of silently reverting to the stock driver.
#
#   sudo ./install-dkms.sh            install / update
#   sudo ./install-dkms.sh --remove   uninstall and restore the stock module
#
set -euo pipefail

NAME=LenovoLegionLinux
VER=83sc
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/fork/kernel_module"
DEST=/usr/src/$NAME-$VER

die() { echo "install-dkms: $*" >&2; exit 1; }
info() { echo "  -> $*"; }

[[ $EUID -eq 0 ]] || die "must run as root: sudo ./install-dkms.sh"
command -v dkms >/dev/null || die "dkms is not installed"

if [[ "${1:-}" == "--remove" ]]; then
    dkms remove -m $NAME -v $VER --all 2>/dev/null || true
    rm -rf "$DEST"
    info "removed $NAME/$VER"
    if [[ -d /usr/src/$NAME-1.0.0 ]]; then
        dkms install -m $NAME -v 1.0.0 2>/dev/null \
            && info "stock $NAME/1.0.0 reinstalled" \
            || info "stock source present, run: dkms install -m $NAME -v 1.0.0"
    fi
    exit 0
fi

# Source resolution, in order of preference. The in-repo fork/ is optional:
# it was removed once the fixes landed upstream, so fall back to the already
# registered DKMS tree, then to the upstream release that carries them.
UPSTREAM_TAG=v0.0.26   # first tag containing the 83SC fixes (merge 9af57e3)
if [[ ! -f "$SRC/legion-laptop.c" ]]; then
    if [[ -f "$DEST/legion-laptop.c" ]] \
       && grep -q "has_single_fan" "$DEST/legion-laptop.c" 2>/dev/null; then
        info "fork/ absent; reusing the patched source already at $DEST"
        SRC="$DEST"
        REUSE_DEST=1
    elif command -v git >/dev/null; then
        TMPSRC=$(mktemp -d)
        info "fork/ absent; fetching upstream $UPSTREAM_TAG (contains the 83SC fixes)"
        if git clone --quiet --depth 1 --branch "$UPSTREAM_TAG" \
             https://github.com/johnfanv2/LenovoLegionLinux.git "$TMPSRC/LLL" 2>/dev/null \
           && grep -q "has_single_fan" "$TMPSRC/LLL/kernel_module/legion-laptop.c" 2>/dev/null; then
            SRC="$TMPSRC/LLL/kernel_module"
        else
            die "could not obtain patched source (no fork/, no $DEST, upstream fetch failed)"
        fi
    else
        die "patched source not found at $SRC and no fallback available (install git, or restore fork/)"
    fi
fi
[[ -f "$SRC/legion-laptop.c" ]] || die "patched source not found at $SRC"

# This kernel is built with clang+LTO on CachyOS; building the module with gcc
# fails on -fsplit-lto-unit. Detect rather than hardcode, so a future gcc
# kernel still builds.
MAKEFLAGS_EXTRA=""
KCONFIG=/proc/config.gz
if [[ -r $KCONFIG ]] && zcat $KCONFIG 2>/dev/null | grep -q "^CONFIG_CC_IS_CLANG=y"; then
    MAKEFLAGS_EXTRA=" LLVM=1"
    info "kernel is clang-built, adding LLVM=1"
fi

# The distro module builds the same legion-laptop.ko. Leaving both registered
# means whichever DKMS installs last wins, silently. Drop it.
if dkms status -m $NAME 2>/dev/null | grep -q "$NAME/1.0.0"; then
    info "removing stock $NAME/1.0.0 from dkms (source kept at /usr/src)"
    dkms remove -m $NAME -v 1.0.0 --all 2>/dev/null || true
fi

dkms status -m $NAME -v $VER 2>/dev/null | grep -q . && {
    info "removing previous $NAME/$VER"
    dkms remove -m $NAME -v $VER --all 2>/dev/null || true
}

if [[ "${REUSE_DEST:-0}" == 1 ]]; then
    info "source already in place at $DEST"
else
    info "installing source to $DEST"
    rm -rf "$DEST"
    install -d -m 0755 "$DEST"
    cp -a "$SRC"/. "$DEST"/
fi
rm -f "$DEST"/*.o "$DEST"/*.ko "$DEST"/*.mod* "$DEST"/Module.symvers "$DEST"/modules.order 2>/dev/null || true

cat > "$DEST/dkms.conf" <<EOF
PACKAGE_NAME="$NAME"
PACKAGE_VERSION="$VER"
MAKE[0]="make KERNELVERSION=\${kernelver}$MAKEFLAGS_EXTRA"
CLEAN="make clean"
BUILT_MODULE_NAME[0]="legion-laptop"
DEST_MODULE_NAME[0]="legion-laptop"
DEST_MODULE_LOCATION[0]="/kernel/drivers/platform/x86"
AUTOINSTALL="yes"
EOF

dkms add -m $NAME -v $VER
dkms build -m $NAME -v $VER
dkms install -m $NAME -v $VER --force

echo
dkms status -m $NAME
echo
info "reloading module"
modprobe -r legion_laptop 2>/dev/null || true
modprobe legion_laptop || die "modprobe failed"

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
