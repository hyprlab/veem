#!/usr/bin/env bash
#
# Build native (non-Flatpak) packages for Fedora, Arch, Debian/Ubuntu and Snap.
#
#   tools/build-packages.sh            # builds rpm + arch + deb + snap
#   tools/build-packages.sh rpm        # Fedora RPM only
#   tools/build-packages.sh arch       # Arch package only
#   tools/build-packages.sh deb        # Debian/Ubuntu package only
#   tools/build-packages.sh snap       # Snap package only
#
# Output lands in packaging/out/:
#   vireo-<ver>-1.fc44.x86_64.rpm       - packaged from the host (Fedora) cargo build
#   vireo-<ver>-1-x86_64.pkg.tar.zst    - built from source in an Arch container (podman)
#   vireo_<ver>-1_amd64.deb             - built from source in an Ubuntu 24.04 container
#   vireo_<ver>_amd64.snap              - snapcraft (destructive mode) in a container
#
# The RPM wraps the host-built release binary; the Arch package compiles from a
# `git archive HEAD` tarball inside an Arch container so it links Arch's libs —
# run this from a committed tree (a release tag) or the Arch package won't
# include uncommitted changes.
set -euo pipefail

APP_ID="co.hyprlab.Vireo"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/packaging/out"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
WHAT="${1:-all}"

[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
mkdir -p "$OUT"

# Keep the version in the packaging files in lockstep with Cargo.toml
sed -i "s/^Version:.*/Version:        $VERSION/" "$ROOT/packaging/fedora/vireo.spec"
sed -i "s/^pkgver=.*/pkgver=$VERSION/" "$ROOT/packaging/arch/PKGBUILD"
sed -i "s/^version: .*/version: '$VERSION'/" "$ROOT/packaging/snap/snapcraft.yaml"

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

build_arch() {
    local image=localhost/vireo-arch-builder
    if ! podman image exists "$image"; then
        echo "==> Building Arch builder image (one-time; refresh with podman build --no-cache)"
        podman build -t vireo-arch-builder -f "$ROOT/packaging/arch/Containerfile" "$ROOT/packaging/arch"
    fi

    echo "==> Staging Arch source build"
    local work
    work="$(mktemp -d)"
    cp "$ROOT/packaging/arch/PKGBUILD" "$work/"
    ( cd "$ROOT" && git archive --format=tar.gz --prefix="vireo-$VERSION/" \
        -o "$work/vireo-$VERSION.tar.gz" HEAD )

    echo "==> makepkg in Arch container (compiles from source; takes a while)"
    podman run --rm -v "$work:/build:Z" "$image" bash -c '
        set -euo pipefail
        chown -R builder /build
        cd /build
        runuser -u builder -- makepkg -f --nocheck --skipchecksums
        # Rootless podman: give the artifacts back to the host user
        chown -R 0:0 /build
    '
    cp "$work/"vireo-*.pkg.tar.zst "$OUT/"
    rm -rf "$work"
    echo "==> Arch package done"
}

build_deb() {
    local image=localhost/vireo-deb-builder
    if ! podman image exists "$image"; then
        echo "==> Building deb builder image (one-time; refresh with podman build --no-cache)"
        podman build -t vireo-deb-builder -f "$ROOT/packaging/debian/Containerfile" "$ROOT/packaging/debian"
    fi

    echo "==> Staging Debian source tree"
    local work
    work="$(mktemp -d)"
    ( cd "$ROOT" && git archive --format=tar --prefix="vireo-$VERSION/" HEAD | tar -x -C "$work" )
    cp -r "$ROOT/packaging/debian/debian" "$work/vireo-$VERSION/debian"
    printf 'vireo (%s-1) unstable; urgency=medium\n\n  * Release %s - https://github.com/hyprlab/vireo/releases/tag/v%s\n\n -- Hyprlab <hyprlab@proton.me>  %s\n' \
        "$VERSION" "$VERSION" "$VERSION" "$(date -R)" > "$work/vireo-$VERSION/debian/changelog"

    echo "==> dpkg-buildpackage in Ubuntu 24.04 container (compiles from source; takes a while)"
    podman run --rm -v "$work:/build:Z" "$image" bash -c "
        set -euo pipefail
        cd /build/vireo-$VERSION
        dpkg-buildpackage -b -us -uc
        chown -R 0:0 /build
    "
    cp "$work/vireo_${VERSION}-1_amd64.deb" "$OUT/"
    rm -rf "$work"
    echo "==> deb done"
}

build_snap() {
    local image=ghcr.io/canonical/snapcraft:8_core24
    local cache="$ROOT/packaging/out/.snap-sdk-cache"
    mkdir -p "$cache"

    echo "==> Staging snap build tree"
    local work
    work="$(mktemp -d)"
    ( cd "$ROOT" && git archive --format=tar --prefix=tree/ HEAD | tar -x -C "$work" )
    mkdir -p "$work/tree/snap"
    cp "$ROOT/packaging/snap/snapcraft.yaml" "$work/tree/snap/snapcraft.yaml"
    cp "$ROOT/packaging/snap/prepare-sdk.sh" "$work/prepare-sdk.sh"

    echo "==> snapcraft in container (destructive mode; compiles from source)"
    podman run --rm -v "$work:/build:Z" -v "$cache:/cache:Z" --entrypoint bash "$image" -c "
        set -euo pipefail
        bash /build/prepare-sdk.sh core24 gnome-46-2404-sdk
        cd /build/tree
        snapcraft pack --destructive-mode --output /build/vireo_${VERSION}_amd64.snap
        chown -R 0:0 /build
    "
    cp "$work/vireo_${VERSION}_amd64.snap" "$OUT/"
    rm -rf "$work"
    echo "==> snap done"
}

case "$WHAT" in
    rpm)  build_rpm ;;
    arch) build_arch ;;
    deb)  build_deb ;;
    snap) build_snap ;;
    all)  build_rpm; build_arch; build_deb; build_snap ;;
    *)    echo "usage: $0 [rpm|arch|deb|snap|all]" >&2; exit 1 ;;
esac

echo "==> Packages in $OUT:"
ls -1 "$OUT"
