#!/usr/bin/env bash
# Fetches a GEBCO patch of a region and writes its realism reference to
# crates/vale-terrain/data/gebco/REGION.txt. The grid stays in target/gebco.
#
#   scripts/gebco-reference.sh pacific-flank
set -euo pipefail

cd "$(dirname "$0")/.."

region=${1:-pacific-flank}

# The profiles across the Manihiki Scarp, from the grid of the Manihiki patch.
# The profiles run east from 163 W to 159 W, 12 of them from 10.5 S to 13.25 S.
# North of 10.5 S, seamounts and troughs break up the scarp.
if [ "$region" = manihiki-scarp ]; then
	grid=target/gebco/manihiki.asc
	[ -f "$grid" ] || "$0" manihiki
	out=crates/vale-terrain/data/gebco/manihiki-scarp.txt
	values=$(cargo run --release -q -p vale-terrain --example gebco_profiles -- \
		"$grid" -13.25 -10.5 12 -163 -159)
	{
		echo "# The profiles across the Manihiki Scarp, 10.5 S to 13.25 S."
		echo "# Data: GEBCO Compilation Group (2020) GEBCO 2020 Grid, doi:10.5285/a29c5465-b138-234d-e053-6c86abc040b9"
		echo "# Source: https://coastwatch.pfeg.noaa.gov/erddap/griddap/GEBCO_2020"
		echo "# Each profile runs east along one latitude, from 163 W to 159 W."
		echo "# profile = drop_m, width_km, trace_km, then the shape."
		echo "# Made by: scripts/gebco-reference.sh $region"
		echo
		echo "$values"
	} >"$out"
	echo "wrote $out" >&2
	exit 0
fi

case "$region" in
pacific-flank)
	title="East Pacific Rise west flank, 11.6 N"
	lon=-105.35
	lat=11.6
	size=512
	cell_m=460
	band_max_m=60000
	# The trend of the abyssal hills, N10W. It is the azimuth of the strongest
	# anisotropy of the patch, and the rise axis east of the patch has the same trend.
	fabric_deg=-10
	;;
atlantic-ridge)
	title="Mid-Atlantic Ridge, 26.5 N"
	lon=-45.0
	lat=26.5
	size=1024
	cell_m=920
	band_max_m=225000
	# The trend of the segments of the axis and of the abyssal hills, N15E.
	# The stepped axis as a whole trends more to the east.
	fabric_deg=15
	;;
epr-ridge)
	title="East Pacific Rise, 11 N"
	lon=-104.1
	lat=11.0
	size=1024
	cell_m=920
	band_max_m=225000
	# The trend of the axis, N8W, square to the Clipperton and the Siqueiros
	# transform faults.
	fabric_deg=-8
	;;
us-east-margin)
	title="Eastern United States margin, 38.3 N"
	lon=-72.4
	lat=38.3
	size=512
	cell_m=920
	# The band stops at 40 km, so the curve of the shelf break does not
	# fill the spectrum.
	band_max_m=40000
	# The trend of the shelf break off New Jersey and the Delmarva
	# Peninsula, N40E.
	fabric_deg=40
	;;
hawaii)
	title="Hawaiian Islands, 20.6 N"
	lon=-157.3
	lat=20.6
	size=1024
	cell_m=920
	band_max_m=225000
	# The trend of the chain from Hawaii to Kauai, N60W.
	fabric_deg=-60
	;;
manihiki)
	title="Manihiki Plateau, 8.8 S"
	lon=-163.0
	lat=-8.8
	size=1024
	cell_m=1380
	band_max_m=225000
	# The trend of the Manihiki Scarp on the east side and of the Danger
	# Islands Troughs on the west side, N5W.
	fabric_deg=-5
	;;
*)
	echo "unknown region: $region" >&2
	exit 2
	;;
esac

dataset=GEBCO_2020
server=https://coastwatch.pfeg.noaa.gov/erddap/griddap
dir=target/gebco
mkdir -p "$dir"

# The bounds of the patch, with a margin of 0.05 degrees.
read -r south north west east < <(awk -v lon="$lon" -v lat="$lat" -v n="$size" -v c="$cell_m" 'BEGIN {
	half = n * c / 2 / 6371000 * 180 / 3.141592653589793 * 1.02 + 0.05
	k = cos(((lat < 0 ? -lat : lat) + half) * 3.141592653589793 / 180)
	printf "%.3f %.3f %.3f %.3f\n", lat - half, lat + half, lon - half / k, lon + half / k
}')

grid="$dir/$region.asc"
url="$server/$dataset.esriAscii?elevation%5B($south):1:($north)%5D%5B($west):1:($east)%5D"
echo "fetching $url" >&2
curl -sSf -m 600 -o "$grid" "$url"

out=crates/vale-terrain/data/gebco/$region.txt
mkdir -p "$(dirname "$out")"
values=$(cargo run --release -q -p vale-terrain --example gebco_reference -- \
	"$grid" "$dir" "$region" "$title" "$lon,$lat" "$size" "$cell_m" "$band_max_m" "$fabric_deg")
{
	echo "# The realism reference of a GEBCO patch."
	echo "# Data: GEBCO Compilation Group (2020) GEBCO 2020 Grid, doi:10.5285/a29c5465-b138-234d-e053-6c86abc040b9"
	echo "# Source: $server/$dataset"
	echo "# Bounds of the fetch: $south to $north N, $west to $east E"
	echo "# Fetched: $(date -u +%Y-%m-%d)"
	echo "# Made by: scripts/gebco-reference.sh $region"
	echo
	echo "$values"
} >"$out"
echo "wrote $out" >&2
