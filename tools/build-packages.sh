#!/usr/bin/env bash
#
# Build the native (non-Flatpak) package for Fedora.
#
#   tools/build-packages.sh            # builds the RPM
#   tools/build-packages.sh rpm        # same
#
# Output lands in packaging/out/:
#   vireo-<ver>-1.fc44.x86_64.rpm       - packaged from the host (Fedora) cargo build
#
# The RPM wraps the host-built release binary. Arch, Debian/Ubuntu and Snap
# packages were discontinued after 1.7.0: every other distribution is served by
# the Flatpak, which needs no per-distro build machinery.
set -euo pipefail

APP_ID="co.hyprlab.Vireo"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/packaging/out"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
WHAT="${1:-all}"

[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
mkdir -p "$OUT"

# Keep the version in the spec file in lockstep with Cargo.toml
sed -i "s/^Version:.*/Version:        $VERSION/" "$ROOT/packaging/fedora/vireo.spec"

build_rpm() {
    echo "==> Building release binary (host)"
    ( cd "$ROOT" && cargo build --release )

    echo "==> Staging RPM payload"
    local work stage
    work="$(mktemp -d)"
    stage="$work/vireo-$VERSION-bin"
    mkdir -p "$stage/icons/256x256" "$stage/icons/512x512"
    cp "$ROOT/target/release/vireo"              "$stage/vireo"
    cp "$ROOT/LICENSE"                          "$stage/LICENSE"
    cp "$ROOT/data/$APP_ID.desktop"             "$stage/$APP_ID.desktop"
    cp "$ROOT/data/$APP_ID.metainfo.xml"        "$stage/$APP_ID.metainfo.xml"
    for size in 256x256 512x512; do
        cp "$ROOT/data/icons/hicolor/$size/apps/$APP_ID.png" "$stage/icons/$size/$APP_ID.png"
    done
    mkdir -p "$work/rpmbuild/SOURCES"
    tar -C "$work" -cf "$work/rpmbuild/SOURCES/vireo-$VERSION-bin.tar" "vireo-$VERSION-bin"

    echo "==> rpmbuild"
    rpmbuild -bb --define "_topdir $work/rpmbuild" "$ROOT/packaging/fedora/vireo.spec"
    cp "$work/rpmbuild/RPMS/x86_64/"vireo-*.rpm "$OUT/"
    rm -rf "$work"
    echo "==> RPM done"
}

case "$WHAT" in
    rpm|all) build_rpm ;;
    *)       echo "usage: $0 [rpm]" >&2; exit 1 ;;
esac

echo "==> Packages in $OUT:"
ls -1 "$OUT"
