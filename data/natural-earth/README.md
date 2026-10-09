# Natural Earth data

Source: https://github.com/nvkelso/natural-earth-vector, tag `v5.1.2` (GeoJSON files).

Made with Natural Earth. Free vector and raster map data @ naturalearthdata.com.
Natural Earth is in the public domain.

The 110m files are checked in. To fetch the larger 50m set (ignored by git):

    sh scripts/fetch-natural-earth.sh 50m

Each file is verified against a pinned SHA-256.
