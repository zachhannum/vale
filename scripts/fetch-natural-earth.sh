#!/bin/sh
# Fetch the Natural Earth GeoJSON files of one scale (110m or 50m) and verify SHA-256.
set -eu

BASE="https://raw.githubusercontent.com/nvkelso/natural-earth-vector/v5.1.2/geojson"

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <110m|50m>" >&2
    exit 2
fi
scale="$1"

case "$scale" in
    110m)
        sums="ne_110m_populated_places_simple 0dbd25c9ad8bd797ddf164b067f563be5c16be2c002254eb594862377963f9dc
ne_110m_rivers_lake_centerlines 55aa4497405afc07cdc931b7fbe062c4d6693ba2a550c0d24899953f5d507c8d
ne_110m_land 9e0729ee253ca7d7a5c4ae9395fb1902264c5377c52e224d13dd85010e2835d9
ne_110m_admin_0_countries 6866c877d39cba9c357620878839b336d569f8c662d3cfab4cb1dbe2d39c977f
ne_110m_geography_marine_polys b9c3f7f557d0ff5217906adc82b66ecdac14aa7438df7e518cf6675d037bceb8"
        ;;
    50m)
        sums="ne_50m_populated_places_simple 8e70756b39fae9bcdc1e332bfc510c024c5edd3a13203ffd20092ee37b61d978
ne_50m_rivers_lake_centerlines f286e0ce978fde999ca2d7a78c764be08542e19b63cded52b05c12d5173ccc51
ne_50m_land e874b27a51d146452be360cafb3cc50c86001074a67d534113e6534682f9826b
ne_50m_admin_0_countries 3e458fc036ad0a66411f2c1e6cac49c5d7bfb81cb1123bc513b22511a2b7fdeb
ne_50m_geography_marine_polys 6fe58083e0cc5c7fad9e396970e28a8580bbd8770cfa4d1d7b5a34423e912f97"
        ;;
    *)
        echo "unknown scale: $scale (expected 110m or 50m)" >&2
        exit 2
        ;;
esac

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dir="$root/data/natural-earth/$scale"
mkdir -p "$dir"

sha256() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        sha256sum "$1" | cut -d ' ' -f 1
    fi
}

status=0
while read -r name want; do
    file="$dir/$name.geojson"
    if [ -f "$file" ] && [ "$(sha256 "$file")" = "$want" ]; then
        echo "ok (present): $name"
        continue
    fi
    echo "fetching: $name"
    curl -fsSL "$BASE/$name.geojson" -o "$file"
    got=$(sha256 "$file")
    if [ "$got" != "$want" ]; then
        echo "checksum mismatch for $name: expected $want, got $got" >&2
        rm -f "$file"
        status=1
    fi
done <<EOT
$sums
EOT
exit $status
