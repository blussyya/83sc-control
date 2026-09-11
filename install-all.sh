#!/usr/bin/env bash
# One-shot installer for the whole 83sc-control stack. Distro-agnostic:
# it checks for what each step needs and tells you what to install if missing,
# rather than assuming Arch/CachyOS. Safe to re-run.
#
#   sudo ./install-all.sh
#
# Steps, in dependency order:
#   1. helper + sudoers rule        (install.sh)
#   2. CLI tools + curve.conf        (install-cli.sh)
#   3. boot-time thermal service     (install-thermal.sh)   <- persistence
#   4. patched legion_laptop (DKMS)  (install-dkms.sh)      <- optional if your
#                                      kernel already ships the upstream fix
#   5. GUI (built from source)       (gui/install-gui.sh)
#   6. keyboard idle dimmer (user)   (kbd-idle/install-kbd-idle.sh)
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ $EUID -eq 0 ]] || { echo "run as root: sudo ./install-all.sh" >&2; exit 1; }
USER_NAME="${SUDO_USER:-}"
[[ -n "$USER_NAME" && "$USER_NAME" != root ]] || { echo "run via sudo, not from a root shell" >&2; exit 1; }
uid=$(id -u "$USER_NAME")
as_user() { sudo -u "$USER_NAME" XDG_RUNTIME_DIR="/run/user/$uid" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$uid/bus" "$@"; }

have() { command -v "$1" >/dev/null 2>&1; }
step() { echo; echo "== $* =="; }
warn() { echo "  !! $*" >&2; }

# --- dependency check (names differ per distro; report, don't guess) ---
step "checking dependencies"
miss=()
have dkms   || miss+=("dkms")
have cargo  || miss+=("cargo/rust")
have gcc || have clang || miss+=("a C compiler (gcc or clang)")
[[ -d /lib/modules/$(uname -r)/build ]] || miss+=("kernel headers for $(uname -r)")
have intel-undervolt || warn "intel-undervolt missing: the -100mV offset won't replay at boot until it's installed (optional)"
if ((${#miss[@]})); then
    warn "missing: ${miss[*]}"
    warn "install them with your package manager, then re-run. On Debian:"
    warn "  apt install dkms build-essential linux-headers-\$(uname -r) cargo"
    echo
    read -r -p "continue anyway (skip steps that need the missing tools)? [y/N] " a
    [[ ${a,,} == y ]] || exit 1
fi

step "1/6 helper + sudoers";        "$HERE/install.sh" || warn "install.sh failed"
step "2/6 CLI tools";               "$HERE/install-cli.sh" || warn "install-cli.sh failed"
step "3/6 thermal service";         "$HERE/install-thermal.sh" || warn "install-thermal.sh failed"

if have dkms && [[ -d /lib/modules/$(uname -r)/build ]]; then
    step "4/6 DKMS driver";         "$HERE/install-dkms.sh" || warn "install-dkms.sh failed"
else
    step "4/6 DKMS driver";         warn "skipped (needs dkms + headers). If your kernel already has the"
    warn "   upstream fix (legion-laptop >= the 9af57e3 merge), you don't need this."
fi

if have cargo; then
    step "5/6 GUI (cargo build --release)"
    ( cd "$HERE/gui" && as_user cargo build --release ) && "$HERE/gui/install-gui.sh" || warn "GUI build/install failed"
    step "6/6 keyboard idle dimmer"
    ( cd "$HERE/kbd-idle" && as_user cargo build --release ) && as_user "$HERE/kbd-idle/install-kbd-idle.sh" || warn "kbd-idle build/install failed"
else
    warn "cargo missing: skipped GUI and kbd-idle (both build from source)"
fi

echo; echo "== done =="
echo "Apply your settings in the GUI, then: sudo systemctl start 83sc-thermal.service"
echo "Everything replays from /etc/83sc-control/boot.conf on the next boot."
