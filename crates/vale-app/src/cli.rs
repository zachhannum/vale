//! Command-line arguments.

use std::path::PathBuf;

use anyhow::{Context, anyhow, bail};
use clap::Parser;
use vale_sphere::{ProjectionKind, ProjectionSpec};

use crate::document::Document;
use crate::pipeline::Pipeline;

#[derive(Parser, Clone, Debug)]
#[command(
    name = "vale-app",
    version,
    about = "Vale: a desktop GIS for fictional worlds"
)]
pub struct Args {
    /// GeoJSON files to open in place of the sample world.
    #[arg(value_name = "FILES")]
    pub files: Vec<PathBuf>,
    /// equal-earth, mercator, lambert-azimuthal, orthographic, or stereographic.
    #[arg(long, value_name = "ID", default_value = "equal-earth")]
    pub projection: String,
    /// Center of the projection.
    #[arg(long, value_name = "LON,LAT", allow_hyphen_values = true)]
    pub center: Option<String>,
    /// Radius of the world in kilometers.
    #[arg(long, value_name = "KM", default_value_t = 6371.0)]
    pub radius_km: f64,
    /// Put this place at the middle of the view.
    #[arg(long, value_name = "LON,LAT", allow_hyphen_values = true)]
    pub look_at: Option<String>,
    /// Zoom factor on the fitted view.
    #[arg(long, value_name = "F", default_value_t = 1.0)]
    pub zoom: f64,
    /// Size of the window or of the image, in points.
    #[arg(long, value_name = "WxH", default_value = "1280x800")]
    pub size: String,
    /// Pixels per point of headless output.
    #[arg(long, value_name = "F", default_value_t = 1.0)]
    pub pixel_ratio: f64,
    /// Hide the layer with this name.
    #[arg(long, value_name = "LAYER")]
    pub hide: Vec<String>,
    /// Turn the labels off.
    #[arg(long)]
    pub no_labels: bool,
    /// Turn the graticule off.
    #[arg(long)]
    pub no_graticule: bool,
    /// Headless: write a PNG and exit.
    #[arg(long, value_name = "PNG")]
    pub screenshot: Option<PathBuf>,
    /// With --screenshot: the map without the UI.
    #[arg(long)]
    pub map_only: bool,
    /// Headless: export the map to .png or .pdf and exit.
    #[arg(long, value_name = "FILE")]
    pub export: Option<PathBuf>,
    /// Headless: print the color of the map pixel at this place.
    #[arg(long, value_name = "LON,LAT", allow_hyphen_values = true)]
    pub probe: Vec<String>,
    /// Print the world, the layers, and the label counts.
    #[arg(long)]
    pub report: bool,
    /// Open the window, draw N frames, and exit with code 0.
    #[arg(long, value_name = "N")]
    pub smoke_frames: Option<u32>,
}

/// Reads `lon,lat`.
pub fn parse_pair(s: &str) -> anyhow::Result<[f64; 2]> {
    let (a, b) = s
        .split_once(',')
        .ok_or_else(|| anyhow!("expected LON,LAT, got `{s}`"))?;
    let num = |t: &str| -> anyhow::Result<f64> {
        let v: f64 = t
            .trim()
            .parse()
            .with_context(|| format!("`{t}` is not a number in `{s}`"))?;
        if !v.is_finite() {
            bail!("`{t}` is not finite in `{s}`");
        }
        Ok(v)
    };
    Ok([num(a)?, num(b)?])
}

impl Args {
    pub fn document(&self) -> anyhow::Result<Document> {
        let mut doc = if self.files.is_empty() {
            Document::sample()
        } else {
            let mut doc = Document::empty();
            for f in &self.files {
                doc.open_geojson(f)
                    .map_err(|e| anyhow!("{}: {e}", f.display()))?;
            }
            doc
        };
        doc.set_radius_km(self.radius_km);

        let kind = ProjectionKind::from_id(&self.projection).ok_or_else(|| {
            let ids: Vec<&str> = ProjectionKind::ALL.iter().map(|k| k.id()).collect();
            anyhow!(
                "unknown projection `{}`; use one of {}",
                self.projection,
                ids.join(", ")
            )
        })?;
        let [lon0, lat0] = match &self.center {
            Some(c) => parse_pair(c)?,
            None => [0.0, 0.0],
        };
        doc.set_projection(ProjectionSpec { kind, lon0, lat0 });

        for name in &self.hide {
            let id = doc
                .project
                .layers()
                .iter()
                .find(|l| &l.name == name)
                .map(|l| l.id);
            match id {
                Some(id) => doc.entry_mut(id).expect("every layer has an entry").visible = false,
                None => {
                    let names: Vec<&str> = doc
                        .project
                        .layers()
                        .iter()
                        .map(|l| l.name.as_str())
                        .collect();
                    bail!(
                        "unknown layer `{name}`; the layers are {}",
                        names.join(", ")
                    );
                }
            }
        }
        if self.no_labels {
            doc.frame.labels = false;
        }
        if self.no_graticule {
            doc.frame.graticule = false;
        }
        Ok(doc)
    }

    pub fn size(&self) -> anyhow::Result<(f64, f64)> {
        let (w, h) = self
            .size
            .split_once(['x', 'X'])
            .ok_or_else(|| anyhow!("expected WxH, got `{}`", self.size))?;
        let w: f64 = w
            .trim()
            .parse()
            .with_context(|| format!("bad width in `{}`", self.size))?;
        let h: f64 = h
            .trim()
            .parse()
            .with_context(|| format!("bad height in `{}`", self.size))?;
        if !(w >= 1.0 && h >= 1.0 && w.is_finite() && h.is_finite()) {
            bail!("the size must be at least 1x1, got `{}`", self.size);
        }
        Ok((w, h))
    }
}

/// Applies `--zoom` and `--look-at` to the view of the document.
pub fn apply_view(
    args: &Args,
    doc: &mut Document,
    pipeline: &mut Pipeline,
    size: (f64, f64),
) -> anyhow::Result<()> {
    if args.zoom == 1.0 && args.look_at.is_none() {
        return Ok(());
    }
    if !(args.zoom.is_finite() && args.zoom > 0.0) {
        bail!("the zoom must be above zero");
    }
    let mut view = pipeline.resolve_view(doc, size)?;
    if let Some(spec) = &args.look_at {
        let at = parse_pair(spec)?;
        view.center = pipeline
            .project_lonlat(doc, at)?
            .ok_or_else(|| anyhow!("the place {spec} is not visible in this projection"))?;
    }
    view.scale *= args.zoom;
    doc.frame.view = Some(view);
    Ok(())
}
