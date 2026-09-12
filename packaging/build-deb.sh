#!/usr/bin/env bash
# Build a .deb. Works on any distro that has dpkg-deb + cargo.
#   ./packaging/build-deb.sh   ->  dist/83sc-control_1.0.0_amd64.deb
set -euo pipefail
VER=1.0.0
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/dist"; STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
chmod 755 "$STAGE"

command -v dpkg-deb >/dev/null || { echo "need dpkg-deb" >&2; exit 1; }
command -v cargo    >/dev/null || { echo "need cargo" >&2; exit 1; }

cargo build --release --manifest-path "$ROOT/gui/Cargo.toml"
cargo build --release --manifest-path "$ROOT/kbd-idle/Cargo.toml"

install -Dm755 "$ROOT/gui/target/release/legion83-gui"      "$STAGE/usr/bin/legion83-gui"
install -Dm755 "$ROOT/kbd-idle/target/release/83sc-kbd-idle" "$STAGE/usr/bin/83sc-kbd-idle"
for t in 83sc 83sc-diag 83sc-fan 83sc-snap; do
    install -Dm755 "$ROOT/bin/$t" "$STAGE/usr/bin/$t"
done
install -Dm755 "$ROOT/helper/helper.py"             "$STAGE/usr/lib/83sc-control/helper.py"
install -Dm755 "$ROOT/systemd/83sc-thermal.sh"      "$STAGE/usr/lib/83sc-control/83sc-thermal.sh"
install -Dm755 "$ROOT/systemd/83sc-driver-guard.sh" "$STAGE/usr/lib/83sc-control/83sc-driver-guard.sh"
for u in 83sc-thermal 83sc-driver-guard; do
    sed 's#/usr/local/lib/#/usr/lib/#' "$ROOT/systemd/$u.service" \
        > "$STAGE/tmp.service"
    install -Dm644 "$STAGE/tmp.service" "$STAGE/lib/systemd/system/$u.service"
done
sed 's#%h/.local/bin/#/usr/bin/#' "$ROOT/systemd/83sc-kbd-idle.service" > "$STAGE/tmp.service"
install -Dm644 "$STAGE/tmp.service" "$STAGE/lib/systemd/user/83sc-kbd-idle.service"
rm -f "$STAGE/tmp.service"
install -Dm644 "$ROOT/gui/83sc-control.desktop" "$STAGE/usr/share/applications/83sc-control.desktop"
sed 's/%ADMINGROUP%/%sudo/' "$ROOT/packaging/83sc-control.sudoers" > "$STAGE/sudoers"
install -Dm440 "$STAGE/sudoers" "$STAGE/etc/sudoers.d/83sc-control"; rm -f "$STAGE/sudoers"
install -Dm644 "$ROOT/README.md" "$STAGE/usr/share/doc/83sc-control/README.md"

mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CTRL
Package: 83sc-control
Version: $VER
Section: utils
Priority: optional
Architecture: amd64
Depends: python3, systemd, libc6
Recommends: intel-undervolt
Maintainer: blussyya <https://github.com/blussyya>
Description: Thermal, power and fan control for Lenovo LOQ Essential 15IRX11
 Power limits, fan curve, undervolt and keyboard controls for the Lenovo LOQ
 Essential 15IRX11 (DMI 83SC), with a GUI and CLI tools. Settings are replayed
 at boot. Fan control requires legion_laptop with the 83SC fixes
 (LenovoLegionLinux >= v0.0.26).
CTRL
cat > "$STAGE/DEBIAN/postinst" <<'POST'
#!/bin/sh
set -e
systemctl daemon-reload || true
echo ""
echo "  83sc-control installed."
echo "    sudo systemctl enable --now 83sc-driver-guard 83sc-thermal"
echo "    systemctl --user enable --now 83sc-kbd-idle"
echo "  Then apply settings in '83SC Control' and run: sudo 83sc boot-save"
echo ""
POST
cat > "$STAGE/DEBIAN/prerm" <<'PRE'
#!/bin/sh
set -e
systemctl disable --now 83sc-thermal 83sc-driver-guard 2>/dev/null || true
PRE
chmod 755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/prerm"

mkdir -p "$OUT"
dpkg-deb --build --root-owner-group "$STAGE" "$OUT/83sc-control_${VER}_amd64.deb"
echo "-> $OUT/83sc-control_${VER}_amd64.deb"
