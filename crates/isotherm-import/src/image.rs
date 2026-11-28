//! The read of a PNG or TIFF heightmap.

use std::io::Cursor;

use tiff::ColorType;
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

use crate::{ImportError, Samples};

/// The tag of the GDAL metadata, which holds the offset and the scale.
const GDAL_METADATA: Tag = Tag::Unknown(42112);
/// The tag of the GDAL value of a pixel with no data.
const GDAL_NODATA: Tag = Tag::Unknown(42113);

/// The decoder allocates at most this many bytes for the pixels.
const MAX_ALLOC: usize = 4 * 1024 * 1024 * 1024;

/// Reads a PNG or TIFF image. The format comes from the content, not from
/// the file name.
///
/// A grey TIFF with signed or float samples, or with a GDAL offset or
/// scale, gives heights in meters. Each other image gives levels with no
/// unit: a 16-bit grey level stays as it is, an 8-bit level scales to the
/// full range, and a color image becomes its luma.
pub fn read_image(name: &str, bytes: &[u8]) -> Result<Samples, ImportError> {
    if is_tiff(bytes)
        && let Some(samples) = read_grey_tiff(name, bytes)?
    {
        return Ok(samples);
    }
    let image = vale_import::raster::read_bytes(name, bytes)?;
    Ok(Samples::Levels {
        width: image.width,
        height: image.height,
        levels: image.levels,
    })
}

fn is_tiff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"II*\0")
        || bytes.starts_with(b"MM\0*")
        || bytes.starts_with(b"II+\0")
        || bytes.starts_with(b"MM\0+")
}

/// Reads a TIFF image with one grey channel. `None`: the image has another
/// color type.
fn read_grey_tiff(name: &str, bytes: &[u8]) -> Result<Option<Samples>, ImportError> {
    let parse = |message: String| ImportError::Parse {
        name: name.to_string(),
        message,
    };
    let mut limits = Limits::default();
    limits.decoding_buffer_size = MAX_ALLOC;
    limits.intermediate_buffer_size = MAX_ALLOC;
    limits.ifd_value_size = 16 * 1024 * 1024;
    let mut decoder = Decoder::new(Cursor::new(bytes))
        .map_err(|e| parse(e.to_string()))?
        .with_limits(limits);
    if !matches!(decoder.colortype(), Ok(ColorType::Gray(_))) {
        return Ok(None);
    }
    let (width, height) = decoder.dimensions().map_err(|e| parse(e.to_string()))?;
    let metadata = decoder.get_tag_ascii_string(GDAL_METADATA).ok();
    let (offset, scale) = metadata.as_deref().map_or((None, None), gdal_offset_scale);
    let nodata = decoder
        .get_tag_ascii_string(GDAL_NODATA)
        .ok()
        .and_then(|text| text.trim().trim_end_matches('\0').parse::<f64>().ok());
    let image = decoder.read_image().map_err(|e| parse(e.to_string()))?;
    let (width, height) = (width as usize, height as usize);
    let values: Vec<f64> = match image {
        DecodingResult::U8(v) if offset.is_none() && scale.is_none() => {
            let levels = v.iter().map(|&x| u16::from(x) * 257).collect();
            return Ok(Some(Samples::Levels {
                width,
                height,
                levels,
            }));
        }
        DecodingResult::U16(levels) if offset.is_none() && scale.is_none() => {
            return Ok(Some(Samples::Levels {
                width,
                height,
                levels,
            }));
        }
        DecodingResult::U8(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U16(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::U64(v) => v.into_iter().map(|x| x as f64).collect(),
        DecodingResult::I8(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I16(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::I64(v) => v.into_iter().map(|x| x as f64).collect(),
        DecodingResult::F16(v) => v.into_iter().map(|x| x.to_f64()).collect(),
        DecodingResult::F32(v) => v.into_iter().map(f64::from).collect(),
        DecodingResult::F64(v) => v,
    };
    if values.len() != width * height {
        return Err(parse("the image has more than one sample per pixel".into()));
    }
    let (offset, scale) = (offset.unwrap_or(0.0), scale.unwrap_or(1.0));
    let meters = values.into_iter().map(|v| {
        let valid = v.is_finite() && Some(v) != nodata;
        valid.then_some(v * scale + offset)
    });
    meters_to_samples(width, height, meters)
        .ok_or(ImportError::Empty {
            name: name.to_string(),
        })
        .map(Some)
}

/// Spreads the levels over the heights from the lowest to the highest. A
/// pixel with no data gets the lowest height. `None`: no pixel has data.
fn meters_to_samples(
    width: usize,
    height: usize,
    meters: impl Iterator<Item = Option<f64>>,
) -> Option<Samples> {
    let meters: Vec<Option<f64>> = meters.collect();
    let (mut low_m, mut high_m) = (f64::INFINITY, f64::NEG_INFINITY);
    for m in meters.iter().flatten() {
        low_m = low_m.min(*m);
        high_m = high_m.max(*m);
    }
    if low_m > high_m {
        return None;
    }
    if low_m == high_m {
        high_m = low_m + 1.0;
    }
    let span = high_m - low_m;
    let levels = meters
        .into_iter()
        .map(|m| m.map_or(0, |m| ((m - low_m) / span * 65535.0).round() as u16))
        .collect();
    Some(Samples::Meters {
        width,
        height,
        levels,
        low_m,
        high_m,
    })
}

/// The offset and the scale of band 0 in GDAL metadata XML.
fn gdal_offset_scale(xml: &str) -> (Option<f64>, Option<f64>) {
    let (mut offset, mut scale) = (None, None);
    for item in xml.split("<Item").skip(1) {
        let Some((attributes, rest)) = item.split_once('>') else {
            continue;
        };
        let Some((text, _)) = rest.split_once("</Item") else {
            continue;
        };
        let attribute = |key: &str| {
            let start = attributes.find(&format!("{key}=\""))? + key.len() + 2;
            let len = attributes[start..].find('"')?;
            Some(attributes[start..start + len].to_ascii_lowercase())
        };
        if attribute("sample").is_some_and(|s| s != "0") {
            continue;
        }
        let role = attribute("role").or_else(|| attribute("name"));
        let value = text.trim().parse::<f64>().ok().filter(|v| v.is_finite());
        match role.as_deref() {
            Some("offset") => offset = value.or(offset),
            Some("scale") => scale = value.or(scale),
            _ => {}
        }
    }
    (offset, scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gdal_metadata_gives_the_offset_and_the_scale() {
        let xml = "<GDALMetadata>\n  \
             <Item name=\"OFFSET\" sample=\"0\" role=\"offset\">-6000</Item>\n  \
             <Item name=\"SCALE\" sample=\"0\" role=\"scale\">0.5</Item>\n  \
             <Item name=\"UNITTYPE\" sample=\"0\" role=\"unittype\">m</Item>\n\
             </GDALMetadata>\n";
        assert_eq!(gdal_offset_scale(xml), (Some(-6000.0), Some(0.5)));
        assert_eq!(gdal_offset_scale("<GDALMetadata/>"), (None, None));
    }
}
