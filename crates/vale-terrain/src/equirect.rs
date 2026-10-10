//! The import of an equirectangular image into the cube map.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::cube::{FACES, unwarp};
use crate::heightmap::Heightmap;
use crate::math::V3;

/// An equirectangular image of levels that covers the full globe. Row 0 is
/// the north edge, and column 0 starts at longitude -180.
pub struct Equirect {
    width: usize,
    height: usize,
    /// The levels, row by row.
    levels: Vec<u16>,
}

impl Equirect {
    /// Returns `None` if a side is zero or `levels` has the wrong length.
    pub fn new(width: usize, height: usize, levels: Vec<u16>) -> Option<Equirect> {
        let fits = width > 0 && height > 0 && width.checked_mul(height) == Some(levels.len());
        fits.then_some(Equirect {
            width,
            height,
            levels,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// True if the image is twice as wide as it is tall.
    pub fn is_two_to_one(&self) -> bool {
        self.width == 2 * self.height
    }

    /// The level at a direction, from the four nearest pixels. Pixel centers
    /// are at longitude `-180 + (i + 0.5) * 360 / width` and at latitude
    /// `90 - (j + 0.5) * 180 / height`. Longitude wraps. Past the center of the
    /// first or the last row, the sample crosses the pole: it reads the same
    /// row again, half the width away. An image with an odd width has no such
    /// column, so the sample reads the same row with no shift.
    pub fn sample(&self, d: V3) -> u16 {
        let lon = d[1].atan2(d[0]);
        let lat = d[2].atan2((d[0] * d[0] + d[1] * d[1]).sqrt());
        self.pixel(self.column(lon), self.line(lat))
    }

    /// The pixel coordinate of a longitude in radians. The center of pixel
    /// `i` is at `i`.
    fn column(&self, lon: f64) -> f64 {
        let w = self.width as f64;
        ((lon + PI) / TAU * w - 0.5).clamp(-0.5, w - 0.5)
    }

    /// The pixel coordinate of a latitude in radians.
    fn line(&self, lat: f64) -> f64 {
        let h = self.height as f64;
        ((FRAC_PI_2 - lat) / PI * h - 0.5).clamp(-0.5, h - 0.5)
    }

    /// The level at a pixel coordinate, from the four nearest pixels.
    fn pixel(&self, fx: f64, fy: f64) -> u16 {
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let x0 = if x0 < 0.0 {
            self.width - 1
        } else {
            x0 as usize
        };
        let x1 = if x0 + 1 == self.width { 0 } else { x0 + 1 };
        let top = self.row(y0 as isize, x0, x1, tx);
        let bottom = self.row(y0 as isize + 1, x0, x1, tx);
        (top + (bottom - top) * ty + 0.5) as u16
    }

    /// The level between two columns of row `j`. Row -1 and row `height` are
    /// the rows across the poles.
    fn row(&self, j: isize, x0: usize, x1: usize, tx: f64) -> f64 {
        let (w, h) = (self.width, self.height as isize);
        let across = j < 0 || j >= h;
        let shift = if across && w.is_multiple_of(2) {
            w / 2
        } else {
            0
        };
        let start = j.clamp(0, h - 1) as usize * w;
        let at = |x: usize| {
            let x = x + shift;
            f64::from(self.levels[start + if x >= w { x - w } else { x }])
        };
        let (a, b) = (at(x0), at(x1));
        a + (b - a) * tx
    }

    /// Halves the image while its width is at least `8 * face_size`. One
    /// pixel at the equator is then not much finer than one cube texel,
    /// because the equator has `4 * face_size` texels. Each new pixel is the
    /// rounded mean of 2 by 2 pixels. An image with an odd side stays as it is.
    pub fn reduced_for(mut self, face_size: usize) -> Equirect {
        while self.width >= 8 * face_size
            && self.width.is_multiple_of(2)
            && self.height.is_multiple_of(2)
        {
            let (w, h) = (self.width / 2, self.height / 2);
            let mut levels = Vec::with_capacity(w * h);
            for rows in self.levels.chunks_exact(4 * w) {
                let (top, bottom) = rows.split_at(2 * w);
                levels.extend((0..w).map(|i| {
                    let pair = |row: &[u16]| u32::from(row[2 * i]) + u32::from(row[2 * i + 1]);
                    ((pair(top) + pair(bottom) + 2) / 4) as u16
                }));
            }
            self = Equirect {
                width: w,
                height: h,
                levels,
            };
        }
        self
    }

    /// The levels of one whole cube face of `n` by `n` texels, row by row.
    /// Each level is the sample at the texel center. `Heightmap::store_face`
    /// takes this layout.
    pub fn face(&self, face: usize, n: usize) -> Vec<u16> {
        let flat: Vec<f64> = (0..n)
            .map(|i| unwarp((i as f64 + 0.5) / n as f64 * 2.0 - 1.0))
            .collect();
        let axis = face / 2;
        let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
        let mut out = Vec::with_capacity(n * n);
        if axis == 2 {
            for &b in &flat {
                for &a in &flat {
                    out.push(self.sample([a, b, sign]));
                }
            }
            return out;
        }
        // On a side face, the longitude changes along one side only. That
        // side is `u` on the X faces and `v` on the Y faces. The other side
        // is the Z axis.
        let columns: Vec<f64> = flat
            .iter()
            .map(|&p| {
                let lon = if axis == 0 {
                    p.atan2(sign)
                } else {
                    sign.atan2(p)
                };
                self.column(lon)
            })
            .collect();
        // The tangent of the latitude is `z` times this factor.
        let factors: Vec<f64> = flat.iter().map(|&p| 1.0 / (1.0 + p * p).sqrt()).collect();
        for y in 0..n {
            for x in 0..n {
                let (i, z) = if axis == 0 {
                    (x, flat[y])
                } else {
                    (y, flat[x])
                };
                out.push(self.pixel(columns[i], self.line((z * factors[i]).atan())));
            }
        }
        out
    }
}

impl Heightmap {
    /// Replaces all six faces from the image, as one undo entry.
    pub fn import_equirect(&mut self, src: &Equirect) {
        self.begin_stroke();
        for face in 0..FACES {
            self.store_face(face, &src.face(face, self.face_size()));
        }
        self.end_stroke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heightmap::TexelRect;
    use crate::math::lonlat_to_dir;

    const N: usize = 64;

    /// A smooth function of the direction that takes each value from -1 to 1.
    fn hills(d: V3) -> f64 {
        d[0] + 0.5 * d[1] * d[2]
    }

    /// A smooth function that changes fast with longitude at longitude 180,
    /// and that has a slope at each pole.
    fn tilt(d: V3) -> f64 {
        0.7 * d[1] + 0.3 * d[0] * d[2]
    }

    fn level(value: f64) -> f64 {
        32767.5 + 32767.5 * value
    }

    fn image(width: usize, height: usize, f: impl Fn(V3) -> f64) -> Equirect {
        let mut levels = Vec::with_capacity(width * height);
        for j in 0..height {
            let lat = 90.0 - (j as f64 + 0.5) * 180.0 / height as f64;
            for i in 0..width {
                let lon = -180.0 + (i as f64 + 0.5) * 360.0 / width as f64;
                levels.push(level(f(lonlat_to_dir(lon, lat))).round() as u16);
            }
        }
        Equirect::new(width, height, levels).unwrap()
    }

    fn imported(src: &Equirect) -> Heightmap {
        let mut map = Heightmap::with_tile_size(N, 16, 0);
        map.import_equirect(src);
        map
    }

    fn all_texels(map: &Heightmap) -> Vec<u16> {
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: N,
            y1: N,
        };
        let mut out = Vec::new();
        for face in 0..FACES {
            map.read_rect(face, full, &mut out);
        }
        out
    }

    /// The largest difference between a texel and the function.
    fn worst_error(map: &Heightmap, f: impl Fn(V3) -> f64) -> f64 {
        let mut worst = 0.0f64;
        for face in 0..FACES {
            for y in 0..N {
                for x in 0..N {
                    let want = level(f(map.texel_dir(face, x, y)));
                    let got = f64::from(map.get(face, x as i64, y as i64));
                    worst = worst.max((got - want).abs());
                }
            }
        }
        worst
    }

    /// The largest difference between two texels that are side by side on
    /// one face.
    fn largest_step(map: &Heightmap) -> u16 {
        let mut step = 0;
        for face in 0..FACES {
            for y in 0..N as i64 {
                for x in 0..N as i64 - 1 {
                    step = step
                        .max(map.get(face, x, y).abs_diff(map.get(face, x + 1, y)))
                        .max(map.get(face, y, x).abs_diff(map.get(face, y, x + 1)));
                }
            }
        }
        step
    }

    #[test]
    fn import_keeps_the_levels_of_the_image() {
        let map = imported(&image(512, 256, hills));
        // The error of a bilinear sample is at most `h * h / 8` times the sum
        // of the second derivatives along longitude and latitude. `h` is the
        // pixel size in radians. For `hills`, the sum is at most 3.25. The
        // rounding of the image and of the sample adds 0.5 each.
        let h = TAU / 512.0;
        let bound = 32767.5 * 3.25 * h * h / 8.0 + 1.0;
        assert!(bound < 3.1);
        let worst = worst_error(&map, hills);
        assert!(worst <= bound, "{worst} > {bound}");
        let texels = all_texels(&map);
        assert!(*texels.iter().min().unwrap() < 1000);
        assert!(*texels.iter().max().unwrap() > 64500);
    }

    #[test]
    fn import_has_no_seam_at_a_face_edge() {
        for f in [hills, tilt] {
            let map = imported(&image(512, 256, f));
            let inside = largest_step(&map);
            let n = N as i64;
            for face in 0..FACES {
                for i in 0..n {
                    let pairs = [
                        (map.get(face, 0, i), map.get(face, -1, i)),
                        (map.get(face, n - 1, i), map.get(face, n, i)),
                        (map.get(face, i, 0), map.get(face, i, -1)),
                        (map.get(face, i, n - 1), map.get(face, i, n)),
                    ];
                    for (a, b) in pairs {
                        assert!(a.abs_diff(b) <= inside, "face {face}, texel {i}");
                    }
                }
            }
        }
    }

    /// The image is coarse, so many texels are past the last pixel center at
    /// longitude 180 and at each pole.
    #[test]
    fn import_has_no_seam_at_longitude_180_and_at_the_poles() {
        let map = imported(&image(64, 32, tilt));
        let inside = largest_step(&map);
        let h = TAU / 64.0;
        // `tilt` has second derivatives with a sum of at most 2.75.
        let bound = 32767.5 * 2.75 * h * h / 8.0 + 1.0;
        let worst = worst_error(&map, tilt);
        assert!(worst <= bound, "{worst} > {bound}");

        // Longitude 180 is the center line of face 1.
        let mid = N as i64 / 2;
        let mut across = 0;
        let mut beside = 0;
        for y in 0..N as i64 {
            across = across.max(map.get(1, mid - 1, y).abs_diff(map.get(1, mid, y)));
            beside = beside.max(map.get(1, mid, y).abs_diff(map.get(1, mid + 1, y)));
        }
        assert!(across > 500, "the image has a slope at longitude 180");
        assert!(f64::from(across) <= 1.25 * f64::from(beside));

        // Each pole is the center of face 4 or face 5. The pixels of the
        // first image row are 2.8 degrees from the pole, which is 2 texels.
        for face in [4, 5] {
            let mut step = 0;
            for y in mid - 3..mid + 3 {
                for x in mid - 3..mid + 2 {
                    step = step
                        .max(map.get(face, x, y).abs_diff(map.get(face, x + 1, y)))
                        .max(map.get(face, y, x).abs_diff(map.get(face, y, x + 1)));
                }
            }
            assert!(step > 300, "the image has a slope at the pole");
            assert!(step <= inside, "face {face}");
        }
    }

    #[test]
    fn import_keeps_16_bit_levels() {
        let flat = Equirect::new(512, 256, vec![12345; 512 * 256]).unwrap();
        assert!(all_texels(&imported(&flat)).iter().all(|&l| l == 12345));

        // The south half is one level higher than the north half.
        let mut levels = vec![30000; 512 * 128];
        levels.resize(512 * 256, 30001);
        let texels = all_texels(&imported(&Equirect::new(512, 256, levels).unwrap()));
        assert!(texels.iter().all(|&l| l == 30000 || l == 30001));
        assert!(texels.contains(&30000) && texels.contains(&30001));
        // Face 4 is all north, and face 5 is all south.
        assert!(texels[4 * N * N..5 * N * N].iter().all(|&l| l == 30000));
        assert!(texels[5 * N * N..].iter().all(|&l| l == 30001));
    }

    #[test]
    fn image_of_any_shape_covers_the_globe() {
        let image_of = |w: usize, h: usize| Equirect::new(w, h, vec![777; w * h]);
        assert!(image_of(512, 256).unwrap().is_two_to_one());
        assert!(!image_of(512, 300).unwrap().is_two_to_one());
        assert!(!image_of(256, 256).unwrap().is_two_to_one());
        assert!(Equirect::new(4, 2, vec![0; 7]).is_none());
        assert!(Equirect::new(0, 2, Vec::new()).is_none());
        assert!(Equirect::new(2, 0, Vec::new()).is_none());

        let map = imported(&image(512, 300, hills));
        assert!(worst_error(&map, hills) < 3.1);
        let map = imported(&image(256, 256, hills));
        assert!(worst_error(&map, hills) < 6.0);
        // An odd width has no column across the pole.
        let map = imported(&image_of(7, 3).unwrap());
        assert!(all_texels(&map).iter().all(|&l| l == 777));
    }

    #[test]
    fn reduced_image_is_near_the_texel_size() {
        let levels: Vec<u16> = (0..64 * 32).map(|i| (i * 31 % 65536) as u16).collect();
        let mean = |levels: &[u16]| {
            levels.iter().map(|&l| f64::from(l)).sum::<f64>() / levels.len() as f64
        };
        let before = mean(&levels);
        let src = Equirect::new(64, 32, levels).unwrap();
        // 64 and 32 are at least 8 * 4, and 16 is not.
        let small = src.reduced_for(4);
        assert_eq!((small.width(), small.height()), (16, 8));
        // Each of the two steps rounds by at most half a level.
        assert!((mean(&small.levels) - before).abs() <= 1.0);

        let src = Equirect::new(4, 2, vec![10, 20, 1, 2, 30, 41, 3, 5]).unwrap();
        let small = src.reduced_for(0);
        assert_eq!((small.width(), small.height()), (2, 1));
        // 101 / 4 rounds to 25, and 11 / 4 rounds to 3.
        assert_eq!(small.levels, [25, 3]);

        let src = Equirect::new(64, 32, vec![0; 64 * 32]).unwrap();
        assert_eq!(src.reduced_for(16).width(), 64);
        let src = Equirect::new(12, 6, vec![0; 72]).unwrap();
        assert_eq!(src.reduced_for(1).height(), 3);
    }

    #[test]
    fn undo_puts_back_the_map_from_before_an_import() {
        let mut map = Heightmap::with_tile_size(N, 16, 100);
        map.set(2, 20, 20, 7);
        map.set(5, 63, 0, 60000);
        let before = all_texels(&map);
        let tiles = map.allocated_tiles();
        map.take_reset();
        map.take_dirty();

        map.import_equirect(&image(512, 256, hills));
        let full = TexelRect {
            x0: 0,
            y0: 0,
            x1: N,
            y1: N,
        };
        assert_eq!(map.take_dirty(), [Some(full); FACES]);
        assert!(!map.take_reset());
        assert_ne!(all_texels(&map), before);

        assert!(map.undo());
        assert_eq!(all_texels(&map), before);
        assert_eq!(map.allocated_tiles(), tiles);
        assert_eq!(map.take_dirty(), [Some(full); FACES]);
        assert!(!map.can_undo());
    }
}
