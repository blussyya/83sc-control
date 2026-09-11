#!/usr/bin/env bash
#
# 83sc-control : one command to set everything up.
#
#   git clone https://github.com/blussyya/83sc-control.git
#   cd 83sc-control && ./setup.sh
#
# Run it as yourself - it re-invokes itself with sudo for the privileged parts
# and drops back to your session for the user service. Safe to re-run; every
# step is idempotent. Pass --remove to undo.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOLD=$'\e[1m'; DIM=$'\e[2m'; RED=$'\e[31m'; GRN=$'\e[32m'; YEL=$'\e[33m'; RST=$'\e[0m'
step() { echo; echo "${BOLD}==> $*${RST}"; }
ok()   { echo "  ${GRN}ok${RST}  $*"; }
warn() { echo "  ${YEL}!!${RST}  $*" >&2; }
err()  { echo "  ${RED}xx${RST}  $*" >&2; }
have() { command -v "$1" >/dev/null 2>&1; }

# ---------------------------------------------------------------- privileges
if [[ ${EUID} -ne 0 ]]; then
    have sudo || { err "need root and sudo is not installed"; exit 1; }
    echo "${DIM}re-running with sudo (you stay the target user)...${RST}"
    exec sudo -E "$0" "$@"
fi
TARGET_USER="${SUDO_USER:-}"
if [[ -z $TARGET_USER || $TARGET_USER == root ]]; then
    err "run as your normal user (./setup.sh), not from a root shell"
    exit 1
fi
UID_T=$(id -u "$TARGET_USER")
as_user() {
    sudo -u "$TARGET_USER" \
        XDG_RUNTIME_DIR="/run/user/$UID_T" \
        DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$UID_T/bus" "$@"
}

# ---------------------------------------------------------------- uninstall
if [[ "${1:-}" == "--remove" ]]; then
    step "removing"
    as_user systemctl --user disable --now 83sc-kbd-idle 2>/dev/null || true
    systemctl disable --now 83sc-thermal.service 83sc-driver-guard.service 2>/dev/null || true
    rm -f /etc/systemd/system/83sc-thermal.service /etc/systemd/system/83sc-driver-guard.service
    rm -f /etc/pacman.d/hooks/83sc-driver.hook
    rm -f /usr/local/bin/83sc /usr/local/bin/83sc-diag /usr/local/bin/83sc-fan \
          /usr/local/bin/83sc-snap /usr/local/bin/legion83-gui
    rm -f /usr/share/applications/83sc-control.desktop
    rm -rf /usr/local/lib/83sc-control
    rm -f /etc/sudoers.d/83sc-control
    rm -f "/home/$TARGET_USER/.local/bin/83sc-kbd-idle" \
          "/home/$TARGET_USER/.config/systemd/user/83sc-kbd-idle.service"
    [[ -x "$HERE/install-dkms.sh" ]] && "$HERE/install-dkms.sh" --remove 2>/dev/null || true
    systemctl daemon-reload
    echo; ok "removed (kept /etc/83sc-control so your profiles survive)"
    exit 0
fi

echo "${BOLD}83sc-control setup${RST}  ${DIM}(user: $TARGET_USER)${RST}"

# ---------------------------------------------------------------- deps
step "checking dependencies"
MISS=()
have dkms  || MISS+=(dkms)
have cargo || MISS+=(cargo)
have gcc || have clang || MISS+=("gcc or clang")
[[ -d /lib/modules/$(uname -r)/build ]] || MISS+=("kernel headers for $(uname -r)")
have intel-undervolt || warn "intel-undervolt absent - undervolt won't replay at boot (optional)"
if ((${#MISS[@]})); then
    warn "missing: ${MISS[*]}"
    if   have pacman; then echo "     sudo pacman -S --needed dkms rust base-devel linux-headers intel-undervolt"
    elif have apt;    then echo "     sudo apt install dkms build-essential linux-headers-\$(uname -r) cargo intel-undervolt"
    elif have dnf;    then echo "     sudo dnf install dkms kernel-devel gcc cargo"
    fi
    echo
    read -r -p "  continue anyway, skipping what needs them? [y/N] " a
    [[ ${a,,} == y ]] || exit 1
else
    ok "all present"
fi

RC=0
run() { "$@" >/dev/null 2>&1 && return 0; RC=1; return 1; }

# ---------------------------------------------------------------- 1 helper
step "1/6  privileged helper + sudoers rule"
if run "$HERE/install.sh" --force; then ok "helper installed"; else err "install.sh failed"; fi

# ---------------------------------------------------------------- 2 cli
step "2/6  CLI tools"
if run "$HERE/install-cli.sh"; then ok "83sc, 83sc-diag, 83sc-fan, 83sc-snap"; else err "install-cli.sh failed"; fi

# ---------------------------------------------------------------- 3 driver
step "3/6  patched legion_laptop (DKMS)"
if ! have dkms || [[ ! -d /lib/modules/$(uname -r)/build ]]; then
    warn "skipped - needs dkms + kernel headers"
elif run "$HERE/install-dkms.sh"; then
    ok "patched module installed"
else
    warn "install-dkms.sh failed - if your distro already ships legion-laptop >= v0.0.26"
    warn "  it already has these fixes and you don't need the override"
fi

# ---------------------------------------------------------------- 4 services
step "4/6  boot services (power limits, undervolt, fan curve + driver guard)"
if run "$HERE/install-thermal.sh"; then ok "83sc-thermal + 83sc-driver-guard enabled"; else err "install-thermal.sh failed"; fi

# ---------------------------------------------------------------- 5 gui
step "5/6  GUI"
if have cargo; then
    echo "  ${DIM}building (first build compiles skia, this takes a while)...${RST}"
    if as_user cargo build --release --manifest-path "$HERE/gui/Cargo.toml" >/dev/null 2>&1 \
       && run "$HERE/gui/install-gui.sh"; then
        ok "83SC Control in your app menu"
    else
        err "GUI build/install failed"
    fi
else
    warn "skipped - cargo not installed"
fi

# ---------------------------------------------------------------- 6 dimmer
step "6/6  keyboard idle dimmer"
if have cargo; then
    if as_user cargo build --release --manifest-path "$HERE/kbd-idle/Cargo.toml" >/dev/null 2>&1 \
       && as_user "$HERE/kbd-idle/install-kbd-idle.sh" >/dev/null 2>&1; then
        ok "83sc-kbd-idle running (backlight off after 5s idle)"
    else
        err "kbd-idle build/install failed"
    fi
else
    warn "skipped - cargo not installed"
fi

# ---------------------------------------------------------------- verify
step "verification"
H=""
for h in /sys/class/hwmon/hwmon*; do
    [[ "$(cat "$h/name" 2>/dev/null)" == legion_hwmon ]] && H=$h && break
done
if [[ -n $H ]]; then
    fm=$(cat "$H/fan1_max" 2>/dev/null)
    if [[ $fm == 5400 && ! -e $H/fan2_input ]]; then
        ok "driver: patched (fan1_max=5400, phantom fan hidden)"
    else
        err "driver: STOCK module active (fan1_max=$fm) - fan curve will not work"
        err "  repair: sudo $HERE/systemd/83sc-driver-guard.sh"
    fi
else
    warn "legion_hwmon not present - is legion_laptop loaded?"
fi
[[ -r /etc/83sc-control/boot.conf ]] \
    && ok "boot.conf present - settings replay at boot" \
    || warn "no /etc/83sc-control/boot.conf yet - apply settings in the GUI, then: sudo 83sc boot-save"
systemctl is-enabled 83sc-thermal >/dev/null 2>&1 && ok "83sc-thermal enabled" || warn "83sc-thermal not enabled"
as_user systemctl --user is-active 83sc-kbd-idle >/dev/null 2>&1 && ok "idle dimmer running" || warn "idle dimmer not running"

echo
if ((RC)); then
    echo "${YEL}finished with warnings - see above.${RST}"
else
    echo "${GRN}${BOLD}done.${RST}  Launch ${BOLD}83SC Control${RST} from your app menu, or run ${BOLD}83sc-fan show${RST}."
fi
echo "${DIM}undo everything: ./setup.sh --remove${RST}"
exit $RC
