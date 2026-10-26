//! Command-line arguments.

use std::path::PathBuf;

use anyhow::{Context, anyhow, bail};
use clap::Parser;
use vale_sphere::{ProjectionKind, ProjectionSpec};

use crate::document::Document;
use crate::globe::Globe;
use crate::globe::WorldView;
use crate::globe::export::{Exported, write_heightmap};
use crate::globe::flat::{self, FlatView};
use crate::globe::view::{GlobeView, ZOOM_MAX, ZOOM_MIN};
use crate::pipeline::Pipeline;
use crate::projects::{self, Locations};
use crate::session::Session;
use crate::ui::pad::Panel;
use crate::ui::world_form::FormMode;
use crate::ui::{Action, AppState, Layout, Workspace, is_height};

#[derive(Parser, Clone, Debug)]
#[command(
    name = "vale-app",
    version,
    about = "Vale: a desktop GIS for fictional worlds"
)]
pub struct Args {
    /// The workspace that the app opens.
    #[arg(long, value_enum, default_value_t)]
    pub workspace: Workspace,
    /// The view of the world in the globe workspace.
    #[arg(long, value_enum, default_value_t)]
    pub view: WorldView,
    /// The projection of the flat view: equirectangular, or an ID of --projection.
    #[arg(long, value_name = "ID", default_value = "equirectangular")]
    pub flat_projection: String,
    /// Center of the projection of the flat view.
    #[arg(long, value_name = "LON,LAT", allow_hyphen_values = true)]
    pub flat_center: Option<String>,
    /// The arrangement of the controls. The iPad uses `pad`.
    #[arg(long, value_enum, default_value_t)]
    pub layout: Layout,
    /// With --layout pad: open this panel. You can give more than one.
    #[arg(long, value_enum, value_name = "PANEL")]
    pub panel: Vec<Panel>,
    /// With --layout pad: open the menu of the More cell.
    #[arg(long)]
    pub more_tools: bool,
    /// With --layout pad: open the menu of the worlds.
    #[arg(long)]
    pub worlds_menu: bool,
    /// Open the form of a new raster layer.
    #[arg(long)]
    pub raster_form: bool,
    /// With --layout pad and --panel layers: open the page of the color stops
    /// of the selected raster layer.
    #[arg(long)]
    pub stops_page: bool,
    /// With --layout pad and --panel layers: open the page of the hillshade
    /// of the selected height layer, and turn the hillshade on.
    #[arg(long)]
    pub hillshade_page: bool,
    /// Open the form of a new world or of the world of the project.
    #[arg(long, value_enum, value_name = "FORM")]
    pub world_form: Option<FormMode>,
    /// GeoJSON files to open in place of the sample world.
    #[arg(value_name = "FILES")]
    pub files: Vec<PathBuf>,
    /// The project file. The world and the layers come from this file, and
    /// the window saves each change to it. If no file is there, the app
    /// makes it from the sample world or from FILES.
    #[arg(long, value_name = "GPKG")]
    pub project: Option<PathBuf>,
    /// The folder of the project files of the start screen and of the File
    /// menu. Without it, the folder is that of --project, or the data
    /// folder of the system.
    #[arg(long, value_name = "DIR")]
    pub projects_dir: Option<PathBuf>,
    /// A second folder of project files, which the app shows as the iCloud
    /// Drive folder.
    #[arg(long, value_name = "DIR")]
    pub cloud_dir: Option<PathBuf>,
    /// Show the start screen. The window shows it without this flag if
    /// there is no --project and no FILES.
    #[arg(long)]
    pub start_screen: bool,
    /// equirectangular, equal-earth, mercator, lambert-azimuthal, orthographic, or stereographic.
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
    /// Texels on one edge of a cube face of a raster layer that the app
    /// makes with no choice of the user. The window uses 8192 and headless
    /// output uses 1024, or less if the GPU has a lower limit.
    #[arg(long, value_name = "N")]
    pub face_size: Option<u32>,
    /// Import this equirectangular PNG or TIFF heightmap into a height
    /// layer. A world with no height layer gets one.
    #[arg(long, value_name = "FILE")]
    pub import_heightmap: Option<PathBuf>,
    /// Add the blocks of color of this legend image to the color ramps,
    /// and give the selected raster layer one band for each block.
    #[arg(long, value_name = "FILE")]
    pub import_ramp: Option<PathBuf>,
    /// Headless: export the first height layer of the project to a GeoTIFF
    /// file and exit.
    #[arg(long, value_name = "FILE", requires = "project")]
    pub export_heightmap: Option<PathBuf>,
    /// Headless: paint a fixed stroke on the globe, print the stroke delay,
    /// and exit.
    #[arg(long)]
    pub stroke_test: bool,
    /// Headless: zoom the globe in and out, print the frame times, and
    /// exit.
    #[arg(long)]
    pub zoom_test: bool,
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
    /// The projects folder: `--projects-dir`, or the folder of `--project`.
    /// With a start screen, it is else the folder of the system.
    pub fn projects_folder(&self, window: bool) -> Option<PathBuf> {
        let beside = self.project.as_ref().map(|path| match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
            _ => ".".into(),
        });
        let dir = self.projects_dir.clone().or(beside);
        match dir {
            None if self.starts_on_projects(window) => projects::default_dir(),
            dir => dir,
        }
    }

    /// The folders of the projects. With no flag, a start screen has the
    /// folders of the system.
    pub fn locations(&self, window: bool) -> Locations {
        let flags = self.projects_dir.is_some() || self.project.is_some();
        let mut locations = match self.projects_folder(window) {
            Some(dir) if flags => Locations::single(dir),
            Some(_) => projects::default_locations(),
            None => Locations::default(),
        };
        if self.cloud_dir.is_some() {
            locations.set_cloud(self.cloud_dir.clone());
        }
        locations
    }

    /// True if the app opens on the start screen. `window`: the run has a
    /// window.
    pub fn starts_on_projects(&self, window: bool) -> bool {
        self.start_screen || (window && self.project.is_none() && self.files.is_empty())
    }

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

        doc.set_projection(projection_spec(&self.projection, self.center.as_deref())?);

        for name in &self.hide {
            let id = doc
                .project
                .layers()
                .iter()
                .find(|l| &l.name == name)
                .map(|l| l.id);
            match id {
                Some(id) => _ = doc.set_visible(id, false),
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

/// The projection with this ID and this center.
fn projection_spec(id: &str, center: Option<&str>) -> anyhow::Result<ProjectionSpec> {
    let kind = ProjectionKind::from_id(id).ok_or_else(|| {
        let ids: Vec<&str> = ProjectionKind::ALL.iter().map(|k| k.id()).collect();
        anyhow!("unknown projection `{id}`; use one of {}", ids.join(", "))
    })?;
    let [lon0, lat0] = match center {
        Some(c) => parse_pair(c)?,
        None => [0.0, 0.0],
    };
    Ok(ProjectionSpec::new(kind, lon0, lat0))
}

impl Args {
    /// The projection of the flat view, from `--flat-projection` and
    /// `--flat-center`.
    pub fn flat_projection_spec(&self) -> anyhow::Result<ProjectionSpec> {
        projection_spec(&self.flat_projection, self.flat_center.as_deref())
    }
}

/// The limits of `--face-size`.
const FACE_SIZES: std::ops::RangeInclusive<u32> = 16..=16384;

/// Applies `--face-size`, `--view`, `--look-at`, and `--zoom` to the globe.
/// `face_size` is the face size without the flag. The globe must hold no
/// raster layer.
pub fn apply_globe(args: &Args, globe: &mut Globe, face_size: usize) -> anyhow::Result<()> {
    let face_size = match args.face_size {
        Some(n) if FACE_SIZES.contains(&n) => n as usize,
        Some(n) => bail!(
            "the face size must be from {} to {}, got {n}",
            FACE_SIZES.start(),
            FACE_SIZES.end()
        ),
        None => face_size,
    };
    if face_size != globe.new_face_size() {
        globe.set_face_size(face_size);
    }
    globe.world_view = args.view;
    globe.flat = FlatView::new(args.flat_projection_spec()?);
    globe.flat.zoom = args.zoom.clamp(flat::ZOOM_MIN, flat::ZOOM_MAX);
    if let Some(spec) = &args.look_at {
        let [lon, lat] = parse_pair(spec)?;
        globe.flat.look_at(lon, lat);
    }
    apply_globe_view(args, &mut globe.view)
}

/// Applies `--import-heightmap` and `--import-ramp`. `wait`: the imports
/// end before the call returns, and a failure is an error. Without `wait`,
/// the host starts the imports at its first frame.
pub fn apply_import(args: &Args, state: &mut AppState, wait: bool) -> anyhow::Result<()> {
    apply_heightmap(args, state, wait)?;
    apply_ramp(args, state, wait)?;
    if args.stops_page
        && let Some(id) = state.selected_raster()
    {
        state.pad.layer = Some(crate::ui::LayerRef::Raster(id));
        state.pad.page = Some(crate::ui::pad::Page::Stops);
    }
    if args.hillshade_page
        && let Some(id) = state.selected_raster()
    {
        state.set_raster_hillshade(id, true);
        state.pad.layer = Some(crate::ui::LayerRef::Raster(id));
        state.pad.page = Some(crate::ui::pad::Page::Hillshade);
    }
    Ok(())
}

fn apply_ramp(args: &Args, state: &mut AppState, wait: bool) -> anyhow::Result<()> {
    let Some(path) = &args.import_ramp else {
        return Ok(());
    };
    let id = state
        .selected_raster()
        .ok_or_else(|| anyhow!("--import-ramp needs a raster layer"))?;
    state.run_action(Action::ImportRamp(id, path.clone()));
    // The panel of the layer shows the result.
    state.pad.layer = Some(crate::ui::LayerRef::Raster(id));
    match &state.ramp_note {
        Some(note) if note.warning => {
            if wait {
                bail!("{}", note.text);
            }
        }
        _ => {
            let last = state.ramps.list().len().saturating_sub(1);
            let colors = state.ramps.list().get(last).map(|r| r.colors.clone());
            let part = state
                .globe
                .bands_mut(id)
                .zip(colors)
                .map_or(vale_terrain::Part::All, |(b, c)| b.part(&c));
            if let Err(text) = state.apply_ramp(id, last, false, part) {
                bail!("{text}");
            }
        }
    }
    Ok(())
}

fn apply_heightmap(args: &Args, state: &mut AppState, wait: bool) -> anyhow::Result<()> {
    let Some(path) = &args.import_heightmap else {
        return Ok(());
    };
    if !wait {
        state.actions.push(Action::ImportHeightmap(path.clone()));
        return Ok(());
    }
    state.import_note = None;
    state.select_height();
    // The layer of the import has its whole heightmap.
    state.pump_layers(true);
    state.globe.start_import(path.clone());
    state.globe.wait_import();
    match state.globe.take_import_result() {
        Some(Err(text)) => bail!("{text}"),
        Some(result) => state.note_import(result),
        None => bail!("the import of {} did not end", path.display()),
    }
    Ok(())
}

/// Runs `--export-heightmap`. The call needs no window and no GPU. `None`:
/// the run has no such flag.
pub fn export_heightmap(args: &Args) -> anyhow::Result<Option<Exported>> {
    let (Some(out), Some(path)) = (&args.export_heightmap, &args.project) else {
        return Ok(None);
    };
    // `Session::open` makes a file that is not there.
    if !path.is_file() {
        bail!("the project file {} does not exist", path.display());
    }
    let mut doc = Document::empty();
    let session = Session::open(path, &mut doc).with_context(|| format!("{}", path.display()))?;
    let project = &doc.project;
    let raster = project.rasters().iter().find(|raster| is_height(raster));
    let raster = raster.ok_or_else(|| {
        let path = path.display();
        anyhow!("the world of {path} has no height layer: no raster layer has the unit m")
    })?;
    let map = session
        .load_map(raster)
        .with_context(|| format!("{}", path.display()))?;
    let done = write_heightmap(&map, &project.world, Some(raster), out, &|_| {});
    done.map(Some).map_err(|text| anyhow!("{text}"))
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

/// Applies `--look-at` and `--zoom` to the view of the globe.
pub fn apply_globe_view(args: &Args, view: &mut GlobeView) -> anyhow::Result<()> {
    if let Some(spec) = &args.look_at {
        let [lon, lat] = parse_pair(spec)?;
        *view = GlobeView::centered(lon, lat);
    }
    view.zoom = args.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    Ok(())
}
