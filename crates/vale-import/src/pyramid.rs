//! A tile pyramid of straight RGBA8 pixels, built from strips of rows.
//!
//! Level 0 is the full image. Level k is ceil(w / 2^k) by ceil(h / 2^k)
//! pixels. The last level is the first level whose width is 256 pixels or
//! less. A pixel of the next level is the mean of a 2×2 block, with
//! premultiplied colors. Each level keeps only one band of 256 rows.

/// The width and the height of a tile.
pub const TILE: u32 = 256;

/// Returns the number of levels for an image.
pub fn levels(width: u32, _height: u32) -> u32 {
    let mut w = width.max(1);
    let mut n = 1;
    while w > TILE {
        w = w.div_ceil(2);
        n += 1;
    }
    n
}

/// Returns the width and the height of a level.
pub fn level_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    let d = 1u64 << level.min(63);
    let size = |v: u32| (v as u64).div_ceil(d).max(1) as u32;
    (size(width), size(height))
}

struct Level {
    level: u32,
    width: u32,
    height: u32,
    /// The rows of the current band.
    band: Vec<u8>,
    band_rows: u32,
    /// The rows that came in, in all.
    rows: u32,
    /// The first row of a pair, until its second row comes in.
    carry: Vec<u8>,
    has_carry: bool,
    /// The last row of the next level.
    down: Vec<u8>,
    last: bool,
}

impl Level {
    fn new(level: u32, (width, height): (u32, u32), last: bool) -> Self {
        Level {
            level,
            width,
            height,
            band: Vec::with_capacity(width as usize * 4 * TILE.min(height) as usize),
            band_rows: 0,
            rows: 0,
            carry: Vec::new(),
            has_carry: false,
            down: Vec::new(),
            last,
        }
    }

    /// Adds a row. Returns true if a row of the next level is in `down`.
    fn push<F: FnMut(u32, u32, u32, u32, u32, &[u8])>(&mut self, row: &[u8], sink: &mut F) -> bool {
        if self.rows >= self.height {
            return false;
        }
        self.band.extend_from_slice(row);
        self.band_rows += 1;
        self.rows += 1;
        if self.band_rows == TILE || self.rows == self.height {
            self.flush(sink);
        }
        if self.last {
            return false;
        }
        if self.has_carry {
            self.has_carry = false;
            downsample(&self.carry, row, self.width, &mut self.down);
            true
        } else if self.rows == self.height {
            downsample(row, row, self.width, &mut self.down);
            true
        } else {
            self.carry.clear();
            self.carry.extend_from_slice(row);
            self.has_carry = true;
            false
        }
    }

    fn flush<F: FnMut(u32, u32, u32, u32, u32, &[u8])>(&mut self, sink: &mut F) {
        let tile_row = (self.rows - self.band_rows) / TILE;
        let stride = self.width as usize * 4;
        let mut tile = Vec::new();
        for col in 0..self.width.div_ceil(TILE) {
            let x0 = (col * TILE) as usize * 4;
            let tile_w = TILE.min(self.width - col * TILE);
            let len = tile_w as usize * 4;
            tile.clear();
            for y in 0..self.band_rows as usize {
                let start = y * stride + x0;
                tile.extend_from_slice(&self.band[start..start + len]);
            }
            sink(self.level, col, tile_row, tile_w, self.band_rows, &tile);
        }
        self.band.clear();
        self.band_rows = 0;
    }
}

/// Writes the 2×2 mean of two rows to `out`. The mean of the colors is
/// weighted by alpha, so a transparent pixel does not change the color.
fn downsample(top: &[u8], bottom: &[u8], width: u32, out: &mut Vec<u8>) {
    out.clear();
    let w = width as usize;
    for x in 0..w.div_ceil(2) {
        let a = 2 * x;
        let b = (2 * x + 1).min(w - 1);
        let px = [
            &top[a * 4..a * 4 + 4],
            &top[b * 4..b * 4 + 4],
            &bottom[a * 4..a * 4 + 4],
            &bottom[b * 4..b * 4 + 4],
        ];
        let alpha: u32 = px.iter().map(|p| p[3] as u32).sum();
        for c in 0..3 {
            let sum: u32 = px.iter().map(|p| p[c] as u32 * p[3] as u32).sum();
            let v = (sum + alpha / 2).checked_div(alpha).unwrap_or(0);
            out.push(v as u8);
        }
        out.push(((alpha + 2) / 4) as u8);
    }
}

/// Builds a tile pyramid from strips of straight RGBA8 rows, from the top
/// row down. The sink gets each tile as `(level, col, row, tile_w, tile_h,
/// rgba)`. Edge tiles have their real size.
pub struct PyramidBuilder<F: FnMut(u32, u32, u32, u32, u32, &[u8])> {
    width: u32,
    height: u32,
    rows: u32,
    levels: Vec<Level>,
    sink: F,
}

impl<F: FnMut(u32, u32, u32, u32, u32, &[u8])> PyramidBuilder<F> {
    pub fn new(width: u32, height: u32, sink: F) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let n = levels(width, height);
        let levels = (0..n)
            .map(|k| Level::new(k, level_size(width, height, k), k + 1 == n))
            .collect();
        PyramidBuilder {
            width,
            height,
            rows: 0,
            levels,
            sink,
        }
    }

    /// Adds `rows` rows of `width * 4` bytes each. Rows past the height
    /// are ignored.
    pub fn push_strip(&mut self, rgba: &[u8], rows: u32) {
        let stride = self.width as usize * 4;
        for row in rgba.chunks_exact(stride).take(rows as usize) {
            self.push_row(row);
        }
    }

    fn push_row(&mut self, row: &[u8]) {
        if self.rows >= self.height {
            return;
        }
        self.rows += 1;
        let mut more = self.levels[0].push(row, &mut self.sink);
        let mut k = 0;
        while more && k + 1 < self.levels.len() {
            let (done, rest) = self.levels.split_at_mut(k + 1);
            more = rest[0].push(&done[k].down, &mut self.sink);
            k += 1;
        }
    }

    /// Completes the pyramid. Missing rows are transparent.
    pub fn finish(mut self) {
        let row = vec![0u8; self.width as usize * 4];
        while self.rows < self.height {
            self.push_row(&row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Tile = (u32, u32, u32, u32, u32, Vec<u8>);

    fn build(width: u32, height: u32, rgba: &[u8], strip: u32) -> Vec<Tile> {
        let mut tiles = Vec::new();
        let mut builder = PyramidBuilder::new(width, height, |l, c, r, w, h, px: &[u8]| {
            tiles.push((l, c, r, w, h, px.to_vec()))
        });
        let stride = width as usize * 4;
        for chunk in rgba.chunks(stride * strip as usize) {
            builder.push_strip(chunk, (chunk.len() / stride) as u32);
        }
        builder.finish();
        tiles
    }

    #[test]
    fn level_sizes() {
        assert_eq!(levels(256, 128), 1);
        assert_eq!(levels(257, 128), 2);
        assert_eq!(levels(16384, 8192), 7);
        assert_eq!(levels(1000, 500), 3);
        assert_eq!(level_size(1000, 501, 0), (1000, 501));
        assert_eq!(level_size(1000, 501, 1), (500, 251));
        assert_eq!(level_size(1000, 501, 2), (250, 126));
        assert_eq!(level_size(16384, 8192, 6), (256, 128));
    }

    #[test]
    fn tiles_cover_each_level() {
        let (w, h) = (600, 301);
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8).collect();
        let tiles = build(w, h, &rgba, 37);
        for level in 0..levels(w, h) {
            let (lw, lh) = level_size(w, h, level);
            let mut area = 0;
            for t in tiles.iter().filter(|t| t.0 == level) {
                assert_eq!(t.3, TILE.min(lw - t.1 * TILE));
                assert_eq!(t.4, TILE.min(lh - t.2 * TILE));
                assert_eq!(t.5.len(), (t.3 * t.4 * 4) as usize);
                area += t.3 * t.4;
            }
            assert_eq!(area, lw * lh, "level {level}");
        }
        // Level 0 holds the source pixels.
        assert_eq!((tiles[0].0, tiles[0].1, tiles[0].2), (0, 0, 0));
        assert_eq!(&tiles[0].5[..8], &rgba[..8]);
        let edge = tiles
            .iter()
            .find(|t| t.0 == 0 && t.1 == 2 && t.2 == 1)
            .unwrap();
        let src = (256 * w as usize + 512) * 4;
        assert_eq!(&edge.5[..4], &rgba[src..src + 4]);
    }

    #[test]
    fn mean_is_premultiplied() {
        // An opaque red pixel and three transparent white pixels.
        let rgba = [
            255, 0, 0, 255, 255, 255, 255, 0, //
            255, 255, 255, 0, 255, 255, 255, 0,
        ];
        let mut out = Vec::new();
        downsample(&rgba[..8], &rgba[8..], 2, &mut out);
        assert_eq!(out, [255, 0, 0, 64]);

        // A half-transparent black pixel and an opaque white pixel.
        let rgba = [0, 0, 0, 128, 255, 255, 255, 255];
        downsample(&rgba, &rgba, 2, &mut out);
        assert_eq!(out, [170, 170, 170, 192]);

        // All transparent.
        downsample(
            &[9, 9, 9, 0, 9, 9, 9, 0],
            &[9, 9, 9, 0, 9, 9, 9, 0],
            2,
            &mut out,
        );
        assert_eq!(out, [0, 0, 0, 0]);
    }

    #[test]
    fn odd_edges_clamp() {
        // 3×3 opaque, level 1 is 2×2: the right column and the bottom row
        // repeat the edge pixels.
        let mut rgba = Vec::new();
        for y in 0..3u8 {
            for x in 0..3u8 {
                rgba.extend_from_slice(&[x * 100, y * 100, 0, 255]);
            }
        }
        let mut builder_tiles = Vec::new();
        let mut b = PyramidBuilder::new(3, 3, |l, _, _, w, h, px: &[u8]| {
            builder_tiles.push((l, w, h, px.to_vec()))
        });
        // Force a second level, though the width is under one tile.
        b.levels[0].last = false;
        b.levels.push(Level::new(1, (2, 2), true));
        b.push_strip(&rgba, 3);
        b.finish();
        let level1 = builder_tiles.iter().find(|t| t.0 == 1).unwrap();
        assert_eq!((level1.1, level1.2), (2, 2));
        assert_eq!(
            level1.3,
            [
                50, 50, 0, 255, 200, 50, 0, 255, 50, 200, 0, 255, 200, 200, 0, 255
            ]
        );
    }

    #[test]
    fn missing_rows_are_transparent() {
        let tiles = build(4, 4, &[255; 4 * 4 * 2], 2);
        assert_eq!(tiles.len(), 1);
        assert_eq!(&tiles[0].5[32..36], &[0, 0, 0, 0]);
    }
}
