#!/bin/bash
#
# Unpack base/SDK snaps into /snap/<name>/current so snapcraft's extensions
# work in a container without snapd (the standard snapcraft-in-docker recipe).
# Downloads come straight from the store API and are cached in $SDK_CACHE.
set -euo pipefail

CACHE="${SDK_CACHE:-/cache}"
mkdir -p "$CACHE"

for name in "$@"; do
    [ -d "/snap/$name/current" ] && continue
    f="$CACHE/$name.snap"
    if [ ! -f "$f" ]; then
        echo "==> Downloading $name from the snap store"
        url=$(curl -s -H "Snap-Device-Series: 16" \
            "https://api.snapcraft.io/v2/snaps/info/$name?architecture=amd64&fields=download" \
            | python3 -c 'import sys,json;d=json.load(sys.stdin);print(next(c["download"]["url"] for c in d["channel-map"] if c["channel"]["risk"]=="stable" and c["channel"]["architecture"]=="amd64"))')
        curl -sL -o "$f.part" "$url" && mv "$f.part" "$f"
    fi
    echo "==> Unpacking $name to /snap/$name/current"
    mkdir -p "/snap/$name"
    unsquashfs -q -f -d "/snap/$name/current" "$f" >/dev/null
done
