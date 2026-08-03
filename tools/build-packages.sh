#!/usr/bin/env bash
#
# Build native (non-Flatpak) packages for Fedora and Arch.
#
#   tools/build-packages.sh            # builds both
#   tools/build-packages.sh rpm        # Fedora RPM only
#   tools/build-packages.sh arch       # Arch package only
#
# Output lands in packaging/out/:
#   veem-<ver>-1.fc44.x86_64.rpm       - packaged from the host (Fedora) cargo build
#   veem-<ver>-1-x86_64.pkg.tar.zst    - built from source in an Arch container (podman)
#
# The RPM wraps the host-built release binary; the Arch package compiles from a
# `git archive HEAD` tarball inside an Arch container so it links Arch's libs —
# run this from a committed tree (a release tag) or the Arch package won't
# include uncommitted changes.
set -euo pipefail

APP_ID="com.getveem.Veem"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/packaging/out"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
WHAT="${1:-all}"

[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
mkdir -p "$OUT"

# Keep the version in the packaging files in lockstep with Cargo.toml
sed -i "s/^Version:.*/Version:        $VERSION/" "$ROOT/packaging/fedora/veem.spec"
sed -i "s/^pkgver=.*/pkgver=$VERSION/" "$ROOT/packaging/arch/PKGBUILD"

build_rpm() {
    echo "==> Building release binary (host)"
    ( cd "$ROOT" && cargo build --release )

    echo "==> Staging RPM payload"
    local work stage
    work="$(mktemp -d)"
    stage="$work/veem-$VERSION-bin"
    mkdir -p "$stage/icons/256x256" "$stage/icons/512x512"
    cp "$ROOT/target/release/veem"              "$stage/veem"
    cp "$ROOT/LICENSE"                          "$stage/LICENSE"
    cp "$ROOT/data/$APP_ID.desktop"             "$stage/$APP_ID.desktop"
    cp "$ROOT/data/$APP_ID.metainfo.xml"        "$stage/$APP_ID.metainfo.xml"
    for size in 256x256 512x512; do
        cp "$ROOT/data/icons/hicolor/$size/apps/$APP_ID.png" "$stage/icons/$size/$APP_ID.png"
    done
    mkdir -p "$work/rpmbuild/SOURCES"
    tar -C "$work" -cf "$work/rpmbuild/SOURCES/veem-$VERSION-bin.tar" "veem-$VERSION-bin"

    echo "==> rpmbuild"
    rpmbuild -bb --define "_topdir $work/rpmbuild" "$ROOT/packaging/fedora/veem.spec"
    cp "$work/rpmbuild/RPMS/x86_64/"veem-*.rpm "$OUT/"
    rm -rf "$work"
    echo "==> RPM done"
}

build_arch() {
    local image=localhost/veem-arch-builder
    if ! podman image exists "$image"; then
        echo "==> Building Arch builder image (one-time; refresh with podman build --no-cache)"
        podman build -t veem-arch-builder -f "$ROOT/packaging/arch/Containerfile" "$ROOT/packaging/arch"
    fi

    echo "==> Staging Arch source build"
    local work
    work="$(mktemp -d)"
    cp "$ROOT/packaging/arch/PKGBUILD" "$work/"
    ( cd "$ROOT" && git archive --format=tar.gz --prefix="veem-$VERSION/" \
        -o "$work/veem-$VERSION.tar.gz" HEAD )

    echo "==> makepkg in Arch container (compiles from source; takes a while)"
    podman run --rm -v "$work:/build:Z" "$image" bash -c '
        set -euo pipefail
        chown -R builder /build
        cd /build
        runuser -u builder -- makepkg -f --nocheck --skipchecksums
        # Rootless podman: give the artifacts back to the host user
        chown -R 0:0 /build
    '
    cp "$work/"veem-*.pkg.tar.zst "$OUT/"
    rm -rf "$work"
    echo "==> Arch package done"
}

case "$WHAT" in
    rpm)  build_rpm ;;
    arch) build_arch ;;
    all)  build_rpm; build_arch ;;
    *)    echo "usage: $0 [rpm|arch|all]" >&2; exit 1 ;;
esac

echo "==> Packages in $OUT:"
ls -1 "$OUT"
