#!/usr/bin/env bash
# Build every package format this machine has tooling for.
#   ./packaging/build-all.sh   ->  dist/
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$ROOT/dist"
built=0

if command -v dpkg-deb >/dev/null && command -v cargo >/dev/null; then
    echo "==> .deb"
    "$ROOT/packaging/build-deb.sh" && built=$((built+1))
else
    echo "-- skipping .deb (needs dpkg-deb + cargo)"
fi

if command -v makepkg >/dev/null; then
    echo "==> Arch package"
    ( cd "$ROOT/packaging" && makepkg -f --nodeps --noconfirm ) \
        && mv "$ROOT"/packaging/*.pkg.tar.zst "$ROOT/dist/" 2>/dev/null \
        && built=$((built+1))
else
    echo "-- skipping Arch package (needs makepkg)"
fi

if command -v rpmbuild >/dev/null; then
    echo "==> .rpm"
    TAR="$ROOT/dist/83sc-control-1.0.0.tar.gz"
    ( cd "$ROOT/.." && tar czf "$TAR" --transform 's,^[^/]*,83sc-control-1.0.0,' \
        "$(basename "$ROOT")" ) 2>/dev/null
    rpmbuild -ta "$TAR" && built=$((built+1))
else
    echo "-- skipping .rpm (needs rpmbuild; the spec is at packaging/83sc-control.spec)"
fi

echo
echo "built $built package(s):"
ls -1 "$ROOT/dist" 2>/dev/null || echo "  (none)"
