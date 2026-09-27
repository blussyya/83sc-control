#!/usr/bin/env bash
# Register / unregister the patched legion_laptop with DKMS. The source tree
# must already be at /usr/src/LenovoLegionLinux-83sc (the package puts it
# there; install-dkms.sh stages it first).
#   driver-setup.sh install   build for the running kernel, swap the module in
#   driver-setup.sh remove    drop ours, restore the stock DKMS module if present
#
# Called from package scripts, so it never fails the transaction: problems are
# printed with the command that fixes them. Later kernels are rebuilt by
# DKMS itself (AUTOINSTALL=yes).
set -uo pipefail

NAME=LenovoLegionLinux
VER=83sc
STOCK=1.0.0
KVER=$(uname -r)

say()  { echo "  83sc driver: $*"; }
have() { command -v "$1" >/dev/null 2>&1; }

reload() {
    have modprobe || return 0
    modprobe -r legion_laptop 2>/dev/null || true
    modprobe legion_laptop 2>/dev/null || { say "modprobe legion_laptop failed (reboot will load it)"; return 0; }
    # A reload orphans UPower's keyboard-LED handle (breaks the idle dimmer).
    systemctl is-active --quiet upower 2>/dev/null && systemctl restart upower 2>/dev/null
    return 0
}

case "${1:-}" in
install)
    have dkms || { say "dkms not installed - install it, then: sudo dkms autoinstall"; exit 0; }
    [[ -f /usr/src/$NAME-$VER/dkms.conf ]] || { say "source missing at /usr/src/$NAME-$VER"; exit 0; }

    # The stock module builds the same legion-laptop.ko; with both registered,
    # whichever DKMS installs last wins. Keep its source, drop the registration.
    if dkms status -m $NAME -v $STOCK 2>/dev/null | grep -q .; then
        say "unregistering stock $NAME/$STOCK"
        dkms remove -m $NAME -v $STOCK --all >/dev/null 2>&1 || true
    fi
    # Start clean so an upgrade rebuilds from the new source.
    dkms remove -m $NAME -v $VER --all >/dev/null 2>&1 || true
    dkms add -m $NAME -v $VER >/dev/null 2>&1 || true

    if [[ ! -d /lib/modules/$KVER/build ]]; then
        say "no kernel headers for $KVER - install them, then: sudo dkms autoinstall"
        exit 0
    fi
    say "building for $KVER (takes ~30 s)"
    if dkms install -m $NAME -v $VER -k "$KVER" --force >/tmp/83sc-dkms.log 2>&1; then
        say "installed"
        # Skip the swap inside a chroot or container: the running kernel is
        # not the one being installed for.
        if [[ -d /sys/module ]] && ! { have systemd-detect-virt && systemd-detect-virt -qc; }; then
            reload
        fi
    else
        say "build failed, log: /tmp/83sc-dkms.log"
    fi
    ;;
remove)
    have dkms || exit 0
    dkms remove -m $NAME -v $VER --all >/dev/null 2>&1 || true
    if [[ -f /usr/src/$NAME-$STOCK/dkms.conf ]]; then
        dkms install -m $NAME -v $STOCK -k "$KVER" >/dev/null 2>&1 \
            && say "stock $NAME/$STOCK restored"
    fi
    reload
    ;;
*)
    echo "usage: $0 install|remove" >&2; exit 2 ;;
esac
exit 0
