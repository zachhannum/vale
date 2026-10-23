//! GeoTIFF export of a raster layer.

use std::fmt;
use std::fs::{self, File};
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use std::thread;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use tiff::encoder::TiffEncoder;
use tiff::tags::Tag;

/// The number of rows in one strip of the file.
pub const ROWS_PER_STRIP: u32 = 16;

/// The tag of the GDAL metadata, which holds the offset, the scale, and the unit.
const GDAL_METADATA: Tag = Tag::Unknown(42112);

/// The CRS name of a world with an empty name.
const FALLBACK_NAME: &str = "World";

/// A GeoTIFF export failure.
#[derive(Debug)]
pub enum GeoTiffError {
    Io { path: PathBuf, message: String },
    Write { message: String },
    Empty,
    TooLarge,
}

impl fmt::Display for GeoTiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeoTiffError::Io { path, message } => {
                write!(f, "cannot write {}: {message}", path.display())
            }
            GeoTiffError::Write { message } => write!(f, "cannot write the GeoTIFF: {message}"),
            GeoTiffError::Empty => write!(f, "the image has no pixels"),
            GeoTiffError::TooLarge => {
                write!(f, "the file is larger than 4 GB, the limit of a TIFF file")
            }
        }
    }
}

impl std::error::Error for GeoTiffError {}

impl From<tiff::TiffError> for GeoTiffError {
    fn from(e: tiff::TiffError) -> Self {
        GeoTiffError::Write {
            message: e.to_string(),
        }
    }
}

impl From<std::io::Error> for GeoTiffError {
    fn from(e: std::io::Error) -> Self {
        GeoTiffError::Write {
            message: e.to_string(),
        }
    }
}

/// The size and the tags of an equirectangular image of the full world.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoTiff {
    pub width: u32,
    pub height: u32,
    /// The name of the CRS, the datum, and the sphere.
    pub crs_name: String,
    /// The radius of the sphere, in meters.
    pub radius_m: f64,
    /// The value of level 0.
    pub value_min: f64,
    /// The value of level 65,535.
    pub value_max: f64,
    /// The unit of the values, for example `m`.
    pub unit: String,
}

impl GeoTiff {
    /// With no raster record: -6000, 6000, "m".
    pub fn new(
        world: &vale_store::World,
        raster: Option<&vale_store::Raster>,
        width: u32,
        height: u32,
    ) -> GeoTiff {
        let (value_min, value_max, unit) = match raster {
            Some(r) => (r.value_min, r.value_max, r.unit.clone()),
            None => (-6000.0, 6000.0, "m".to_string()),
        };
        GeoTiff {
            width,
            height,
            crs_name: world.name.clone(),
            radius_m: world.radius_km * 1000.0,
            value_min,
            value_max,
            unit,
        }
    }

    /// The CRS name in the form that the ASCII tag can hold.
    fn ascii_name(&self) -> String {
        if self.crs_name.is_empty() {
            return FALLBACK_NAME.to_string();
        }
        self.crs_name
            .chars()
            .map(|c| {
                if c.is_ascii() && c != '|' && c != '\0' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    }

    /// The ASCII parameters and the key directory of a geographic CRS on a sphere.
    fn geo_keys(&self) -> (String, Vec<u16>) {
        let name = self.ascii_name();
        let citation = format!("{name}|");
        let geog =
            format!("GCS Name = {name}|Datum = {name}|Ellipsoid = {name}|Primem = Greenwich|");
        // A world name is short, so the lengths fit in 16 bits.
        let a = citation.len() as u16;
        let b = geog.len() as u16;
        #[rustfmt::skip]
        let keys = vec![
            1, 1, 0, 11,
            1024, 0, 1, 2, // The model is geographic.
            1025, 0, 1, 1, // A pixel is an area.
            1026, 34737, a, 0, // The citation.
            2048, 0, 1, 32767, // The CRS is user-defined.
            2049, 34737, b, a, // The names of the CRS parts.
            2050, 0, 1, 32767, // The datum is user-defined.
            2054, 0, 1, 9102, // The angular unit is the degree.
            2056, 0, 1, 32767, // The ellipsoid is user-defined.
            2057, 34736, 1, 0, // The semi-major axis.
            2058, 34736, 1, 1, // The semi-minor axis.
            2061, 34736, 1, 2, // The longitude of the prime meridian.
        ];
        (citation + &geog, keys)
    }

    /// The GDAL metadata: a level times the scale plus the offset is a value.
    fn metadata(&self) -> String {
        let scale = (self.value_max - self.value_min) / 65535.0;
        let mut xml = String::from("<GDALMetadata>\n");
        xml += &format!(
            "  <Item name=\"OFFSET\" sample=\"0\" role=\"offset\">{}</Item>\n",
            self.value_min
        );
        xml += &format!("  <Item name=\"SCALE\" sample=\"0\" role=\"scale\">{scale}</Item>\n");
        if !self.unit.is_empty() {
            xml += &format!(
                "  <Item name=\"UNITTYPE\" sample=\"0\" role=\"unittype\">{}</Item>\n",
                escape_xml(&self.unit)
            );
        }
        xml += "</GDALMetadata>\n";
        xml
    }
}

/// Escapes text for XML. A character outside ASCII becomes a character
/// reference, because a TIFF text tag holds ASCII only.
fn escape_xml(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '&' => out += "&amp;",
            '<' => out += "&lt;",
            '>' => out += "&gt;",
            '"' => out += "&quot;",
            '\'' => out += "&apos;",
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c => out += &format!("&#x{:X};", c as u32),
        }
    }
    out
}

/// The offset as a classic TIFF holds it.
fn offset_32(offset: u64) -> Result<u32, GeoTiffError> {
    u32::try_from(offset).map_err(|_| GeoTiffError::TooLarge)
}

/// Fills the rows of one strip, applies the horizontal predictor, and compresses them.
fn pack_strip(
    strip: u32,
    tags: &GeoTiff,
    rows: &(dyn Fn(usize, &mut [u16]) + Sync),
) -> std::io::Result<Vec<u8>> {
    let first = strip * ROWS_PER_STRIP;
    let last = (first + ROWS_PER_STRIP).min(tags.height);
    let mut row = vec![0u16; tags.width as usize];
    let mut bytes = vec![0u8; row.len() * 2];
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    for j in first..last {
        rows(j as usize, &mut row);
        for i in (1..row.len()).rev() {
            row[i] = row[i].wrapping_sub(row[i - 1]);
        }
        // The encoder writes the header in the byte order of the machine.
        for (i, v) in row.iter().enumerate() {
            bytes[2 * i..2 * i + 2].copy_from_slice(&v.to_ne_bytes());
        }
        encoder.write_all(&bytes)?;
    }
    encoder.finish()
}

/// Writes a GeoTIFF with one band of 16-bit levels.
///
/// `rows(j, out)` fills row j (row 0 is the north edge). `progress(rows_done)` runs after each batch.
pub fn write<W: Write + Seek>(
    mut out: W,
    tags: &GeoTiff,
    rows: &(dyn Fn(usize, &mut [u16]) + Sync),
    progress: &(dyn Fn(u32) + Sync),
) -> Result<(), GeoTiffError> {
    if tags.width == 0 || tags.height == 0 {
        return Err(GeoTiffError::Empty);
    }
    let strips = tags.height.div_ceil(ROWS_PER_STRIP);
    let threads = thread::available_parallelism().map_or(1, |n| n.get() as u32);

    let mut encoder = TiffEncoder::new(&mut out)?;
    let mut dir = encoder.image_directory()?;
    let mut offsets = Vec::with_capacity(strips as usize);
    let mut counts = Vec::with_capacity(strips as usize);
    let mut first = 0;
    while first < strips {
        let end = strips.min(first.saturating_add(threads));
        // The memory holds the strips of one batch only.
        let batch: Vec<std::io::Result<Vec<u8>>> = thread::scope(|scope| {
            let handles: Vec<_> = (first..end)
                .map(|strip| scope.spawn(move || pack_strip(strip, tags, rows)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p)))
                .collect()
        });
        for packed in batch {
            let packed = packed?;
            let offset = dir.write_data(&packed[..])?;
            offset_32(offset + packed.len() as u64)?;
            offsets.push(offset_32(offset)?);
            counts.push(packed.len() as u32);
        }
        first = end;
        progress((first * ROWS_PER_STRIP).min(tags.height));
    }

    let (ascii, keys) = tags.geo_keys();
    let metadata = tags.metadata();
    // The tag values and the directory come after the strips.
    let tail = 8 * strips as u64 + (ascii.len() + metadata.len() + keys.len() * 2) as u64 + 1024;
    let data_end = offsets.last().zip(counts.last());
    if let Some((&offset, &count)) = data_end {
        offset_32(offset as u64 + count as u64 + tail)?;
    }

    dir.write_tag(Tag::ImageWidth, tags.width)?;
    dir.write_tag(Tag::ImageLength, tags.height)?;
    dir.write_tag(Tag::BitsPerSample, &[16u16][..])?;
    dir.write_tag(Tag::Compression, 8u16)?;
    dir.write_tag(Tag::PhotometricInterpretation, 1u16)?;
    dir.write_tag(Tag::StripOffsets, &offsets[..])?;
    dir.write_tag(Tag::SamplesPerPixel, 1u16)?;
    dir.write_tag(Tag::RowsPerStrip, ROWS_PER_STRIP)?;
    dir.write_tag(Tag::StripByteCounts, &counts[..])?;
    dir.write_tag(Tag::PlanarConfiguration, 1u16)?;
    dir.write_tag(Tag::Predictor, 2u16)?;
    dir.write_tag(Tag::SampleFormat, &[1u16][..])?;
    let scale = [
        360.0 / tags.width as f64,
        180.0 / tags.height as f64,
        0.0f64,
    ];
    dir.write_tag(Tag::ModelPixelScaleTag, &scale[..])?;
    // Pixel corner (0, 0) is at longitude -180 and latitude 90.
    let tiepoint = [0.0f64, 0.0, 0.0, -180.0, 90.0, 0.0];
    dir.write_tag(Tag::ModelTiepointTag, &tiepoint[..])?;
    dir.write_tag(Tag::GeoKeyDirectoryTag, &keys[..])?;
    let doubles = [tags.radius_m, tags.radius_m, 0.0f64];
    dir.write_tag(Tag::GeoDoubleParamsTag, &doubles[..])?;
    dir.write_tag(Tag::GeoAsciiParamsTag, ascii.as_str())?;
    dir.write_tag(GDAL_METADATA, metadata.as_str())?;
    dir.finish()?;
    out.flush()?;
    Ok(())
}

/// Writes a GeoTIFF file. See [`write`].
///
/// Makes the parent folders, and removes the file if the write fails.
pub fn write_file(
    path: &Path,
    tags: &GeoTiff,
    rows: &(dyn Fn(usize, &mut [u16]) + Sync),
    progress: &(dyn Fn(u32) + Sync),
) -> Result<(), GeoTiffError> {
    let io = |e: std::io::Error| GeoTiffError::Io {
        path: path.to_path_buf(),
        message: e.to_string(),
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    let file = File::create(path).map_err(io)?;
    let result = write(BufWriter::new(file), tags, rows, progress);
    if let Err(e) = result {
        let _ = fs::remove_file(path);
        return Err(match e {
            GeoTiffError::Write { message } => GeoTiffError::Io {
                path: path.to_path_buf(),
                message,
            },
            e => e,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Cursor;
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    use tiff::ColorType;
    use tiff::decoder::{Decoder, DecodingResult};

    /// A level that changes along the row and from row to row, with wraps.
    fn level(i: usize, j: usize) -> u16 {
        (i * 7919 + j * 104729 + (i * j) % 251) as u16
    }

    fn pattern(j: usize, out: &mut [u16]) {
        for (i, v) in out.iter_mut().enumerate() {
            *v = level(i, j);
        }
    }

    fn expected(width: u32, height: u32) -> Vec<u16> {
        (0..height as usize)
            .flat_map(|j| (0..width as usize).map(move |i| level(i, j)))
            .collect()
    }

    fn tags(width: u32, height: u32) -> GeoTiff {
        GeoTiff::new(&vale_store::World::default(), None, width, height)
    }

    fn bytes(tags: &GeoTiff) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        write(&mut out, tags, &pattern, &|_| {}).unwrap();
        out.into_inner()
    }

    /// A height that needs more than one batch and ends in a short strip.
    fn tall() -> u32 {
        let threads = thread::available_parallelism().map_or(1, |n| n.get() as u32);
        ROWS_PER_STRIP * (2 * threads + 1) + 5
    }

    #[test]
    fn file_reads_back_with_the_same_samples_and_tags() {
        for (width, height) in [(37, 53), (5, tall()), (1, 1)] {
            let tags = tags(width, height);
            let mut d = Decoder::new(Cursor::new(bytes(&tags))).unwrap();
            assert_eq!(d.dimensions().unwrap(), (width, height));
            assert_eq!(d.colortype().unwrap(), ColorType::Gray(16));
            assert_eq!(d.get_tag_u32(Tag::Compression).unwrap(), 8);
            assert_eq!(d.get_tag_u32(Tag::Predictor).unwrap(), 2);
            assert_eq!(d.get_tag_u32(Tag::RowsPerStrip).unwrap(), ROWS_PER_STRIP);
            assert_eq!(d.get_tag_u32(Tag::SampleFormat).unwrap(), 1);
            assert_eq!(
                d.get_tag_u32_vec(Tag::StripOffsets).unwrap().len() as u32,
                height.div_ceil(ROWS_PER_STRIP)
            );
            match d.read_image().unwrap() {
                DecodingResult::U16(levels) => assert_eq!(levels, expected(width, height)),
                _ => panic!("the samples are not 16-bit"),
            }
        }
    }

    #[test]
    fn geo_tags_place_the_image_on_the_sphere_of_the_world() {
        let tags = tags(37, 53);
        let mut d = Decoder::new(Cursor::new(bytes(&tags))).unwrap();
        assert_eq!(
            d.get_tag_f64_vec(Tag::ModelPixelScaleTag).unwrap(),
            [360.0 / 37.0, 180.0 / 53.0, 0.0]
        );
        assert_eq!(
            d.get_tag_f64_vec(Tag::ModelTiepointTag).unwrap(),
            [0.0, 0.0, 0.0, -180.0, 90.0, 0.0]
        );
        assert_eq!(
            d.get_tag_f64_vec(Tag::GeoDoubleParamsTag).unwrap(),
            [6371000.0, 6371000.0, 0.0]
        );
        assert_eq!(
            d.get_tag_ascii_string(Tag::GeoAsciiParamsTag).unwrap(),
            "New world|GCS Name = New world|Datum = New world|Ellipsoid = New world|Primem = Greenwich|"
        );
        #[rustfmt::skip]
        let keys = [
            1, 1, 0, 11,
            1024, 0, 1, 2,
            1025, 0, 1, 1,
            1026, 34737, 10, 0,
            2048, 0, 1, 32767,
            2049, 34737, 80, 10,
            2050, 0, 1, 32767,
            2054, 0, 1, 9102,
            2056, 0, 1, 32767,
            2057, 34736, 1, 0,
            2058, 34736, 1, 1,
            2061, 34736, 1, 2,
        ];
        assert_eq!(d.get_tag_u16_vec(Tag::GeoKeyDirectoryTag).unwrap(), keys);
    }

    #[test]
    fn metadata_gives_the_offset_the_scale_and_the_unit() {
        let mut d = Decoder::new(Cursor::new(bytes(&tags(4, 2)))).unwrap();
        assert_eq!(
            d.get_tag_ascii_string(GDAL_METADATA).unwrap(),
            "<GDALMetadata>\n  \
             <Item name=\"OFFSET\" sample=\"0\" role=\"offset\">-6000</Item>\n  \
             <Item name=\"SCALE\" sample=\"0\" role=\"scale\">0.18310826276035705</Item>\n  \
             <Item name=\"UNITTYPE\" sample=\"0\" role=\"unittype\">m</Item>\n\
             </GDALMetadata>\n"
        );
    }

    #[test]
    fn raster_record_gives_the_value_range_and_the_unit() {
        let world = vale_store::World {
            radius_km: 1200.5,
            ..Default::default()
        };
        let raster = vale_store::Raster {
            id: vale_store::RasterId(1),
            face_size: 16,
            display: vale_store::RasterDisplay::Bands,
            smoothing: vale_store::Smoothing::default(),
            hillshade: vale_store::Hillshade::default(),
            rivers: true,
            unit: "<°C & more>".to_string(),
            value_min: -40.0,
            value_max: 25.535,
            base: 0,
            stops: Vec::new(),
            colors: Vec::new(),
            opacity: 1.0,
        };
        let mut tags = GeoTiff::new(&world, Some(&raster), 4, 2);
        assert_eq!(tags.radius_m, 1200500.0);
        let xml = tags.metadata();
        assert!(xml.contains(">-40</Item>"), "{xml}");
        assert!(xml.contains("role=\"scale\">0.001</Item>"), "{xml}");
        assert!(xml.contains(">&lt;&#xB0;C &amp; more&gt;</Item>"), "{xml}");
        let mut d = Decoder::new(Cursor::new(bytes(&tags))).unwrap();
        assert_eq!(d.get_tag_ascii_string(GDAL_METADATA).unwrap(), xml);

        tags.unit.clear();
        assert!(!tags.metadata().contains("UNITTYPE"));
    }

    #[test]
    fn import_reads_the_levels_of_its_own_export() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/folder/world.tif");
        write_file(&path, &tags(37, 53), &pattern, &|_| {}).unwrap();
        let g = crate::raster::read_file(&path).unwrap();
        assert_eq!((g.width, g.height), (37, 53));
        assert_eq!(g.levels, expected(37, 53));
    }

    #[test]
    fn world_name_becomes_ascii_with_matching_lengths() {
        for (name, ascii) in [("Żeł|a\0 é", "_e__a_ _"), ("", "World")] {
            let mut tags = tags(4, 2);
            tags.crs_name = name.to_string();
            let mut d = Decoder::new(Cursor::new(bytes(&tags))).unwrap();
            let params = d.get_tag_ascii_string(Tag::GeoAsciiParamsTag).unwrap();
            assert!(params.is_ascii());
            let keys = d.get_tag_u16_vec(Tag::GeoKeyDirectoryTag).unwrap();
            let text = |id: u16| {
                let key = keys.chunks(4).find(|k| k[0] == id).unwrap();
                assert_eq!(key[1], 34737);
                &params[key[3] as usize..(key[3] + key[2]) as usize]
            };
            assert_eq!(text(1026), format!("{ascii}|"));
            assert_eq!(
                text(2049),
                format!(
                    "GCS Name = {ascii}|Datum = {ascii}|Ellipsoid = {ascii}|Primem = Greenwich|"
                )
            );
            assert_eq!(text(1026).len() + text(2049).len(), params.len());
        }
    }

    #[test]
    fn failed_write_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.tif");
        let result = write_file(&path, &tags(4, 0), &pattern, &|_| {});
        assert!(matches!(result, Err(GeoTiffError::Empty)));
        assert!(!path.exists());
    }

    #[test]
    fn offset_past_4_gb_is_an_error() {
        assert_eq!(offset_32(u32::MAX as u64).unwrap(), u32::MAX);
        let e = offset_32(u32::MAX as u64 + 1).unwrap_err();
        assert!(matches!(e, GeoTiffError::TooLarge));
        assert!(e.to_string().contains("4 GB"));
    }

    #[test]
    fn progress_reaches_the_height() {
        let height = tall();
        let last = AtomicU32::new(0);
        let calls = AtomicU32::new(0);
        let progress = |rows: u32| {
            assert!(rows > last.swap(rows, Ordering::Relaxed));
            calls.fetch_add(1, Ordering::Relaxed);
        };
        write(
            Cursor::new(Vec::new()),
            &tags(5, height),
            &pattern,
            &progress,
        )
        .unwrap();
        assert_eq!(last.load(Ordering::Relaxed), height);
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn gdalinfo_reads_the_place_and_the_value_range() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("world.tif");
        write_file(&path, &tags(64, 32), &pattern, &|_| {}).unwrap();
        // A machine without GDAL skips the test.
        let Ok(output) = Command::new("gdalinfo").arg(&path).output() else {
            return;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{text}");
        for part in [
            "Origin = (-180",
            "Pixel Size = (5.625",
            "Type=UInt16",
            "Offset: -6000",
            "6371000",
        ] {
            assert!(text.contains(part), "no {part:?} in:\n{text}");
        }
    }
}
