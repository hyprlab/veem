#!/bin/bash
#
# Unpack base/SDK snaps into /snap/<name>/current so snapcraft's extensions
# work in a container without snapd (the standard snapcraft-in-docker recipe).
# Downloads come straight from the store API (python3 stdlib — the snapcraft
# container has no curl) and are cached in $SDK_CACHE.
set -euo pipefail

CACHE="${SDK_CACHE:-/cache}"
mkdir -p "$CACHE"

for name in "$@"; do
    [ -d "/snap/$name/current" ] && continue
    f="$CACHE/$name.snap"
    if [ ! -f "$f" ]; then
        echo "==> Downloading $name from the snap store"
        python3 - "$name" "$f" <<'EOF'
import json, sys, urllib.request

name, dest = sys.argv[1], sys.argv[2]
req = urllib.request.Request(
    f"https://api.snapcraft.io/v2/snaps/info/{name}?architecture=amd64&fields=download",
    headers={"Snap-Device-Series": "16"},
)
with urllib.request.urlopen(req) as r:
    info = json.load(r)
url = next(
    c["download"]["url"]
    for c in info["channel-map"]
    if c["channel"]["risk"] == "stable" and c["channel"]["architecture"] == "amd64"
)
urllib.request.urlretrieve(url, dest + ".part")
EOF
        mv "$f.part" "$f"
    fi
    echo "==> Unpacking $name to /snap/$name/current"
    mkdir -p "/snap/$name"
    unsquashfs -q -f -d "/snap/$name/current" "$f" >/dev/null
done

# The gnome extension's command-chain part sources
# /usr/share/snapcraft/extensions/desktop — present in the snapcraft *snap*
# but empty in the container image, so fetch it from the snapcraft repo at
# the container's own snapcraft version (fall back to main).
if [ ! -d /usr/share/snapcraft/extensions/desktop ]; then
    ver=$(snapcraft --version | awk '{print $2}')
    echo "==> Fetching extensions/desktop from snapcraft $ver sources"
    python3 - "$ver" <<'EOF'
import io, shutil, sys, tarfile, urllib.request

ver = sys.argv[1]
urls = [
    f"https://github.com/canonical/snapcraft/archive/refs/tags/{ver}.tar.gz",
    "https://github.com/canonical/snapcraft/archive/refs/heads/main.tar.gz",
]
data = None
for url in urls:
    try:
        data = urllib.request.urlopen(url).read()
        break
    except Exception as e:
        print(f"  ({url}: {e})")
if data is None:
    sys.exit("could not download snapcraft sources")

tf = tarfile.open(fileobj=io.BytesIO(data))
members = [m for m in tf.getmembers() if "/extensions/desktop/" in m.name]
if not members:
    sys.exit("no extensions/desktop in snapcraft tarball")
root = members[0].name.split("/")[0]
tf.extractall("/tmp/snapcraft-src", members=members)
shutil.copytree(
    f"/tmp/snapcraft-src/{root}/extensions/desktop",
    "/usr/share/snapcraft/extensions/desktop",
)
EOF
fi
