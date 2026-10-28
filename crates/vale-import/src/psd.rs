//! A reader for the composite image of a PSD or PSB file, a strip of
//! rows at a time.
//!
//! The reader supports the RGB and grayscale color modes, 8 and 16 bits
//! per channel, and raw or PackBits data. The channel after the colors is
//! alpha. Photoshop mattes the composite with white where alpha is less
//! than 1, so the reader removes the matte.

use std::io::{Read, Seek, SeekFrom};

/// The composite image of a PSD or PSB file.
pub struct Psd<R: Read + Seek> {
    reader: R,
    width: u32,
    height: u32,
    /// The number of color channels: 1 or 3.
    colors: usize,
    has_alpha: bool,
    /// The bytes of one sample: 1 or 2.
    bytes: usize,
    /// The file offset of each row of each channel that the reader uses,
    /// and the end of the last row.
    starts: Vec<u64>,
    rle: bool,
    next: u32,
    packed: Vec<u8>,
    planes: Vec<Vec<u8>>,
}

fn read_u16(r: &mut impl Read) -> Result<u16, String> {
    let mut b = [0; 2];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u16::from_be_bytes(b))
}

fn read_u32(r: &mut impl Read) -> Result<u32, String> {
    let mut b = [0; 4];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u32::from_be_bytes(b))
}

fn read_u64(r: &mut impl Read) -> Result<u64, String> {
    let mut b = [0; 8];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u64::from_be_bytes(b))
}

fn skip(r: &mut impl Seek, len: u64) -> Result<(), String> {
    let len = i64::try_from(len).map_err(|_| "a section length is too large".to_string())?;
    r.seek(SeekFrom::Current(len)).map_err(|e| e.to_string())?;
    Ok(())
}

impl<R: Read + Seek> Psd<R> {
    /// Reads the header and the offsets of the rows.
    pub fn new(mut reader: R) -> Result<Self, String> {
        let mut sig = [0; 4];
        reader.read_exact(&mut sig).map_err(|e| e.to_string())?;
        if &sig != b"8BPS" {
            return Err("the file is not a PSD file".into());
        }
        let big = match read_u16(&mut reader)? {
            1 => false,
            2 => true,
            v => return Err(format!("the PSD version {v} is not supported")),
        };
        skip(&mut reader, 6)?;
        let channels = read_u16(&mut reader)? as usize;
        let height = read_u32(&mut reader)?;
        let width = read_u32(&mut reader)?;
        let depth = read_u16(&mut reader)?;
        let mode = read_u16(&mut reader)?;
        let colors = match mode {
            1 => 1,
            3 => 3,
            _ => return Err("the PSD color mode is not supported; use RGB or grayscale".into()),
        };
        let bytes = match depth {
            8 => 1,
            16 => 2,
            _ => return Err(format!("{depth}-bit PSD files are not supported")),
        };
        if width == 0 || height == 0 {
            return Err("the image is empty".into());
        }
        if channels < colors {
            return Err("the PSD file has too few channels".into());
        }
        let has_alpha = channels > colors;
        let used = colors + has_alpha as usize;

        let color_data = read_u32(&mut reader)?;
        skip(&mut reader, color_data as u64)?;
        let resources = read_u32(&mut reader)?;
        skip(&mut reader, resources as u64)?;
        let layers = if big {
            read_u64(&mut reader)?
        } else {
            read_u32(&mut reader)? as u64
        };
        skip(&mut reader, layers)?;

        let compression = read_u16(&mut reader)?;
        let h = height as usize;
        let row_bytes = width as u64 * bytes as u64;
        let mut starts = Vec::with_capacity(used * h + 1);
        let rle = match compression {
            0 => {
                let data = reader.stream_position().map_err(|e| e.to_string())?;
                for i in 0..=(used * h) as u64 {
                    starts.push(data + i * row_bytes);
                }
                false
            }
            1 => {
                let entry = if big { 4 } else { 2 };
                let table = reader.stream_position().map_err(|e| e.to_string())?;
                let mut offset = table + (channels * h * entry) as u64;
                let mut counts = std::io::BufReader::new(&mut reader);
                for _ in 0..used * h {
                    starts.push(offset);
                    offset += if big {
                        read_u32(&mut counts)? as u64
                    } else {
                        read_u16(&mut counts)? as u64
                    };
                }
                starts.push(offset);
                true
            }
            _ => return Err("the PSD compression is not supported; save without ZIP".into()),
        };
        Ok(Psd {
            reader,
            width,
            height,
            colors,
            has_alpha,
            bytes,
            starts,
            rle,
            next: 0,
            packed: Vec::new(),
            planes: vec![Vec::new(); used],
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn has_alpha(&self) -> bool {
        self.has_alpha
    }

    /// Appends up to `rows` rows of straight RGBA8 to `out` and returns
    /// the number of rows.
    pub fn next_strip(&mut self, rows: u32, out: &mut Vec<u8>) -> Result<u32, String> {
        let n = rows.min(self.height - self.next);
        if n == 0 {
            return Ok(0);
        }
        let h = self.height as usize;
        let y0 = self.next as usize;
        let row_bytes = self.width as usize * self.bytes;
        for c in 0..self.planes.len() {
            let first = c * h + y0;
            let (start, end) = (self.starts[first], self.starts[first + n as usize]);
            self.reader
                .seek(SeekFrom::Start(start))
                .map_err(|e| e.to_string())?;
            let len = usize::try_from(end - start).map_err(|_| "a row is too large")?;
            let plane = &mut self.planes[c];
            plane.clear();
            if self.rle {
                self.packed.resize(len, 0);
                self.reader
                    .read_exact(&mut self.packed)
                    .map_err(|e| e.to_string())?;
                for y in 0..n as usize {
                    let a = (self.starts[first + y] - start) as usize;
                    let b = (self.starts[first + y + 1] - start) as usize;
                    unpack(&self.packed[a..b], row_bytes, plane)?;
                }
            } else {
                plane.resize(len, 0);
                self.reader.read_exact(plane).map_err(|e| e.to_string())?;
            }
        }
        out.reserve(n as usize * self.width as usize * 4);
        let pixels = n as usize * self.width as usize;
        let max: i64 = if self.bytes == 1 { 255 } else { 65535 };
        let sample = |plane: &[u8], i: usize| -> i64 {
            if self.bytes == 1 {
                plane[i] as i64
            } else {
                u16::from_be_bytes([plane[2 * i], plane[2 * i + 1]]) as i64
            }
        };
        let to8 = |v: i64| -> u8 {
            if max == 255 {
                v as u8
            } else {
                ((v * 255 + 32767) / 65535) as u8
            }
        };
        for i in 0..pixels {
            let a = if self.has_alpha {
                sample(&self.planes[self.colors], i)
            } else {
                max
            };
            let mut rgb = [0u8; 3];
            for (k, v) in rgb.iter_mut().enumerate() {
                let c = sample(&self.planes[k.min(self.colors - 1)], i);
                *v = to8(unmatte(c, a, max));
            }
            out.extend_from_slice(&rgb);
            out.push(to8(a));
        }
        self.next += n;
        Ok(n)
    }
}

/// Removes the white matte: c = (c' − (1 − a)) / a.
fn unmatte(c: i64, a: i64, max: i64) -> i64 {
    if a == max {
        return c;
    }
    if a == 0 {
        return 0;
    }
    let num = c - (max - a);
    if num <= 0 {
        return 0;
    }
    ((num * max + a / 2) / a).min(max)
}

/// Appends one PackBits row of `len` bytes to `out`.
fn unpack(mut data: &[u8], len: usize, out: &mut Vec<u8>) -> Result<(), String> {
    let end = out.len() + len;
    let bad = || "the PackBits data is not valid".to_string();
    while out.len() < end {
        let (&head, rest) = data.split_first().ok_or_else(bad)?;
        data = rest;
        let head = head as i8;
        if head >= 0 {
            let count = head as usize + 1;
            if data.len() < count || out.len() + count > end {
                return Err(bad());
            }
            out.extend_from_slice(&data[..count]);
            data = &data[count..];
        } else if head != -128 {
            let count = 1 - head as isize;
            let (&v, rest) = data.split_first().ok_or_else(bad)?;
            data = rest;
            if out.len() + count as usize > end {
                return Err(bad());
            }
            out.resize(out.len() + count as usize, v);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// Encodes one row with PackBits.
    fn pack(row: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < row.len() {
            let mut run = 1;
            while i + run < row.len() && run < 128 && row[i + run] == row[i] {
                run += 1;
            }
            if run >= 2 {
                out.push((1 - run as isize) as i8 as u8);
                out.push(row[i]);
                i += run;
            } else {
                let start = i;
                while i < row.len()
                    && i - start < 128
                    && !(i + 1 < row.len() && row[i + 1] == row[i])
                {
                    i += 1;
                }
                if i == start {
                    i += 1;
                }
                out.push((i - start - 1) as u8);
                out.extend_from_slice(&row[start..i]);
            }
        }
        out
    }

    /// Builds a PSD (or PSB) file from planar channels of big-endian
    /// samples.
    pub(crate) fn build(
        big: bool,
        rle: bool,
        mode: u16,
        depth: u16,
        width: u32,
        height: u32,
        channels: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(b"8BPS");
        f.extend_from_slice(&(if big { 2u16 } else { 1 }).to_be_bytes());
        f.extend_from_slice(&[0; 6]);
        f.extend_from_slice(&(channels.len() as u16).to_be_bytes());
        f.extend_from_slice(&height.to_be_bytes());
        f.extend_from_slice(&width.to_be_bytes());
        f.extend_from_slice(&depth.to_be_bytes());
        f.extend_from_slice(&mode.to_be_bytes());
        // Color mode data, image resources, and layer info, with content
        // that the reader must skip.
        f.extend_from_slice(&3u32.to_be_bytes());
        f.extend_from_slice(&[7; 3]);
        f.extend_from_slice(&5u32.to_be_bytes());
        f.extend_from_slice(&[7; 5]);
        if big {
            f.extend_from_slice(&4u64.to_be_bytes());
        } else {
            f.extend_from_slice(&4u32.to_be_bytes());
        }
        f.extend_from_slice(&[7; 4]);
        f.extend_from_slice(&(rle as u16).to_be_bytes());
        let row_bytes = width as usize * depth as usize / 8;
        if rle {
            let rows: Vec<Vec<u8>> = channels
                .iter()
                .flat_map(|c| c.chunks(row_bytes).map(pack))
                .collect();
            for r in &rows {
                if big {
                    f.extend_from_slice(&(r.len() as u32).to_be_bytes());
                } else {
                    f.extend_from_slice(&(r.len() as u16).to_be_bytes());
                }
            }
            for r in &rows {
                f.extend_from_slice(r);
            }
        } else {
            for c in channels {
                f.extend_from_slice(c);
            }
        }
        f
    }

    fn read_all(file: Vec<u8>, strip: u32) -> (Psd<Cursor<Vec<u8>>>, Vec<u8>) {
        let mut psd = Psd::new(Cursor::new(file)).unwrap();
        let mut out = Vec::new();
        while psd.next_strip(strip, &mut out).unwrap() > 0 {}
        (psd, out)
    }

    #[test]
    fn packbits_round_trip() {
        let row: Vec<u8> = (0..300)
            .map(|i| if i % 50 < 30 { 9 } else { i as u8 })
            .collect();
        let mut out = Vec::new();
        unpack(&pack(&row), row.len(), &mut out).unwrap();
        assert_eq!(out, row);
        assert!(unpack(&[5, 1], 6, &mut Vec::new()).is_err());
    }

    #[test]
    fn rgb_8_bit_raw_and_rle() {
        let (w, h) = (5, 3);
        let r: Vec<u8> = (0..w * h).map(|i| i as u8 * 10).collect();
        let g = vec![200; (w * h) as usize];
        let b: Vec<u8> = (0..w * h).map(|i| 255 - i as u8).collect();
        let mut expected = Vec::new();
        for i in 0..(w * h) as usize {
            expected.extend_from_slice(&[r[i], g[i], b[i], 255]);
        }
        for (big, rle) in [(false, false), (false, true), (true, false), (true, true)] {
            let file = build(big, rle, 3, 8, w, h, &[r.clone(), g.clone(), b.clone()]);
            let (psd, out) = read_all(file, 2);
            assert!(!psd.has_alpha());
            assert_eq!((psd.width(), psd.height()), (w, h));
            assert_eq!(out, expected, "big {big} rle {rle}");
        }
    }

    #[test]
    fn alpha_is_unmatted() {
        // One opaque pixel, one half-transparent red pixel matted with
        // white, and one transparent pixel.
        let a = [255u8, 128, 0];
        // c' = c·a + (1 − a)·255 for c = (255, 0, 0).
        let matte =
            |c: u8, a: u8| ((c as u32 * a as u32 + (255 - a as u32) * 255 + 127) / 255) as u8;
        let r = vec![10, matte(255, 128), 255];
        let g = vec![20, matte(0, 128), 255];
        let b = vec![30, matte(0, 128), 255];
        let file = build(false, true, 3, 8, 3, 1, &[r, g, b, a.to_vec()]);
        let (psd, out) = read_all(file, 1);
        assert!(psd.has_alpha());
        assert_eq!(&out[..4], &[10, 20, 30, 255]);
        assert_eq!(&out[4..8], &[255, 0, 0, 128]);
        assert_eq!(&out[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn grey_16_bit_with_alpha() {
        let be = |v: &[u16]| v.iter().flat_map(|s| s.to_be_bytes()).collect::<Vec<u8>>();
        // Grey 0.25 at alpha 0.5, matted: 0.25·0.5 + 0.5 = 0.625.
        let grey = be(&[65535, 40960, 0]);
        let alpha = be(&[65535, 32768, 65535]);
        for rle in [false, true] {
            let file = build(true, rle, 1, 16, 3, 1, &[grey.clone(), alpha.clone()]);
            let (psd, out) = read_all(file, 4);
            assert!(psd.has_alpha());
            assert_eq!(&out[..4], &[255, 255, 255, 255]);
            assert_eq!(&out[4..8], &[64, 64, 64, 128]);
            assert_eq!(&out[8..12], &[0, 0, 0, 255]);
        }
    }

    #[test]
    fn errors() {
        assert!(Psd::new(Cursor::new(b"8BPX".to_vec())).is_err());
        let cmyk = build(
            false,
            false,
            4,
            8,
            1,
            1,
            &[vec![0], vec![0], vec![0], vec![0]],
        );
        let err = Psd::new(Cursor::new(cmyk)).err().unwrap();
        assert!(err.contains("color mode"));
        let mut short = build(
            false,
            false,
            3,
            8,
            4,
            4,
            &[vec![0; 16], vec![0; 16], vec![0; 16]],
        );
        short.truncate(short.len() - 10);
        let mut psd = Psd::new(Cursor::new(short)).unwrap();
        let mut out = Vec::new();
        assert!(psd.next_strip(4, &mut out).is_err());
    }
}
