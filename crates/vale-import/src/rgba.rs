//! A reader of straight RGBA8 rows from a PNG, TIFF, or PSD file, a strip
//! at a time. The reader never holds the whole image, so a large image
//! loads in a small amount of memory.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use tiff::ColorType as TiffColor;
use tiff::decoder::{ChunkType, Decoder as TiffDecoder, DecodingResult, Limits as TiffLimits};
use tiff::tags::Tag;

use crate::ImportError;
use crate::psd::Psd;

/// The largest block of pixels that the reader decodes at one time.
const MAX_ALLOC: usize = 64 << 20;

trait Input: BufRead + Seek + Send {}
impl<T: BufRead + Seek + Send> Input for T {}

type Boxed = Box<dyn Input>;

/// The channels of a pixel in the source.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Layout {
    Grey,
    GreyAlpha,
    Rgb,
    Rgba,
}

impl Layout {
    fn channels(self) -> usize {
        match self {
            Layout::Grey => 1,
            Layout::GreyAlpha => 2,
            Layout::Rgb => 3,
            Layout::Rgba => 4,
        }
    }
}

/// Appends 8-bit samples of one layout to `out` as RGBA8. If `associated`
/// is true, the colors are premultiplied by alpha, and they become
/// straight.
fn push_rgba(samples: &[u8], layout: Layout, associated: bool, out: &mut Vec<u8>) {
    let n = layout.channels();
    out.reserve(samples.len() / n * 4);
    for px in samples.chunks_exact(n) {
        let (rgb, a) = match layout {
            Layout::Grey => ([px[0]; 3], 255),
            Layout::GreyAlpha => ([px[0]; 3], px[1]),
            Layout::Rgb => ([px[0], px[1], px[2]], 255),
            Layout::Rgba => ([px[0], px[1], px[2]], px[3]),
        };
        if associated && a < 255 {
            for c in rgb {
                let v = if a == 0 {
                    0
                } else {
                    ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255)
                };
                out.push(v as u8);
            }
        } else {
            out.extend_from_slice(&rgb);
        }
        out.push(a);
    }
}

fn to8(v: u16) -> u8 {
    ((v as u32 * 255 + 32767) / 65535) as u8
}

struct PngRows {
    reader: png::Reader<Boxed>,
    layout: Layout,
    /// The decoded image, for an interlaced PNG only.
    frame: Option<Vec<u8>>,
    line: usize,
    next: u32,
}

struct TiffRows {
    decoder: TiffDecoder<Boxed>,
    layout: Layout,
    associated: bool,
    white_is_zero: bool,
    tiled: bool,
    chunk_w: u32,
    chunk_h: u32,
    /// RGBA8 rows of the current strip or row of tiles.
    band: Vec<u8>,
    /// The first row of the band that is not read.
    band_next: usize,
    /// The next strip or row of tiles.
    chunk_row: u32,
    samples: Vec<u8>,
}

enum Source {
    Png(Box<PngRows>),
    Tiff(Box<TiffRows>),
    Psd(Psd<Boxed>),
}

/// Reads an image as rows of straight RGBA8, from the top row down.
pub struct RowReader {
    name: String,
    width: u32,
    height: u32,
    has_alpha: bool,
    next: u32,
    source: Source,
}

impl RowReader {
    /// Opens a PNG, TIFF, or PSD file. The format comes from the first
    /// bytes of the file. The name in an error is the file stem.
    pub fn open(path: &Path) -> Result<RowReader, ImportError> {
        let file = File::open(path).map_err(|e| ImportError::Io {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".to_string());
        Self::new(name, Box::new(BufReader::new(file)))
    }

    /// Reads a PNG, TIFF, or PSD file from memory.
    pub fn from_bytes(name: &str, bytes: Vec<u8>) -> Result<RowReader, ImportError> {
        Self::new(name.to_string(), Box::new(Cursor::new(bytes)))
    }

    fn new(name: String, mut input: Boxed) -> Result<RowReader, ImportError> {
        let parse = |message: String| ImportError::Parse {
            name: name.clone(),
            message,
        };
        let mut magic = [0u8; 8];
        let mut len = 0;
        while len < magic.len() {
            match input.read(&mut magic[len..]) {
                Ok(0) => break,
                Ok(n) => len += n,
                Err(e) => return Err(parse(e.to_string())),
            }
        }
        input
            .seek(SeekFrom::Start(0))
            .map_err(|e| parse(e.to_string()))?;
        let magic = &magic[..len];
        let (width, height, has_alpha, source) = if magic.starts_with(b"\x89PNG\r\n\x1a\n") {
            open_png(input).map_err(parse)?
        } else if [b"II*\0", b"MM\0*", b"II+\0", b"MM\0+"]
            .iter()
            .any(|m| magic.starts_with(*m))
        {
            open_tiff(input).map_err(parse)?
        } else if magic.starts_with(b"8BPS") {
            let psd = Psd::new(input).map_err(parse)?;
            (psd.width(), psd.height(), psd.has_alpha(), Source::Psd(psd))
        } else {
            return Err(ImportError::NotImage { name });
        };
        if width == 0 || height == 0 {
            return Err(parse("the image is empty".into()));
        }
        Ok(RowReader {
            name,
            width,
            height,
            has_alpha,
            next: 0,
            source,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns true if the file has an alpha channel or a transparent
    /// color.
    pub fn has_alpha(&self) -> bool {
        self.has_alpha
    }

    /// Replaces `out` with the next rows, up to `rows` rows of
    /// `width * 4` bytes. Returns the number of rows, which is 0 at the
    /// end of the image.
    pub fn next_strip(&mut self, rows: u32, out: &mut Vec<u8>) -> Result<u32, ImportError> {
        out.clear();
        let n = rows.min(self.height - self.next);
        let result = match &mut self.source {
            Source::Png(png) => png.read(n, out),
            Source::Tiff(tiff) => tiff.read(self.width, self.height, n, out),
            Source::Psd(psd) => psd.next_strip(n, out).map(|_| ()),
        };
        result.map_err(|message| ImportError::Parse {
            name: self.name.clone(),
            message,
        })?;
        self.next += n;
        Ok(n)
    }
}

fn open_png(input: Boxed) -> Result<(u32, u32, bool, Source), String> {
    let mut decoder = png::Decoder::new(input);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    let has_alpha = info.color_type == png::ColorType::GrayscaleAlpha
        || info.color_type == png::ColorType::Rgba
        || info.trns.is_some();
    let interlaced = info.interlaced;
    let layout = match reader.output_color_type() {
        (png::ColorType::Grayscale, png::BitDepth::Eight) => Layout::Grey,
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => Layout::GreyAlpha,
        (png::ColorType::Rgb, png::BitDepth::Eight) => Layout::Rgb,
        (png::ColorType::Rgba, png::BitDepth::Eight) => Layout::Rgba,
        _ => return Err("the PNG pixel format is not supported".into()),
    };
    let line = width as usize * layout.channels();
    let frame = if interlaced {
        // Adam7 rows come in seven passes, so the decoder needs the whole
        // image.
        let size = reader.output_buffer_size().unwrap_or(usize::MAX);
        if size > MAX_ALLOC {
            return Err("the interlaced PNG is too large; save it without interlacing".into());
        }
        let mut buf = vec![0; size];
        reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        Some(buf)
    } else {
        None
    };
    let rows = PngRows {
        reader,
        layout,
        frame,
        line,
        next: 0,
    };
    Ok((width, height, has_alpha, Source::Png(Box::new(rows))))
}

impl PngRows {
    fn read(&mut self, n: u32, out: &mut Vec<u8>) -> Result<(), String> {
        for _ in 0..n {
            if let Some(frame) = &self.frame {
                let start = self.next as usize * self.line;
                push_rgba(&frame[start..start + self.line], self.layout, false, out);
            } else {
                let row = self
                    .reader
                    .next_row()
                    .map_err(|e| e.to_string())?
                    .ok_or("the PNG has too few rows")?;
                push_rgba(&row.data()[..self.line], self.layout, false, out);
            }
            self.next += 1;
        }
        Ok(())
    }
}

fn open_tiff(input: Boxed) -> Result<(u32, u32, bool, Source), String> {
    let err = |e: tiff::TiffError| e.to_string();
    let limits = {
        let mut l = TiffLimits::default();
        l.decoding_buffer_size = MAX_ALLOC;
        l
    };
    let mut decoder = TiffDecoder::new(input).map_err(err)?.with_limits(limits);
    let (width, height) = decoder.dimensions().map_err(err)?;
    let (layout, bits) = match decoder.colortype().map_err(err)? {
        TiffColor::Gray(b) => (Layout::Grey, b),
        TiffColor::GrayA(b) => (Layout::GreyAlpha, b),
        TiffColor::RGB(b) => (Layout::Rgb, b),
        TiffColor::RGBA(b) => (Layout::Rgba, b),
        _ => return Err("the TIFF color type is not supported; use RGB or grayscale".into()),
    };
    if bits != 8 && bits != 16 {
        return Err(format!("{bits}-bit TIFF files are not supported"));
    }
    let planar = decoder
        .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
        .map_err(err)?;
    if planar == Some(2) {
        return Err("planar TIFF files are not supported".into());
    }
    let associated = decoder
        .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
        .map_err(err)?
        .is_some_and(|v| v.first() == Some(&1));
    let white_is_zero = decoder
        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
        .map_err(err)?
        == Some(0);
    let tiled = decoder.get_chunk_type() == ChunkType::Tile;
    let (chunk_w, chunk_h) = decoder.chunk_dimensions();
    let bytes = (bits / 8) as u64;
    let chunk = chunk_w as u64 * chunk_h as u64 * layout.channels() as u64 * bytes;
    let band = width as u64 * chunk_h.min(height) as u64 * 4;
    if chunk > MAX_ALLOC as u64 || band > MAX_ALLOC as u64 {
        return Err(
            "the TIFF has too many rows in one strip or tile; save it with smaller strips or tiles"
                .into(),
        );
    }
    let rows = TiffRows {
        decoder,
        layout,
        associated,
        white_is_zero,
        tiled,
        chunk_w,
        chunk_h,
        band: Vec::new(),
        band_next: 0,
        chunk_row: 0,
        samples: Vec::new(),
    };
    let has_alpha = matches!(layout, Layout::GreyAlpha | Layout::Rgba);
    Ok((width, height, has_alpha, Source::Tiff(Box::new(rows))))
}

impl TiffRows {
    fn read(&mut self, width: u32, height: u32, n: u32, out: &mut Vec<u8>) -> Result<(), String> {
        let stride = width as usize * 4;
        let mut left = n as usize;
        while left > 0 {
            let band_rows = self.band.len() / stride;
            if self.band_next == band_rows {
                self.load(width, height)?;
                continue;
            }
            let take = left.min(band_rows - self.band_next);
            let start = self.band_next * stride;
            out.extend_from_slice(&self.band[start..start + take * stride]);
            self.band_next += take;
            left -= take;
        }
        Ok(())
    }

    /// Decodes the next strip or row of tiles into the band.
    fn load(&mut self, width: u32, height: u32) -> Result<(), String> {
        let y0 = self.chunk_row * self.chunk_h;
        if y0 >= height {
            return Err("the TIFF has too few rows".into());
        }
        let rows = self.chunk_h.min(height - y0) as usize;
        let stride = width as usize * 4;
        self.band.clear();
        self.band.resize(rows * stride, 0);
        self.band_next = 0;
        let across = if self.tiled {
            width.div_ceil(self.chunk_w)
        } else {
            1
        };
        let mut px = Vec::new();
        for col in 0..across {
            let index = self.chunk_row * across + col;
            let (cw, ch) = self.decoder.chunk_data_dimensions(index);
            let data = self.decoder.read_chunk(index).map_err(|e| e.to_string())?;
            self.samples.clear();
            match data {
                DecodingResult::U8(v) => self.samples.extend_from_slice(&v),
                DecodingResult::U16(v) => self.samples.extend(v.iter().map(|&s| to8(s))),
                _ => return Err("the TIFF sample format is not supported".into()),
            }
            if self.white_is_zero {
                let n = self.layout.channels();
                for s in self.samples.iter_mut().step_by(n) {
                    *s = 255 - *s;
                }
            }
            let (cw, ch) = (cw as usize, (ch as usize).min(rows));
            let line = cw * self.layout.channels();
            if self.samples.len() < line * ch {
                return Err("a TIFF chunk is too short".into());
            }
            let x0 = (col * self.chunk_w) as usize * 4;
            for y in 0..ch {
                px.clear();
                push_rgba(
                    &self.samples[y * line..(y + 1) * line],
                    self.layout,
                    self.associated,
                    &mut px,
                );
                let start = y * stride + x0;
                self.band[start..start + px.len()].copy_from_slice(&px);
            }
        }
        self.chunk_row += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psd::tests::build as build_psd;
    use tiff::encoder::{TiffEncoder, colortype};

    fn read_all(name: &str, bytes: Vec<u8>, strip: u32) -> (RowReader, Vec<u8>) {
        let mut reader = RowReader::from_bytes(name, bytes).unwrap();
        let mut all = Vec::new();
        let mut out = Vec::new();
        loop {
            let n = reader.next_strip(strip, &mut out).unwrap();
            if n == 0 {
                break;
            }
            assert_eq!(out.len(), (n * reader.width() * 4) as usize);
            all.extend_from_slice(&out);
        }
        assert_eq!(all.len(), (reader.width() * reader.height() * 4) as usize);
        (reader, all)
    }

    fn png(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        palette: Option<(&[u8], &[u8])>,
        data: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(color);
        enc.set_depth(depth);
        if let Some((plte, trns)) = palette {
            enc.set_palette(plte.to_vec());
            enc.set_trns(trns.to_vec());
        }
        let mut w = enc.write_header().unwrap();
        w.write_image_data(data).unwrap();
        w.finish().unwrap();
        out
    }

    #[test]
    fn png_8_bit_rgba() {
        let data: Vec<u8> = (0..3 * 2 * 4).map(|i| i as u8 * 7).collect();
        let file = png(
            3,
            2,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            None,
            &data,
        );
        let (r, out) = read_all("a", file, 1);
        assert!(r.has_alpha());
        assert_eq!((r.width(), r.height()), (3, 2));
        assert_eq!(out, data);
    }

    #[test]
    fn png_16_bit_rgb() {
        let samples: [u16; 6] = [0, 65535, 32896, 257, 514, 65280];
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_be_bytes()).collect();
        let file = png(
            2,
            1,
            png::ColorType::Rgb,
            png::BitDepth::Sixteen,
            None,
            &data,
        );
        let (r, out) = read_all("a", file, 4);
        assert!(!r.has_alpha());
        assert_eq!(out, [0, 255, 128, 255, 1, 2, 255, 255]);
    }

    #[test]
    fn png_grey_alpha() {
        let file = png(
            2,
            1,
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            None,
            &[10, 0, 200, 255],
        );
        let (r, out) = read_all("a", file, 4);
        assert!(r.has_alpha());
        assert_eq!(out, [10, 10, 10, 0, 200, 200, 200, 255]);
    }

    #[test]
    fn png_palette_with_transparency() {
        let plte = [255, 0, 0, 0, 255, 0, 0, 0, 255];
        let trns = [0, 128];
        let file = png(
            3,
            1,
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            Some((&plte, &trns)),
            &[0, 1, 2],
        );
        let (r, out) = read_all("a", file, 4);
        assert!(r.has_alpha());
        assert_eq!(out, [255, 0, 0, 0, 0, 255, 0, 128, 0, 0, 255, 255]);
    }

    #[test]
    fn png_grey_without_alpha() {
        let file = png(
            2,
            1,
            png::ColorType::Grayscale,
            png::BitDepth::Eight,
            None,
            &[3, 4],
        );
        let (r, out) = read_all("a", file, 4);
        assert!(!r.has_alpha());
        assert_eq!(out, [3, 3, 3, 255, 4, 4, 4, 255]);
    }

    #[test]
    fn tiff_strips() {
        let (w, h) = (7u32, 20u32);
        let data: Vec<u8> = (0..w * h * 4).map(|i| (i * 3) as u8).collect();
        let mut file = Cursor::new(Vec::new());
        let mut enc = TiffEncoder::new(&mut file).unwrap();
        let mut image = enc.new_image::<colortype::RGBA8>(w, h).unwrap();
        image.rows_per_strip(3).unwrap();
        image.write_data(&data).unwrap();
        let (r, out) = read_all("a", file.into_inner(), 4);
        assert!(r.has_alpha());
        assert_eq!(out, data);
    }

    #[test]
    fn tiff_16_bit_grey() {
        let data = [0u16, 65535, 32896];
        let mut file = Cursor::new(Vec::new());
        TiffEncoder::new(&mut file)
            .unwrap()
            .write_image::<colortype::Gray16>(3, 1, &data)
            .unwrap();
        let (r, out) = read_all("a", file.into_inner(), 1);
        assert!(!r.has_alpha());
        assert_eq!(out, [0, 0, 0, 255, 255, 255, 255, 255, 128, 128, 128, 255]);
    }

    /// Builds an uncompressed little-endian tiled RGB8 TIFF.
    fn tiled_tiff(
        width: u32,
        height: u32,
        tile: u32,
        pixel: impl Fn(u32, u32) -> [u8; 3],
    ) -> Vec<u8> {
        let across = width.div_ceil(tile);
        let down = height.div_ceil(tile);
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        let header = 8u32;
        for ty in 0..down {
            for tx in 0..across {
                offsets.push(header + data.len() as u32);
                for y in 0..tile {
                    for x in 0..tile {
                        let (px, py) = (tx * tile + x, ty * tile + y);
                        if px < width && py < height {
                            data.extend_from_slice(&pixel(px, py));
                        } else {
                            data.extend_from_slice(&[0; 3]);
                        }
                    }
                }
            }
        }
        let count = offsets.len() as u32;
        let tile_bytes = tile * tile * 3;
        let mut f = Vec::new();
        f.extend_from_slice(b"II*\0");
        let extra = header + data.len() as u32;
        // Out-of-line values: bits per sample, offsets, byte counts.
        let bits_at = extra;
        let offsets_at = bits_at + 6;
        let counts_at = offsets_at + 4 * count;
        let ifd_at = counts_at + 4 * count;
        f.extend_from_slice(&ifd_at.to_le_bytes());
        f.extend_from_slice(&data);
        for _ in 0..3 {
            f.extend_from_slice(&8u16.to_le_bytes());
        }
        for o in &offsets {
            f.extend_from_slice(&o.to_le_bytes());
        }
        for _ in 0..count {
            f.extend_from_slice(&tile_bytes.to_le_bytes());
        }
        let entries: [(u16, u16, u32, u32); 10] = [
            (256, 4, 1, width),
            (257, 4, 1, height),
            (258, 3, 3, bits_at),
            (259, 3, 1, 1),
            (262, 3, 1, 2),
            (277, 3, 1, 3),
            (322, 4, 1, tile),
            (323, 4, 1, tile),
            (324, 4, count, offsets_at),
            (325, 4, count, counts_at),
        ];
        f.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, ty, n, v) in entries {
            f.extend_from_slice(&tag.to_le_bytes());
            f.extend_from_slice(&ty.to_le_bytes());
            f.extend_from_slice(&n.to_le_bytes());
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.extend_from_slice(&0u32.to_le_bytes());
        f
    }

    #[test]
    fn tiff_tiles() {
        let (w, h) = (40, 21);
        let pixel = |x: u32, y: u32| [x as u8, y as u8, (x + y) as u8];
        let file = tiled_tiff(w, h, 16, pixel);
        let (r, out) = read_all("a", file, 5);
        assert!(!r.has_alpha());
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let [a, b, c] = pixel(x, y);
                assert_eq!(&out[i..i + 4], &[a, b, c, 255], "pixel {x}, {y}");
            }
        }
    }

    #[test]
    fn tiff_one_huge_strip_is_an_error() {
        // A header that claims one strip of 8192 rows by 16384 RGBA pixels.
        let (w, h) = (16384u32, 8192u32);
        let mut f = Vec::new();
        f.extend_from_slice(b"II*\0");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&[8, 0, 8, 0, 8, 0, 8, 0]);
        let entries: [(u16, u16, u32, u32); 10] = [
            (256, 4, 1, w),
            (257, 4, 1, h),
            (258, 3, 4, 8),
            (259, 3, 1, 1),
            (262, 3, 1, 2),
            (273, 4, 1, 0),
            (277, 3, 1, 4),
            (278, 4, 1, h),
            (279, 4, 1, w * h * 4),
            (338, 3, 1, 2),
        ];
        f.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, ty, n, v) in entries {
            f.extend_from_slice(&tag.to_le_bytes());
            f.extend_from_slice(&ty.to_le_bytes());
            f.extend_from_slice(&n.to_le_bytes());
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.extend_from_slice(&0u32.to_le_bytes());
        let err = RowReader::from_bytes("big", f).err().unwrap().to_string();
        assert!(err.contains("smaller strips"), "{err}");
    }

    #[test]
    fn psd_by_magic() {
        let file = build_psd(
            false,
            true,
            3,
            8,
            2,
            1,
            &[vec![1, 2], vec![3, 4], vec![5, 6]],
        );
        let (r, out) = read_all("a.png", file, 8);
        assert!(!r.has_alpha());
        assert_eq!(out, [1, 3, 5, 255, 2, 4, 6, 255]);
    }

    #[test]
    fn not_an_image() {
        for bytes in [b"hello world".to_vec(), Vec::new(), b"GIF89a....".to_vec()] {
            let err = RowReader::from_bytes("notes", bytes).err().unwrap();
            assert!(matches!(err, ImportError::NotImage { .. }), "{err}");
            assert_eq!(err.to_string(), "notes is not a PNG, TIFF, or PSD image");
        }
        let err = RowReader::from_bytes("bad", b"\x89PNG\r\n\x1a\nxx".to_vec())
            .err()
            .unwrap();
        assert!(matches!(err, ImportError::Parse { .. }));
        assert!(RowReader::open(Path::new("/no/such/file.png")).is_err());
    }
}
