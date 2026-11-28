//! The ESRI ASCII grid of the GEBCO examples.

/// The meters of one degree of latitude.
pub const M_PER_DEG: f64 = 6371.0 * 1000.0 * std::f64::consts::PI / 180.0;

pub struct Grid {
    pub columns: usize,
    pub rows: usize,
    /// The longitude and the latitude of the center of the south-west cell.
    pub west: f64,
    pub south: f64,
    pub cell_deg: f64,
    /// Row 0 is the north row.
    pub values: Vec<f64>,
}

impl Grid {
    pub fn read(text: &str) -> Grid {
        let mut words = text.split_whitespace();
        let mut header = std::collections::HashMap::new();
        for _ in 0..6 {
            let key = words.next().expect("header key").to_ascii_lowercase();
            let value: f64 = words.next().expect("header value").parse().expect("number");
            header.insert(key, value);
        }
        let get = |key: &str| *header.get(key).unwrap_or_else(|| panic!("no `{key}`"));
        let half = |corner: &str, center: &str| match header.get(center) {
            Some(&v) => v,
            None => get(corner) + get("cellsize") * 0.5,
        };
        let nodata = get("nodata_value");
        let values: Vec<f64> = words
            .map(|w| w.parse::<f64>().expect("number"))
            .inspect(|&v| assert!(v != nodata, "the grid has no-data cells"))
            .collect();
        let grid = Grid {
            columns: get("ncols") as usize,
            rows: get("nrows") as usize,
            west: half("xllcorner", "xllcenter"),
            south: half("yllcorner", "yllcenter"),
            cell_deg: get("cellsize"),
            values,
        };
        assert_eq!(grid.values.len(), grid.columns * grid.rows);
        grid
    }

    /// The bilinear elevation at a longitude and a latitude.
    pub fn at(&self, lon: f64, lat: f64) -> f64 {
        let x = (lon - self.west) / self.cell_deg;
        let y = (self.rows - 1) as f64 - (lat - self.south) / self.cell_deg;
        let inside = |v: f64, n: usize| v >= 0.0 && v <= (n - 1) as f64;
        assert!(
            inside(x, self.columns) && inside(y, self.rows),
            "the patch reaches past the grid at {lon}, {lat}"
        );
        let (x0, y0) = (
            (x.floor() as usize).min(self.columns - 2),
            (y.floor() as usize).min(self.rows - 2),
        );
        let (tx, ty) = (x - x0 as f64, y - y0 as f64);
        let v = |i: usize, j: usize| self.values[j * self.columns + i];
        let top = v(x0, y0) + (v(x0 + 1, y0) - v(x0, y0)) * tx;
        let bottom = v(x0, y0 + 1) + (v(x0 + 1, y0 + 1) - v(x0, y0 + 1)) * tx;
        top + (bottom - top) * ty
    }
}
