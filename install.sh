#!/usr/bin/env bash
#
# 83sc-control : one-time privileged install
#
# Run this once, as root:   sudo ./install.sh
#
# It does four things and nothing else:
#   1. installs the analysis tooling packages
#   2. copies helper.py to root-owned /usr/local/lib/83sc-control/
#   3. installs a sudoers rule scoped to exactly that one file
#   4. verifies the result is actually safe
#
# It will refuse to overwrite an existing install unless you pass --force.

set -euo pipefail

LIBDIR=/usr/local/lib/83sc-control
HELPER="$LIBDIR/helper.py"
SUDOERS=/etc/sudoers.d/83sc-control
STATEDIR=/var/lib/83sc-control
SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FORCE=0
[[ "${1:-}" == "--force" ]] && FORCE=1

die() { echo "install: $*" >&2; exit 1; }
info() { echo "  -> $*"; }

[[ $EUID -eq 0 ]] || die "must run as root:  sudo ./install.sh"

# The user who will be granted the sudoers rule. SUDO_USER is who invoked us.
TARGET_USER="${SUDO_USER:-}"
[[ -n "$TARGET_USER" && "$TARGET_USER" != "root" ]] \
  || die "could not determine the invoking user; run via 'sudo ./install.sh', not from a root shell"

echo
echo "83sc-control installer"
echo "  source:      $SRC"
echo "  helper ->    $HELPER"
echo "  sudoers ->   $SUDOERS  (user: $TARGET_USER)"
echo

# --- 1. packages -----------------------------------------------------------
echo "[1/4] analysis tooling"
PKGS=()
command -v acpidump >/dev/null || PKGS+=(acpica)
command -v rdmsr    >/dev/null || PKGS+=(msr-tools)
command -v turbostat>/dev/null || PKGS+=(turbostat)
command -v flashrom >/dev/null || PKGS+=(flashrom)
if (( ${#PKGS[@]} )); then
  info "installing: ${PKGS[*]}"
  pacman -S --needed --noconfirm "${PKGS[@]}"
else
  info "all present already"
fi

# --- 2. helper -------------------------------------------------------------
echo "[2/4] privileged helper"
[[ -f "$SRC/helper/helper.py" ]] || die "helper/helper.py not found next to install.sh"

if [[ -e "$HELPER" && $FORCE -eq 0 ]]; then
  if cmp -s "$SRC/helper/helper.py" "$HELPER"; then
    info "already installed and identical"
  else
    die "$HELPER exists and differs. Review the diff, then re-run with --force:
     diff $HELPER $SRC/helper/helper.py"
  fi
else
  install -d -o root -g root -m 0755 "$LIBDIR"
  install -o root -g root -m 0755 "$SRC/helper/helper.py" "$HELPER"
  info "installed"
fi
install -d -o root -g root -m 0755 "$STATEDIR"

# The whole security model rests on this file not being user-writable, so
# verify rather than assume -- a copy left on the exfat work volume would be
# writable by the login user and turn the sudoers rule into a root hole.
FS=$(stat -f -c %T "$HELPER")
OWNER=$(stat -c '%U:%G %a' "$HELPER")
[[ "$OWNER" == "root:root 755" ]] || die "helper has wrong ownership/mode: $OWNER"
case "$FS" in
  msdos|exfat|vfat|fuseblk) die "helper landed on a $FS filesystem which cannot enforce ownership. Aborting." ;;
esac
info "verified root:root 0755 on $FS"

# --- 3. sudoers ------------------------------------------------------------
echo "[3/4] sudoers rule"
if [[ -e "$SUDOERS" && $FORCE -eq 0 ]]; then
  info "already present (use --force to rewrite)"
else
  TMP=$(mktemp)
  cat > "$TMP" <<EOF
# 83sc-control : allow $TARGET_USER to run the hardware-control helper as root.
# Scoped to one root-owned file. That helper validates every argument against
# an internal whitelist, so this grant is narrower than it looks -- it is not
# equivalent to NOPASSWD:ALL.
$TARGET_USER ALL=(root) NOPASSWD: $HELPER
EOF
  visudo -cqf "$TMP" || { rm -f "$TMP"; die "generated sudoers rule failed validation; nothing installed"; }
  install -o root -g root -m 0440 "$TMP" "$SUDOERS"
  rm -f "$TMP"
  info "installed and validated"
fi

# --- 4. verify end to end --------------------------------------------------
echo "[4/4] verification"
if sudo -u "$TARGET_USER" sudo -n "$HELPER" status >/dev/null 2>&1; then
  info "$TARGET_USER can invoke the helper without a password"
else
  die "verification failed: $TARGET_USER still cannot run the helper"
fi

echo
echo "Done. From the work directory you can now run:  ./bin/83sc status"
echo
