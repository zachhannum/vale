//! The UI: state, actions, and the draw function.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use vale_labeler::Fonts;
use vale_sphere::LonLat;
use vale_store::{
    Feature, GeometryKind, Layer, LayerId, LayerKind, Project, Raster, RasterDisplay, RasterId,
    Rgb, StoreError, World,
};
use vale_terrain::{Bands, Heightmap, Mode};

use crate::document::Document;
use crate::files::{Availability, ExportEvent, Files, PlainFiles, SystemEvents};
use crate::globe::import::{ImportResult, file_name};
use crate::globe::line::{self, LineEdit};
use crate::globe::polygon;
use crate::globe::snap::{LayerTargets, Targets};
use crate::globe::{Globe, START_ELEVATION, StackItem, Tool, brush};
use crate::headless;
use crate::pipeline::{Composed, Pipeline, Selection, fonts};
use crate::projects::{self, Location, Locations};
use crate::raster_style;
use crate::resume::{self, Resume};
use crate::session::{self, Session};
use crate::workcopy;

pub mod atlas;
pub mod browser;
pub mod dock;
pub mod elevation;
pub mod globe;
pub mod gradient;
pub mod inspector;
pub mod pad;
pub mod panels;
pub mod point_form;
pub mod raster_form;
pub mod world_form;

/// A request that needs the host: a dialog or a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    OpenDialog,
    OpenFiles(Vec<PathBuf>),
    /// Asks for the file of an export of the open page.
    ExportDialog(ExportFormat),
    /// Exports the open page to this PNG or PDF file.
    Export(PathBuf),
    ImportHeightmapDialog,
    /// Imports this equirectangular image into a height layer. A world with
    /// no height layer gets one.
    ImportHeightmap(PathBuf),
    /// Asks for a legend image for a new color ramp. The panel of this
    /// raster layer shows the result.
    ImportRampDialog(RasterId),
    /// Adds the colors of the blocks of this legend image to the color
    /// ramps. The panel of this raster layer shows the result.
    ImportRamp(RasterId, PathBuf),
    /// Asks for the file of an export of the selected raster layer.
    ExportHeightmapDialog,
    /// Exports the selected raster layer to this GeoTIFF file.
    ExportHeightmap(PathBuf),
    /// Makes a project of this world and opens it. With a folder of
    /// projects, the project is a new file there.
    NewWorld(World),
    /// Opens the world of this project file.
    OpenWorld(PathBuf),
    /// Opens the world of a project file that the system gave. A file
    /// outside the folders of the projects gets a copy in the local folder,
    /// and the copy opens.
    OpenExternal(PathBuf),
    /// Asks for a project file and opens its world.
    OpenProjectDialog,
    /// Gives the world of this project file this name.
    RenameProject(PathBuf, String),
    CopyProject(PathBuf),
    /// Removes this project file. The UI asks before it sends this.
    DeleteProject(PathBuf),
    /// Closes the open project and shows the start screen.
    ShowProjects,
    /// Saves each change that waits.
    Save,
    /// Gets this project file from iCloud Drive, and then opens its world.
    Download(PathBuf),
    /// Keeps one version of a project file that two devices changed. The
    /// number is an index into the other versions. `None` keeps the version
    /// of this device.
    KeepVersion(PathBuf, Option<usize>),
}

/// The time between two reads of the projects folders while a download runs.
pub const DOWNLOAD_POLL: Duration = Duration::from_secs(1);

/// The state of the save of the open project.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SaveMark {
    /// A change waits for its save, or a save ended a moment ago.
    Saving,
    /// The file holds each change.
    Saved,
    /// The last save failed.
    Failed,
}

/// The time that the save mark turns after a save.
pub const SAVE_MARK_TIME: Duration = Duration::from_millis(900);
/// The shortest time between two writes of the state for the next start.
const RESUME_DELAY: Duration = Duration::from_secs(1);

/// What the window shows.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Screen {
    /// The workspaces, with the open world.
    #[default]
    Editor,
    /// The start screen with the list of the projects.
    Browser,
}

/// The text of the last import or export of a heightmap.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    /// The import or the export failed, or the import changed the shape of
    /// the image.
    pub warning: bool,
    /// The raster layer of the import or the export. Only the panel of this
    /// layer shows the note.
    pub layer: Option<RasterId>,
    /// The time and the pass of egui when a panel first showed the note,
    /// and the last pass that showed it.
    pub shown: Option<(f64, u64)>,
    pub last_pass: u64,
}

impl Note {
    pub fn new(text: String, warning: bool, layer: Option<RasterId>) -> Note {
        Note {
            text,
            warning,
            layer,
            shown: None,
            last_pass: 0,
        }
    }
}

/// True if the extension of the file is that of a PNG or TIFF image.
pub fn is_heightmap_path(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str());
    ext.is_some_and(|e| ["png", "tif", "tiff"].contains(&e.to_ascii_lowercase().as_str()))
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Pdf,
}

/// A full-window view with its own canvas and panels.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Workspace {
    /// The world on a globe.
    #[default]
    Globe,
    /// The pages of the atlas and their map frames.
    #[value(alias = "map")]
    Atlas,
}

/// The arrangement of the controls on the screen.
#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Layout {
    /// Docked panels, for a mouse.
    Desktop,
    /// A full-screen canvas with floating cards, for a finger and a pen.
    Pad,
}

impl Default for Layout {
    fn default() -> Layout {
        if cfg!(target_os = "ios") {
            Layout::Pad
        } else {
            Layout::Desktop
        }
    }
}

/// A layer of the project.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LayerRef {
    Raster(RasterId),
    Vector(LayerId),
}

impl LayerRef {
    /// The reference of a layer record.
    pub fn of(layer: &Layer) -> LayerRef {
        match layer.raster() {
            Some(id) => LayerRef::Raster(id),
            None => LayerRef::Vector(layer.id),
        }
    }
}

/// The name of the raster layer that the app makes for a height.
pub const HEIGHT_NAME: &str = "Height";
/// The unit of a height layer. A raster layer with this unit has a sea and
/// rivers.
pub const HEIGHT_UNIT: &str = "m";

/// The values of a new raster layer.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterSpec {
    pub name: String,
    /// The unit of the values, for example `m` or `mm`.
    pub unit: String,
    /// The value of the lowest level.
    pub min: f64,
    /// The value of the highest level.
    pub max: f64,
    /// Texels on one edge of a cube face.
    pub face_size: usize,
}

impl RasterSpec {
    /// A height layer in meters, with the range and the bands of the app.
    pub fn height(name: &str, face_size: usize) -> RasterSpec {
        let bands = Bands::default();
        RasterSpec {
            name: name.to_string(),
            unit: HEIGHT_UNIT.to_string(),
            min: bands.min(),
            max: bands.max(),
            face_size,
        }
    }

    /// The first bands of the layer. `None`: the range is empty or not
    /// finite.
    fn bands(&self) -> Option<Bands> {
        let height = Bands::default();
        if self.unit == HEIGHT_UNIT && (self.min, self.max) == (height.min(), height.max()) {
            return Some(height);
        }
        Bands::with_range(self.min, self.max, self.unit == HEIGHT_UNIT)
    }
}

/// True if the values of the raster are heights in meters.
pub fn is_height(raster: &Raster) -> bool {
    raster.unit == HEIGHT_UNIT
}

/// Saves each change that waits as one undo step, and then the picture of
/// the globe. The picture stays if the globe does not hold each raster
/// layer of the project.
fn save_session(
    session: &mut Session,
    project: &mut Project,
    globe: &mut Globe,
) -> Result<(), StoreError> {
    globe.sync_maps();
    sync_reduced(project, globe);
    session.flush(project, &mut globe.raster_maps())?;
    let held = |raster: &Raster| globe.holds(raster.id);
    if project.rasters().iter().all(held) {
        session.save_thumbnail(&globe.picture(), &globe.view)?;
    }
    Ok(())
}

/// Puts the bands of each raster layer that has only its reduced heightmap
/// into its record, for a save.
fn sync_reduced(project: &mut Project, globe: &Globe) {
    for (id, map) in globe.reduced_maps() {
        if let Some(raster) = project.raster_mut(id) {
            session::sync_record(raster, map);
        }
    }
}

/// Gives the globe a raster layer of the project with a heightmap from the
/// file: the whole heightmap, or else the reduced heightmap.
fn add_loaded(globe: &mut Globe, raster: &Raster, map: Heightmap) -> bool {
    let (id, face_size, height) = (raster.id, raster.face_size as usize, is_height(raster));
    if map.face_size() == face_size {
        globe.add_raster(id, map, height)
    } else {
        globe.add_reduced(id, map, face_size, height)
    }
}

/// The whole heightmap of a raster layer that comes from the project file
/// in parts, for the brush.
struct Loader {
    id: RasterId,
    map: Heightmap,
    /// The count of the tiles that the loader asked the file for.
    next: usize,
}

/// The time that the loader reads tiles in one frame.
const LOAD_TIME: Duration = Duration::from_millis(6);

pub struct AppState {
    pub screen: Screen,
    pub browser: browser::BrowserState,
    pub workspace: Workspace,
    pub layout: Layout,
    pub pad: pad::PadState,
    pub globe: Globe,
    pub doc: Document,
    pub pipeline: Pipeline,
    pub fonts: Fonts,
    /// The last compose of the selected frame, or of the bottom frame of
    /// the open page.
    pub composed: Option<Composed>,
    /// Size of the canvas of the atlas in points.
    pub canvas_size: (f64, f64),
    /// The selected feature of a map.
    pub selection: Option<Selection>,
    /// The Atlas workspace.
    pub atlas: atlas::AtlasState,
    /// The count of the changes of the contents of the maps.
    pub map_generation: u64,
    /// The layer that the tools work on. The brush paints a selected raster
    /// layer that is visible.
    pub selected_layer: Option<LayerRef>,
    pub cursor_lonlat: Option<LonLat>,
    /// The value of the raster layer on the globe under the cursor, in the
    /// unit of the layer.
    pub cursor_value: Option<f64>,
    /// Last message, for example an import error.
    pub status: String,
    /// The text of the last import of a heightmap.
    pub import_note: Option<Note>,
    /// The text of the last export of the heightmap.
    pub export_note: Option<Note>,
    /// The color ramps of the app.
    pub ramps: crate::ramps::Ramps,
    /// The text of the last import of a color ramp.
    pub ramp_note: Option<Note>,
    /// The raster layers of the last import and of the last export.
    pub import_layer: Option<RasterId>,
    pub export_layer: Option<RasterId>,
    /// True: a complete export goes to `share`.
    pub share_exports: bool,
    /// The file of a complete export that the host gives to the Files app.
    pub share: Option<PathBuf>,
    /// The open form of a new world or of the world of the project.
    pub world_form: Option<world_form::WorldForm>,
    /// The open form of a point that the point tool placed.
    pub point_form: Option<point_form::PointForm>,
    /// The open form of a new raster layer.
    pub raster_form: Option<raster_form::RasterForm>,
    /// The face size of `--face-size`. The form of a new raster layer
    /// offers it, and it is on at first. `None`: the preset of the form
    /// gives the face size.
    pub form_face_size: Option<usize>,
    /// Requests that need the host (dialogs, files).
    pub actions: Vec<Action>,
    /// True: no timers, and each compose is final.
    pub headless: bool,
    /// False: the tool bar has no buttons for GeoJSON files and for exports.
    pub file_buttons: bool,
    pub frames: u64,
    /// The open project file. `None`: the app saves nothing.
    pub session: Option<Session>,
    /// The folders of the project files. With no folder, a new world has no
    /// file, and the app has no start screen.
    pub projects: Locations,
    /// The file services of the system for those folders.
    pub files: Box<dyn Files>,
    pub system: SystemEvents,
    /// The whole heightmap that comes from the project file for the brush.
    loader: Option<Loader>,
    /// The edit count at the last save.
    saved_edits: u64,
    /// The number of edits of the line, polygon, and point tools.
    line_edits: u64,
    /// The count of the undo steps, the redo steps, and the worlds that open.
    history: u64,
    /// The key of the snap targets of the globe. The targets change when it
    /// changes.
    targets_key: Option<TargetsKey>,
    /// The snap targets of each visible vector layer, with the fingerprint
    /// of the layer.
    layer_targets: HashMap<LayerId, (u64, Arc<LayerTargets>)>,
    /// The time of the last save, for the save mark.
    saved_at: Option<Instant>,
    /// The last save failed.
    save_failed: bool,
    /// The state for the next start that the projects folder holds.
    resume: Option<Resume>,
    /// The time of the last write of the state for the next start.
    resume_at: Option<Instant>,
}

/// What the snap targets of the globe come from.
#[derive(Debug, PartialEq)]
struct TargetsKey {
    /// The visible vector layers, with the count of their features.
    layers: Vec<(LayerId, usize)>,
    edits: u64,
    history: u64,
}

impl AppState {
    pub fn new(doc: Document) -> anyhow::Result<Self> {
        let mut state = AppState {
            screen: Screen::default(),
            browser: browser::BrowserState::default(),
            workspace: Workspace::default(),
            layout: Layout::default(),
            pad: pad::PadState::default(),
            globe: Globe::default(),
            doc,
            pipeline: Pipeline::new(),
            fonts: fonts()?,
            composed: None,
            canvas_size: (0.0, 0.0),
            selection: None,
            atlas: atlas::AtlasState::default(),
            map_generation: 0,
            selected_layer: None,
            cursor_lonlat: None,
            cursor_value: None,
            status: String::new(),
            import_note: None,
            export_note: None,
            ramps: Default::default(),
            ramp_note: None,
            import_layer: None,
            export_layer: None,
            share_exports: false,
            share: None,
            world_form: None,
            point_form: None,
            raster_form: None,
            form_face_size: None,
            actions: Vec::new(),
            headless: false,
            file_buttons: true,
            frames: 0,
            session: None,
            projects: Locations::default(),
            files: Box::new(PlainFiles),
            system: SystemEvents::default(),
            loader: None,
            saved_edits: 0,
            line_edits: 0,
            history: 0,
            targets_key: None,
            layer_targets: HashMap::new(),
            saved_at: None,
            save_failed: false,
            resume: None,
            resume_at: None,
        };
        state.set_layout(Layout::default());
        state.sync_globe();
        Ok(state)
    }

    /// Sets the layout. In the iPad layout the pen paints, so the brush is
    /// the first tool.
    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
        if layout == Layout::Pad {
            self.globe.tool = Tool::Brush;
        }
    }

    /// True if the undo log of the project file has the edits. If not, each
    /// heightmap keeps its strokes in memory.
    fn file_log(&self) -> bool {
        self.session.is_some()
    }

    /// The selected vector layer.
    pub fn selected_vector(&self) -> Option<LayerId> {
        match self.selected_layer {
            Some(LayerRef::Vector(id)) => Some(id),
            _ => None,
        }
    }

    /// The selected raster layer.
    pub fn selected_raster(&self) -> Option<RasterId> {
        match self.selected_layer {
            Some(LayerRef::Raster(id)) => Some(id),
            _ => None,
        }
    }

    /// Selects a layer, or no layer.
    pub fn select(&mut self, layer: Option<LayerRef>) {
        self.selected_layer = layer;
        self.sync_globe();
    }

    /// Gives the globe the visible layers of the project in the draw order,
    /// with the opacity of each raster layer. The brush paints the selected
    /// raster layer while it is visible. A layer that is not in the project
    /// is not selected. Call this after a change of the selection or of the
    /// layers.
    pub fn sync_globe(&mut self) {
        let (project, globe) = (&self.doc.project, &mut self.globe);
        let gone = match self.selected_layer {
            Some(LayerRef::Raster(id)) => project.raster(id).is_none(),
            Some(LayerRef::Vector(id)) => project.layer(id).is_none(),
            None => false,
        };
        if gone {
            self.selected_layer = None;
        }
        let item = |layer: &Layer| match layer.raster() {
            Some(id) => {
                let raster = project.raster(id)?;
                let (opacity, display) = (raster.opacity, raster.display);
                let held = globe.holds(id);
                let mut hillshade = raster.hillshade;
                hillshade.on &= is_height(raster);
                held.then_some(StackItem::Raster {
                    id,
                    opacity,
                    display,
                    smoothing: raster.smoothing,
                    hillshade,
                    rivers: raster.rivers,
                    lines: raster.lines,
                })
            }
            None => Some(StackItem::Vector(layer.id)),
        };
        let visible = project.layers().iter().filter(|layer| layer.visible);
        let stack: Vec<StackItem> = visible.filter_map(item).collect();
        let selected = match self.selected_layer {
            Some(LayerRef::Raster(id)) => Some(id).filter(|id| globe.holds(*id)),
            _ => None,
        };
        let shows = |id: &RasterId| {
            let item =
                |item: &StackItem| matches!(item, StackItem::Raster { id: of, .. } if of == id);
            stack.iter().any(item)
        };
        // The selected raster layer keeps its whole heightmap for the brush
        // while it is hidden. With no selected raster layer, the last one
        // keeps it.
        let active = selected;
        let paintable = selected.is_some_and(|id| shows(&id));
        let height = match selected.and_then(|id| project.raster(id)) {
            Some(raster) => is_height(raster),
            None => globe.is_height(),
        };
        globe.set_stack(stack);
        if active.is_some() {
            globe.activate(active);
        }
        globe.set_paintable(paintable);
        let drawn = match self.selected_layer {
            Some(LayerRef::Vector(id)) => project
                .layer(id)
                .filter(|l| l.visible)
                .map(|l| (id, l.kind)),
            _ => None,
        };
        let kind = |k: GeometryKind| drawn.filter(|d| d.1 == LayerKind::Vector(k));
        let (line, polygon) = (kind(GeometryKind::Line), kind(GeometryKind::Polygon));
        let point = kind(GeometryKind::Point);
        globe.draw_layer = line.or(polygon).or(point).map(|d| d.0);
        globe.lineable = line.is_some();
        globe.fillable = polygon.is_some();
        globe.pointable = point.is_some();
        // A tool that edits a vector layer stays such a tool in a layer of
        // another kind.
        if matches!(globe.tool, Tool::Line | Tool::Polygon | Tool::Point) {
            if globe.lineable {
                globe.tool = Tool::Line;
            } else if globe.fillable {
                globe.tool = Tool::Polygon;
            } else if globe.pointable {
                globe.tool = Tool::Point;
            }
        }
        if !height && brush::needs_height(globe.brush.mode) {
            globe.brush.mode = Mode::Raise;
        }
        if self.headless {
            // A run with no window reads the heightmap of the brush at once.
            self.pump_layers(true);
        }
    }

    /// The names of the raster layers that this device cannot show.
    pub fn lost_rasters(&self) -> Vec<String> {
        let lost = |layer: &&Layer| {
            let raster = layer.raster();
            raster.is_some_and(|id| !self.globe.holds(id))
        };
        let layers = self.doc.project.layers().iter().filter(lost);
        layers.map(|layer| layer.name.clone()).collect()
    }

    /// The layer record of a layer.
    pub fn layer_of(&self, layer: LayerRef) -> Option<&Layer> {
        match layer {
            LayerRef::Raster(id) => self.doc.project.raster_layer(id),
            LayerRef::Vector(id) => self.doc.project.layer(id),
        }
    }

    /// Tells the user which raster layers this device cannot show.
    pub fn note_lost_rasters(&mut self) {
        let lost = self.lost_rasters();
        if !lost.is_empty() {
            self.status = format!(
                "Raster layer {} cannot be shown on this device. It stays in the project.",
                lost.join(", ")
            );
        }
    }

    /// Makes the globe that of the raster layers of the project, with these
    /// heightmaps. The globe holds no heightmap that the device cannot
    /// show.
    ///
    /// A heightmap is the whole heightmap of its layer, or the reduced
    /// heightmap of `Session::load_display_maps`.
    pub fn open_rasters(&mut self, maps: Vec<(RasterId, Heightmap)>) {
        self.globe.clear();
        self.loader = None;
        for (id, map) in maps {
            if let Some(raster) = self.doc.project.raster(id) {
                add_loaded(&mut self.globe, raster, map);
            }
        }
        self.sync_globe();
    }

    /// Call this in each frame. With a project file, the main memory holds
    /// the whole heightmap of the raster layer that the brush paints only.
    /// This takes the whole heightmap from each other layer after its
    /// save, and it reads the whole heightmap of the layer that waits for
    /// the brush from the file. Without `all`, one call reads tiles for a
    /// short time, so the UI does not stop.
    pub fn pump_layers(&mut self, all: bool) {
        if self.session.is_none() {
            return;
        }
        self.load_layer(all);
        if self.globe.has_spare() {
            self.save_edit();
            self.globe.release_saved();
        }
    }

    /// Reads tiles of the raster layer that waits for the brush from the
    /// project file.
    fn load_layer(&mut self, all: bool) {
        let Some((id, face_size)) = self.globe.load_request() else {
            self.loader = None;
            return;
        };
        let session = self.session.as_ref().expect("checked above");
        let project = &self.doc.project;
        let record = project
            .raster(id)
            .filter(|r| r.face_size as usize == face_size);
        let loader = self.loader.take().filter(|loader| loader.id == id);
        let loader = match (loader, record) {
            (Some(loader), _) => Ok(loader),
            (None, Some(record)) => Session::empty_map(record).map(|map| {
                let name = project
                    .raster_layer(id)
                    .map_or("", |layer| layer.name.as_str());
                self.status = format!("Loading {name}…");
                Loader { id, map, next: 0 }
            }),
            (None, None) => Err(StoreError::Corrupt("the raster has no record".to_string())),
        };
        let start = Instant::now();
        let per_side = face_size.div_ceil(vale_store::TILE_SIZE);
        let total = vale_terrain::FACES * per_side * per_side;
        let loaded = loader.and_then(|mut loader| {
            while loader.next < total && (all || start.elapsed() < LOAD_TIME) {
                let (face, at) = (
                    loader.next / (per_side * per_side),
                    loader.next % (per_side * per_side),
                );
                let (row, column) = (at / per_side, at % per_side);
                if let Some(pixels) = session.load_tile(id, face, column, row)? {
                    loader.map.load_tile(face, column, row, &pixels);
                }
                loader.next += 1;
            }
            Ok(loader)
        });
        match loaded {
            Ok(loader) if loader.next < total => self.loader = Some(loader),
            Ok(loader) => {
                self.globe.give_full(id, loader.map);
                self.status.clear();
            }
            Err(e) => {
                self.status = format!("Could not load layer: {e}");
                self.globe.cancel_activation();
            }
        }
    }

    /// True while the whole heightmap of a raster layer comes from the
    /// project file.
    pub fn loading_layer(&self) -> bool {
        self.loader.is_some()
    }

    /// A name for a new raster layer: `name`, or `name` with a number if
    /// a raster layer has that name.
    pub fn raster_name(&self, name: &str) -> String {
        let layers = self.doc.project.layers();
        let taken = |name: &str| {
            let mut rasters = layers.iter().filter(|layer| layer.raster().is_some());
            rasters.any(|layer| layer.name == name)
        };
        let numbered = (2..).map(|number| format!("{name} {number}"));
        let mut names = std::iter::once(name.to_string()).chain(numbered);
        names.find(|name| !taken(name)).expect("a free number")
    }

    /// Saves each change that waits, so that the next change is its own
    /// undo step. A stroke that still waits for its texels saves at its end.
    fn save_edit(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        if self.globe.busy() {
            return;
        }
        if let Err(e) = save_session(session, &mut self.doc.project, &mut self.globe) {
            self.status = format!("Could not save project: {e}");
        }
    }

    /// Adds a raster layer with no painted tile and selects it. The device
    /// sets the limit of the face size. Returns `None` if the range of the
    /// values is empty or not finite, or if the face size is zero.
    fn new_raster(&mut self, spec: RasterSpec) -> Option<RasterId> {
        let bands = spec.bands()?;
        let face_size = spec.face_size.min(self.globe.max_face_size());
        if face_size == 0 {
            return None;
        }
        let height = spec.unit == HEIGHT_UNIT;
        // A height starts as a sea. Each other layer starts at the minimum
        // of its range.
        let start = if height { START_ELEVATION } else { bands.min() };
        let mut map = Heightmap::new(face_size, bands.level(start));
        map.bands = bands;
        let project = &mut self.doc.project;
        let id = project.add_raster(spec.name, face_size as u32, spec.unit, spec.min, spec.max);
        let raster = project.raster_mut(id).expect("the raster of the ID");
        session::sync_record(raster, &map);
        raster_style::store_raster_styles(project);
        self.globe.add_raster(id, map, height);
        self.select(Some(LayerRef::Raster(id)));
        Some(id)
    }

    /// Adds a raster layer with no painted tile, as one undo step, and
    /// selects it. Returns `None` if the range of the values is empty or
    /// not finite, or if the face size is zero.
    pub fn add_raster(&mut self, spec: RasterSpec) -> Option<RasterId> {
        self.save_edit();
        let id = self.new_raster(spec)?;
        self.save_edit();
        Some(id)
    }

    /// Adds a height layer in meters with the face size of the globe, and
    /// selects it.
    pub fn add_height(&mut self) -> RasterId {
        let name = self.raster_name(HEIGHT_NAME);
        let spec = RasterSpec::height(&name, self.globe.new_face_size());
        self.add_raster(spec).expect("the range of a height")
    }

    /// Removes a raster layer and its heightmap, as one undo step. Returns
    /// `false` if the project has no such layer.
    pub fn remove_raster(&mut self, id: RasterId) -> bool {
        self.save_edit();
        if self.doc.project.remove_raster(id).is_none() {
            return false;
        }
        self.globe.remove_raster(id);
        if self.pad.layer == Some(LayerRef::Raster(id)) {
            self.pad.layer = None;
        }
        self.sync_globe();
        self.save_edit();
        true
    }

    /// Gives a raster layer a new name. A name with no letter changes
    /// nothing.
    pub fn rename_raster(&mut self, id: RasterId, name: &str) -> bool {
        let layer = self.doc.project.raster_layer(id).map(|layer| layer.id);
        layer.is_some_and(|layer| self.doc.rename_layer(layer, name))
    }

    /// Sets how much a raster layer hides the layers below it, from 0 to 1.
    pub fn set_raster_opacity(&mut self, id: RasterId, opacity: f32) -> bool {
        let set = self.doc.project.set_raster_opacity(id, opacity);
        raster_style::store_raster_styles(&mut self.doc.project);
        self.sync_globe();
        set
    }

    /// Sets how a raster layer colors its values on the globe.
    pub fn set_raster_display(&mut self, id: RasterId, display: RasterDisplay) -> bool {
        let set = self.doc.project.set_raster_display(id, display);
        raster_style::store_raster_styles(&mut self.doc.project);
        self.sync_globe();
        set
    }

    /// Turns the smoothing of a raster layer on or off.
    pub fn set_raster_smoothing(&mut self, id: RasterId, on: bool) -> bool {
        let set = self.doc.project.set_raster_smoothing(id, on);
        self.sync_globe();
        set
    }

    /// Turns the hillshade of a raster layer on or off.
    pub fn set_raster_hillshade(&mut self, id: RasterId, on: bool) -> bool {
        let set = self.doc.project.set_raster_hillshade(id, on);
        self.sync_globe();
        set
    }

    /// Sets how much the hillshade of a raster layer hides its colors, from
    /// 0 to 1.
    pub fn set_raster_hillshade_opacity(&mut self, id: RasterId, opacity: f32) -> bool {
        let set = self.doc.project.set_raster_hillshade_opacity(id, opacity);
        self.sync_globe();
        set
    }

    /// Turns the highlights of the hillshade of a raster layer on or off.
    pub fn set_raster_hillshade_highlights(&mut self, id: RasterId, on: bool) -> bool {
        let set = self.doc.project.set_raster_hillshade_highlights(id, on);
        self.sync_globe();
        set
    }

    /// Sets the direction of the light of the hillshade of a raster layer,
    /// in degrees clockwise from north.
    pub fn set_raster_hillshade_azimuth(&mut self, id: RasterId, azimuth: f32) -> bool {
        let set = self.doc.project.set_raster_hillshade_azimuth(id, azimuth);
        self.sync_globe();
        set
    }

    /// Sets the height of the light of the hillshade of a raster layer, in
    /// degrees above the ground.
    pub fn set_raster_hillshade_altitude(&mut self, id: RasterId, altitude: f32) -> bool {
        let set = self.doc.project.set_raster_hillshade_altitude(id, altitude);
        self.sync_globe();
        set
    }

    /// Sets how much the hillshade of a raster layer makes the slopes
    /// steeper.
    pub fn set_raster_hillshade_exaggeration(&mut self, id: RasterId, exaggeration: f32) -> bool {
        let set = self
            .doc
            .project
            .set_raster_hillshade_exaggeration(id, exaggeration);
        self.sync_globe();
        set
    }

    /// Sets the shade color of the hillshade of a raster layer.
    pub fn set_raster_hillshade_color(&mut self, id: RasterId, color: Rgb) -> bool {
        let set = self.doc.project.set_raster_hillshade_color(id, color);
        self.sync_globe();
        set
    }

    /// Turns the lines of the stops of a raster layer on or off.
    pub fn set_raster_lines(&mut self, id: RasterId, on: bool) -> bool {
        let set = self.doc.project.set_raster_lines(id, on);
        self.sync_globe();
        set
    }

    /// Turns the rivers of a raster layer on or off.
    pub fn set_raster_rivers(&mut self, id: RasterId, on: bool) -> bool {
        let set = self.doc.project.set_raster_rivers(id, on);
        self.sync_globe();
        set
    }

    /// Sets the minimum feature size of a raster layer, in points.
    pub fn set_raster_feature_size(&mut self, id: RasterId, points: f32) -> bool {
        let set = self.doc.project.set_raster_feature_size(id, points);
        self.sync_globe();
        set
    }

    /// Sets the unit of the values of a raster layer. The unit of a height
    /// layer stays, and no other layer gets that unit.
    pub fn set_raster_unit(&mut self, id: RasterId, unit: &str) -> bool {
        let unit = unit.trim();
        let project = &mut self.doc.project;
        let height = project.raster(id).is_none_or(is_height);
        if height || unit == HEIGHT_UNIT {
            return false;
        }
        project.set_raster_unit(id, unit)
    }

    /// Moves a layer to this index of the draw order. Index 0 is the bottom.
    pub fn move_layer(&mut self, id: LayerId, to: usize) -> bool {
        let moved = self.doc.move_layer(id, to);
        self.sync_globe();
        self.touch();
        moved
    }

    /// Moves a layer one place in the draw order. `up` moves toward the top.
    /// Returns `false` at the end of the order.
    pub fn move_layer_by_one(&mut self, id: LayerId, up: bool) -> bool {
        let moved = self.doc.move_by_one(id, up);
        self.sync_globe();
        self.touch();
        moved
    }

    /// Removes a layer of each kind. A raster layer takes its heightmap, as
    /// one undo step.
    pub fn remove_layer(&mut self, id: LayerId) -> bool {
        match self.doc.layer(id).map(Layer::raster) {
            Some(Some(raster)) => self.remove_raster(raster),
            Some(None) => {
                let removed = self.doc.remove_layer(id);
                if self.pad.layer == Some(LayerRef::Vector(id)) {
                    self.pad.layer = None;
                }
                self.sync_globe();
                self.touch();
                removed
            }
            None => false,
        }
    }

    /// The record of the raster layer of the readout and of the bands
    /// panel: the selected raster layer if it is visible, or else the top
    /// visible raster layer.
    pub fn shown_raster(&self) -> Option<&Raster> {
        let project = &self.doc.project;
        let shows = |id: &RasterId| {
            let visible = project.raster_layer(*id).is_some_and(|layer| layer.visible);
            visible && self.globe.holds(*id)
        };
        let selected = self.selected_raster().filter(shows);
        let layers = project.layers().iter().rev();
        let top = || layers.filter_map(Layer::raster).find(shows);
        project.raster(selected.or_else(top)?)
    }

    /// The raster layer of the desktop style panel: the selected
    /// raster layer, or else the raster layer of the readout.
    pub fn panel_raster(&self) -> Option<&Raster> {
        let selected = self.selected_raster().filter(|id| self.globe.holds(*id));
        match selected {
            Some(id) => self.doc.project.raster(id),
            None => self.shown_raster(),
        }
    }

    /// The unit of the values of the raster layer of the readout. With no
    /// such layer, it is the meter.
    pub fn shown_unit(&self) -> &str {
        self.shown_raster().map_or(HEIGHT_UNIT, |r| r.unit.as_str())
    }

    /// A value of the raster layer of the readout as a text with its unit.
    pub fn value_text(&self, value: f64) -> String {
        let shown = self.shown_raster().map(|raster| raster.id);
        let map = shown.and_then(|id| self.globe.display_map(id));
        let bands = &map.unwrap_or(&self.globe.map).bands;
        crate::world::value_text(value, bands.max() - bands.min(), self.shown_unit())
    }

    /// The value under the cursor for the readout, with its unit. The value
    /// is that of the selected raster layer if it is visible, or else that
    /// of the top visible raster layer, with the name of that layer.
    pub fn readout_value(&self) -> Option<String> {
        let value = self.value_text(self.cursor_value?);
        let shown = self.shown_raster()?.id;
        if self.selected_raster() == Some(shown) {
            return Some(value);
        }
        let layer = self.doc.project.raster_layer(shown)?;
        Some(format!("{} {value}", layer.name))
    }

    /// Shows or hides a layer.
    pub fn set_layer_visible(&mut self, id: LayerId, visible: bool) {
        self.doc.set_visible(id, visible);
        self.sync_globe();
        self.touch();
    }

    /// Shows or hides a raster layer.
    pub fn set_raster_visible(&mut self, id: RasterId, visible: bool) {
        if let Some(layer) = self.doc.project.raster_layer(id).map(|layer| layer.id) {
            self.set_layer_visible(layer, visible);
        }
    }

    /// Selects a height layer that the globe shows, for an import or for
    /// the stroke test. This is the selected raster layer if it is a visible
    /// height layer, or else the first height layer, which becomes visible.
    /// A world with no height layer gets one. The new layer and the next
    /// edit of it are one undo step.
    pub fn select_height(&mut self) -> RasterId {
        let project = &self.doc.project;
        let height = |raster: &&Raster| is_height(raster) && self.globe.holds(raster.id);
        let visible = |raster: &&Raster| {
            let layer = project.raster_layer(raster.id);
            layer.is_some_and(|layer| layer.visible)
        };
        let selected = self.selected_raster().and_then(|id| project.raster(id));
        let selected = selected.filter(height).filter(visible);
        let first = project.rasters().iter().find(height);
        match selected.or(first).map(|raster| raster.id) {
            Some(id) => {
                self.set_raster_visible(id, true);
                self.select(Some(LayerRef::Raster(id)));
                id
            }
            None => {
                self.save_edit();
                let name = self.raster_name(HEIGHT_NAME);
                let spec = RasterSpec::height(&name, self.globe.new_face_size());
                self.new_raster(spec).expect("the range of a height")
            }
        }
    }

    /// Gives the globe the heightmap of each raster layer that an undo or a
    /// redo put back, and takes away each heightmap with no raster layer.
    fn restore_rasters(&mut self) -> Result<(), StoreError> {
        let project = &self.doc.project;
        for id in self.globe.rasters() {
            if project.raster(id).is_none() {
                self.globe.remove_raster(id);
            }
        }
        let Some(session) = &self.session else {
            return Ok(());
        };
        let limit = self.globe.max_face_size();
        for raster in project.rasters() {
            let held = self.globe.holds(raster.id);
            if !held && raster.face_size as usize <= limit {
                let map = session.load_display_map(raster)?;
                add_loaded(&mut self.globe, raster, map);
            }
        }
        // A raster layer with only its reduced heightmap gets the record
        // and the tiles that the step put back in the file.
        let reduced: Vec<(RasterId, u16)> = self
            .globe
            .reduced_maps()
            .map(|(id, map)| (id, map.base()))
            .collect();
        for (id, base) in reduced {
            let Some(raster) = project.raster(id) else {
                continue;
            };
            if raster.base != base {
                self.globe
                    .set_reduced(id, session.load_display_map(raster)?);
            } else if let Some(bands) = self.globe.bands_mut(id) {
                let new = session::bands_of(raster);
                if *bands != new {
                    *bands = new;
                }
            }
        }
        for &(id, key) in &session.last_restored {
            let alone = self.globe.holds(id) && self.globe.raster_map(id).is_none();
            let Some(raster) = project.raster(id).filter(|_| alone) else {
                continue;
            };
            let (face, column, row) = (key.face as usize, key.column as usize, key.row as usize);
            let pixels = session.load_tile(raster.id, face, column, row)?;
            self.globe
                .restore_tile(id, face, column, row, pixels.as_deref());
        }
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        match &self.session {
            Some(session) if self.file_log() => !self.globe.busy() && session.can_undo(),
            _ => self.globe.can_undo(),
        }
    }

    pub fn can_redo(&self) -> bool {
        match &self.session {
            Some(session) if self.file_log() => !self.globe.busy() && session.can_redo(),
            _ => self.globe.can_redo(),
        }
    }

    /// Puts back the world from before the last edit.
    pub fn undo(&mut self) {
        self.step(false);
    }

    /// Puts back the last edit that `undo` took away.
    pub fn redo(&mut self) {
        self.step(true);
    }

    fn step(&mut self, redo: bool) {
        self.commit_point_form();
        let able = if redo {
            self.can_redo()
        } else {
            self.can_undo()
        };
        if !able {
            return;
        }
        let file_log = self.file_log();
        let Some(session) = self.session.as_mut().filter(|_| file_log) else {
            if redo {
                self.globe.redo();
            } else {
                self.globe.undo();
            }
            return;
        };
        sync_reduced(&mut self.doc.project, &self.globe);
        let maps = &mut self.globe.raster_maps();
        let result = if redo {
            session.redo(&mut self.doc, maps)
        } else {
            session.undo(&mut self.doc, maps)
        };
        match result.and_then(|_| self.restore_rasters()) {
            Ok(()) => {
                self.history += 1;
                let gone = |id| self.doc.layer(id).is_none();
                if self.selection.is_some_and(|s| gone(s.layer)) {
                    self.selection = None;
                }
                self.sync_globe();
                // A layer can come back with other features under its ID.
                self.pipeline = Pipeline::new();
                self.touch();
            }
            Err(e) => {
                let what = if redo { "Redo" } else { "Undo" };
                self.status = format!("{what} failed: {e}");
            }
        }
    }

    /// Call this in each frame, after the changes of the frame. It saves the
    /// project a short time after a change, and at once after a stroke or an
    /// import. With `hold`, a control is held, and its changes wait. It also
    /// writes the state for the next start. Returns the time to the next
    /// save.
    pub fn save_tick(&mut self, hold: bool, now: Instant) -> Option<Duration> {
        self.globe.set_memory_undo(!self.file_log());
        // The point of an open form waits for its values.
        let hold = hold || self.point_form.is_some();
        let resume_wait = self.resume_tick(hold, now);
        self.doc.store_styles();
        let edits = self.edits();
        let session = self.session.as_mut()?;
        let saves = session.saves();
        let globe = &mut self.globe;
        let project = &mut self.doc.project;
        let result = if edits != self.saved_edits {
            // The edit is one undo step, with no part of the next stroke.
            self.saved_edits = edits;
            save_session(session, project, globe).map(|()| None)
        } else {
            let hold = hold || globe.busy();
            sync_reduced(project, globe);
            session.tick(project, &mut globe.raster_maps(), hold, now)
        };
        if session.saves() != saves {
            self.saved_at = Some(now);
        }
        let due = !hold && session.write_back_due(now);
        let result = match result {
            // The project file gets a change that is some minutes old.
            Ok(wait) if due => self.write_back().map(|()| wait),
            result => result,
        };
        self.save_failed = result.is_err();
        let wait = match result {
            Ok(wait) => wait,
            Err(e) => {
                self.status = format!("Could not save project: {e}");
                None
            }
        };
        // The save mark turns for a short time after a save.
        let mark_wait = self
            .saved_at
            .and_then(|at| SAVE_MARK_TIME.checked_sub(now.saturating_duration_since(at)));
        [wait, resume_wait, mark_wait].into_iter().flatten().min()
    }

    /// The state of the save, for the save mark. `None`: no project file is
    /// open.
    pub fn save_mark(&self, now: Instant) -> Option<SaveMark> {
        let session = self.session.as_ref()?;
        let turns = |at: Instant| now.saturating_duration_since(at) < SAVE_MARK_TIME;
        Some(if self.save_failed {
            SaveMark::Failed
        } else if session.waits() || self.saved_at.is_some_and(turns) || self.loader.is_some() {
            SaveMark::Saving
        } else {
            SaveMark::Saved
        })
    }

    /// Saves each change that waits and the state for the next start. A
    /// stroke that still waits for its texels stays open, and it saves at
    /// its end. The app calls this when it goes to the background.
    pub fn save_waiting(&mut self) -> Result<(), StoreError> {
        self.save_resume();
        let edits = self.edits();
        let Some(session) = &mut self.session else {
            return Ok(());
        };
        let globe = &mut self.globe;
        if globe.busy() {
            return Ok(());
        }
        self.saved_edits = edits;
        let saved = save_session(session, &mut self.doc.project, globe);
        // The system can stop the app in the background, so the project
        // file gets the working copy now.
        let saved = saved.and_then(|()| self.write_back());
        self.save_failed = saved.is_err();
        saved
    }

    /// The save command. The app saves with no command, so the command
    /// saves only a change that still waits.
    pub fn save(&mut self, now: Instant) {
        if self.session.is_none() {
            return;
        }
        match self.save_waiting() {
            Ok(()) => {
                self.saved_at = Some(now);
                self.status = format!("Saved {}", self.doc.project.world.name);
            }
            Err(e) => self.status = format!("Could not save project: {e}"),
        }
    }

    /// The state that the next start puts back.
    fn resume_now(&self) -> Resume {
        let session = self
            .session
            .as_ref()
            .filter(|_| self.screen == Screen::Editor);
        let Some(session) = session else {
            return Resume::START_SCREEN;
        };
        let path = session.path();
        let globe = &self.globe;
        Resume {
            project: Some(std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())),
            view: Some(resume::View {
                workspace: self.workspace,
                world_view: globe.world_view,
                globe: globe.view,
                flat_spec: globe.flat.spec(),
                flat_center: globe.flat.center,
                flat_zoom: globe.flat.zoom,
                page: Some(self.atlas.page),
            }),
        }
    }

    /// Writes the state for the next start to the projects folder, if the
    /// folder does not hold it.
    pub fn save_resume(&mut self) {
        let Some(dir) = self.projects.state_dir() else {
            return;
        };
        let resume = self.resume_now();
        if self.resume.as_ref() != Some(&resume) {
            // The state is a convenience, so its error has no message.
            let _ = resume.write(dir);
            self.resume = Some(resume);
        }
    }

    /// Writes a change of the state for the next start, one time in
    /// `RESUME_DELAY` at most. With `hold`, a gesture moves the view, and the
    /// write waits. Returns the time to the write.
    fn resume_tick(&mut self, hold: bool, now: Instant) -> Option<Duration> {
        if hold
            || self.projects.state_dir().is_none()
            || self.resume.as_ref() == Some(&self.resume_now())
        {
            return None;
        }
        let age = self.resume_at.map(|at| now.saturating_duration_since(at));
        if let Some(wait) = age.and_then(|age| RESUME_DELAY.checked_sub(age))
            && !wait.is_zero()
        {
            return Some(wait);
        }
        self.resume_at = Some(now);
        self.save_resume();
        None
    }

    /// Opens the project of the last run in the projects folder, at its last
    /// view. Call this one time, at the start.
    pub fn restore(&mut self) {
        let Some(resume) = self.projects.state_dir().and_then(Resume::read) else {
            return;
        };
        self.resume = Some(resume.clone());
        let Some(path) = resume.project.filter(|path| path.is_file()) else {
            return;
        };
        self.open_world(&path);
        let open = self.session.as_ref().is_some_and(|s| s.path() == path);
        if let (true, Some(view)) = (open, resume.view) {
            self.workspace = view.workspace;
            let globe = &mut self.globe;
            globe.world_view = view.world_view;
            globe.view = view.globe;
            globe.flat.set_spec(view.flat_spec);
            globe.flat.center = view.flat_center;
            globe.flat.zoom = view.flat_zoom;
            if let Some(page) = view
                .page
                .filter(|&p| self.doc.project.atlas.page(p).is_some())
            {
                self.atlas.page = page;
            }
        }
    }

    /// True if the world has a raster layer.
    pub fn has_raster(&self) -> bool {
        !self.doc.project.rasters().is_empty()
    }

    /// Saves each change that waits and the picture of the globe to the
    /// project file. Returns `false` if the save failed. The world then
    /// stays open.
    pub fn save_now(&mut self) -> bool {
        self.commit_point_form();
        let Some(session) = &mut self.session else {
            return true;
        };
        let globe = &mut self.globe;
        // The file gets each stroke that ended, and no part of a stroke that
        // still waits for its texels.
        if globe.busy() {
            globe.map.cancel_stroke();
        }
        let saved = save_session(session, &mut self.doc.project, globe);
        match saved.and_then(|()| self.write_back()) {
            Ok(()) => true,
            Err(e) => {
                self.status = format!("Could not save project: {e}. The world stays open.");
                false
            }
        }
    }

    /// Opens the session of a project file, with a working copy if the file
    /// is in a folder of iCloud Drive. `None`: the project goes into the
    /// document of the app.
    fn open_session(
        &mut self,
        path: &Path,
        doc: Option<&mut Document>,
    ) -> Result<Session, StoreError> {
        let doc = doc.unwrap_or(&mut self.doc);
        match self.projects.work_dir(path) {
            Some(dir) => Session::open_copy(path, &dir, self.files.as_ref(), doc),
            None => Session::open(path, doc),
        }
    }

    /// Gives each working copy of an earlier run to its project file. Call
    /// this one time, after the app has its folders.
    pub fn recover_projects(&mut self) {
        let Some(dir) = self.projects.all_work_dir() else {
            return;
        };
        let (saved, errors) = workcopy::recover(&dir, self.files.as_ref());
        let name = |path: &PathBuf| {
            let stem = path.file_stem().unwrap_or_default();
            stem.to_string_lossy().into_owned()
        };
        if let Some(error) = errors.first() {
            self.status =
                format!("Could not restore unsaved changes from the last session: {error}");
        } else if let Some(path) = saved.first() {
            self.status = format!("Saved changes from the last session to \"{}\"", name(path));
        }
    }

    /// Writes the working copy of the open project to its project file.
    fn write_back(&mut self) -> Result<(), StoreError> {
        let Some(session) = &mut self.session else {
            return Ok(());
        };
        match session.write_back(self.files.as_ref()) {
            Ok(false) => Ok(()),
            Ok(true) => {
                let stem = session.path().file_stem().unwrap_or_default();
                self.status = format!(
                    "This world was changed on another device. Your version is saved as \"{}\".",
                    stem.to_string_lossy()
                );
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Takes away the state of the UI that belongs to the last world.
    fn start_world(&mut self) {
        self.doc.frame.view = None;
        self.saved_edits = self.edits();
        self.history += 1;
        self.selection = None;
        self.select(None);
        self.raster_form = None;
        self.import_note = None;
        self.export_note = None;
        self.pad.layer = None;
        self.pad.add = false;
        self.pad.reorder = None;
        // A layer of the next world can have the ID of an old layer.
        self.pipeline = Pipeline::new();
        self.touch();
    }

    /// Opens a new project of this world. The file of the old world gets
    /// each change that waits, and then it stays as it is.
    fn new_world(&mut self, world: World) {
        if !self.save_now() {
            return;
        }
        self.session = None;
        self.status = format!("Created {}", world.name);
        let dir = self.projects.new_world_dir();
        let path = dir.map(|dir| session::new_project_path(dir, &world.name));
        self.doc.set_project(Project::new(world));
        self.globe.clear();
        self.start_world();
        if let Some(path) = path {
            match self.open_session(&path, None) {
                Ok(session) => self.session = Some(session),
                Err(e) => self.status = format!("Could not create file for new world: {e}"),
            }
        }
        self.show_world();
    }

    /// Shows the open world in the editor, and puts its file at the start of
    /// the recent list.
    fn show_world(&mut self) {
        self.screen = Screen::Editor;
        if let (Some(dir), Some(session)) = (self.projects.state_dir(), &self.session) {
            // The recent list is a convenience, so its error has no message.
            let _ = projects::push_recent(dir, session.path());
        }
    }

    /// Saves the open project, closes its file, and shows the start screen.
    /// If the save fails, the world stays open.
    pub fn show_projects(&mut self) {
        // The save can leave a message for the start screen.
        self.status.clear();
        if !self.save_now() {
            return;
        }
        self.session = None;
        self.world_form = None;
        self.raster_form = None;
        self.screen = Screen::Browser;
        self.reload_projects();
    }

    /// Reads the projects folders again while a download runs, one time
    /// each second. It opens the world that waits for its download. Returns
    /// `true` while a download runs.
    pub fn poll_downloads(&mut self, now: Instant) -> bool {
        let downloading = |entry: &projects::Entry| entry.availability == Availability::Downloading;
        if self.browser.pending.is_none() && !self.browser.entries.iter().any(downloading) {
            self.browser.polled = None;
            return false;
        }
        let polled = self.browser.polled;
        if polled.is_some_and(|at| now.duration_since(at) < DOWNLOAD_POLL) {
            return true;
        }
        self.browser.polled = Some(now);
        self.reload_projects();
        if let Some(path) = self.browser.pending.clone() {
            let entry = self.browser.entries.iter().find(|e| e.path == path);
            match entry.map(|entry| entry.availability) {
                Some(Availability::Here) => {
                    self.browser.pending = None;
                    self.actions.push(Action::OpenWorld(path));
                }
                Some(_) => {}
                None => self.browser.pending = None,
            }
        }
        true
    }

    /// Reads the projects folders again.
    pub fn reload_projects(&mut self) {
        self.browser.reload(&self.projects, self.files.as_ref());
    }

    /// The path and the name of the open project.
    pub fn open_project_entry(&self) -> Option<(PathBuf, String)> {
        let session = self.session.as_ref()?;
        let name = self.doc.project.world.name.clone();
        Some((session.path().to_path_buf(), name))
    }

    /// Runs the rename, the copy, or the delete of a project file. `Err`:
    /// the text for the user.
    fn project_action(&mut self, action: Action) -> Result<(), String> {
        let text = |e: StoreError| e.to_string();
        let open = self.session.as_ref().map(|s| s.path().to_path_buf());
        let is_open = |path: &Path| open.as_deref() == Some(path);
        match action {
            Action::RenameProject(path, name) => {
                let name = name.trim();
                if name.is_empty() {
                    return Err("Enter a name.".to_string());
                }
                if is_open(&path) {
                    // The next save writes the name, as one undo step.
                    self.doc.project.world.name = name.to_string();
                } else {
                    match self.projects.work_dir(&path) {
                        Some(dir) => {
                            projects::rename_synced(&path, name, &dir, self.files.as_ref())
                        }
                        None => projects::rename(&path, name),
                    }
                    .map_err(text)?;
                }
            }
            Action::CopyProject(path) => {
                if is_open(&path) && !self.save_now() {
                    return Err(self.status.clone());
                }
                let copy = match self.projects.work_dir(&path) {
                    Some(dir) => projects::copy_synced(&path, &dir, self.files.as_ref()),
                    None => projects::copy(&path),
                };
                let name = vale_store::summary(&copy.map_err(text)?);
                self.status = format!("Duplicated {}", name.map_err(text)?.name);
            }
            Action::DeleteProject(path) => {
                if is_open(&path) {
                    // The file goes away, so the last changes need no save.
                    if let Some(session) = self.session.take() {
                        session.discard();
                    }
                    self.screen = Screen::Browser;
                }
                projects::delete_synced(&path, self.files.as_ref()).map_err(text)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Acts on what the system told the app after the last frame. Call
    /// this in each frame.
    pub fn poll_system(&mut self) {
        // The system gives the folder each time the app comes to the front.
        // The sync service can have new files then.
        if let Some(dir) = self.system.take_cloud() {
            self.projects.set_cloud(dir);
            if self.screen == Screen::Browser {
                self.reload_projects();
            }
        }
        for path in self.system.take_opened() {
            self.open_external(&path);
        }
    }

    /// Opens the world of a project file that the system gave.
    fn open_external(&mut self, path: &Path) {
        let local = self.projects.dir(Location::Local).map(Path::to_path_buf);
        let (Some(local), None) = (local, self.projects.location_of(path)) else {
            let files = self.files.as_ref();
            if files.availability(path) == Availability::Here {
                self.open_world(path);
            } else {
                self.show_projects();
                self.run_action(Action::Download(path.to_path_buf()));
            }
            return;
        };
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let to = session::new_project_path(&local, &stem);
        let files = self.files.as_ref();
        let copied = std::fs::create_dir_all(&local).and_then(|()| {
            files.with_access(path, &mut || {
                files.read(path, &mut |from| std::fs::copy(from, &to).map(drop))
            })
        });
        match copied {
            Ok(()) => self.open_world(&to),
            Err(e) => self.status = format!("Could not copy file: {e}"),
        }
    }

    /// Opens the world of a project file. The file of the old world gets
    /// each change that waits. If the file does not open, the old world
    /// stays open.
    fn open_world(&mut self, path: &Path) {
        let open = self.session.as_ref().is_some_and(|s| s.path() == path);
        if open {
            self.screen = Screen::Editor;
        }
        if open || !self.save_now() {
            return;
        }
        let mut doc = self.doc.clone();
        let opened = self
            .open_session(path, Some(&mut doc))
            .and_then(|session| session.load_display_maps().map(|maps| (session, maps)));
        let (session, maps) = match opened {
            Ok(opened) => opened,
            Err(e) => {
                self.status = format!("Could not open world: {e}");
                return;
            }
        };
        self.doc = doc;
        self.status = format!("Opened {}", self.doc.project.world.name);
        self.open_rasters(maps);
        self.note_lost_rasters();
        self.session = Some(session);
        self.start_world();
        self.show_world();
    }

    /// The number of strokes of the brush and of edits of the vector tools.
    fn edits(&self) -> u64 {
        self.globe.edits() + self.line_edits
    }

    /// Builds the snap targets of the visible vector layers and of the color
    /// stops again if a layer changed. Only the layers that changed build
    /// again.
    pub fn refresh_targets(&mut self) {
        self.globe.refresh_stops();
        let layers = self.doc.project.layers().iter();
        let vectors: Vec<&Layer> = layers
            .filter(|l| l.visible && l.raster().is_none())
            .collect();
        let key = TargetsKey {
            layers: vectors.iter().map(|l| (l.id, l.features.len())).collect(),
            edits: self.edits(),
            history: self.history,
        };
        if self.targets_key.as_ref() == Some(&key) {
            return;
        }
        let mut cache = std::mem::take(&mut self.layer_targets);
        let mut targets = Vec::new();
        for layer in vectors {
            let print = LayerTargets::fingerprint(layer);
            let built = match cache.remove(&layer.id) {
                Some((old, built)) if old == print => built,
                _ => Arc::new(LayerTargets::build(layer.id, layer)),
            };
            targets.push(built.clone());
            self.layer_targets.insert(layer.id, (print, built));
        }
        let targets = (!targets.is_empty()).then(|| Arc::new(Targets::new(targets, Vec::new())));
        self.globe.set_targets(targets);
        self.targets_key = Some(key);
    }

    /// Puts the last stroke of the line tool or the polygon tool in the
    /// selected layer. The next save makes it one undo step.
    pub fn poll_line(&mut self) {
        let Some(stroke) = self.globe.take_line() else {
            return;
        };
        let Some(id) = self.selected_vector() else {
            return;
        };
        let Some(layer) = self.doc.project.layer(id) else {
            return;
        };
        let smoothing = self.globe.line.smoothing;
        let edit = match layer.geometry() {
            Some(GeometryKind::Polygon) => {
                polygon::commit(&stroke.runs, stroke.scale, smoothing).map(LineEdit::New)
            }
            _ => line::commit(&layer.features, &stroke.runs, stroke.scale, smoothing),
        };
        let project = &mut self.doc.project;
        let done = match edit {
            Some(LineEdit::New(geometry)) => {
                project.add_feature(id, Feature::new(geometry, Default::default()))
            }
            Some(LineEdit::Extend { uuid, geometry }) => project.set_geometry(id, uuid, geometry),
            None => false,
        };
        if done {
            self.line_edits += 1;
            self.pipeline.forget(id);
            self.touch();
        }
    }

    /// Changes the attribute schema of a vector layer. The open point form
    /// closes first, and each change that waits saves first. A change that
    /// `change` makes is one undo step, and the project saves at once.
    /// Returns the result of `change`.
    pub fn edit_schema(&mut self, id: LayerId, change: impl FnOnce(&mut Document) -> bool) -> bool {
        self.commit_point_form();
        self.save_edit();
        let changed = change(&mut self.doc);
        if changed {
            self.end_vector_edit(id);
        }
        changed
    }

    /// Ends the undo step of an edit of a vector layer and saves the
    /// project.
    fn end_vector_edit(&mut self, layer: LayerId) {
        self.pipeline.forget(layer);
        self.touch();
        self.line_edits += 1;
        // The next edit must not join this undo step.
        let edits = self.edits();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        self.saved_edits = edits;
        match save_session(session, &mut self.doc.project, &mut self.globe) {
            Ok(()) => self.saved_at = Some(std::time::Instant::now()),
            Err(e) => {
                self.save_failed = true;
                self.status = format!("Could not save project: {e}");
            }
        }
    }

    /// Marks the maps as needing a new compose.
    pub fn touch(&mut self) {
        self.map_generation += 1;
    }

    /// Moves the result of an import of a heightmap to the status text.
    pub fn poll_import(&mut self) {
        if let Some(result) = self.globe.take_import_result() {
            self.note_import(result);
        }
    }

    /// Reads the ramps of the folder of the state, if the folder changed.
    pub fn open_ramps(&mut self) {
        self.ramps.open(self.projects.state_dir());
    }

    /// Adds the colors of the blocks of a legend image to the ramps, and
    /// returns the name of the ramp and the number of colors.
    fn import_ramp(&mut self, path: &Path) -> Result<(String, usize), String> {
        let name = file_name(path);
        let image = image::open(path).map_err(|e| format!("{name}: {e}"))?;
        let image = image.to_rgb8();
        let (width, height) = (image.width() as usize, image.height() as usize);
        let pixels: Vec<[u8; 3]> = image.pixels().map(|p| p.0).collect();
        let colors = vale_terrain::swatches(width, height, &pixels);
        let max = vale_terrain::MAX_BANDS;
        match colors.len() {
            0 => return Err(format!("{name}: No color blocks found.")),
            n if n > max => {
                return Err(format!("{name}: {n} colors. The maximum is {max}."));
            }
            _ => {}
        }
        let count = colors.len();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        self.open_ramps();
        let ramp = self
            .ramps
            .add(stem.as_deref().unwrap_or("Ramp"), colors)
            .map_err(|e| format!("{name}: {e}"))?;
        Ok((ramp, count))
    }

    /// Gives the bands of a part of a raster layer the colors of a ramp.
    /// With `keep`, the layer keeps its stops. Otherwise, the part gets one
    /// band for each color.
    pub fn apply_ramp(
        &mut self,
        id: RasterId,
        index: usize,
        keep: bool,
        part: vale_terrain::Part,
    ) -> Result<(), String> {
        let Some(ramp) = self.ramps.list().get(index) else {
            return Err("Ramp not found.".to_owned());
        };
        let colors = ramp.colors.clone();
        let Some(bands) = self.globe.bands_mut(id) else {
            return Err("The layer has no color stops.".to_owned());
        };
        let applied = if keep {
            bands.stretched_part(&colors, part)
        } else {
            bands.painted_part(&colors, part)
        };
        let Some(applied) = applied else {
            let count = colors.len();
            return Err(format!("Range is too small for {count} bands."));
        };
        *bands = applied;
        self.globe.preview.selected = None;
        self.pad.style.forget_color();
        Ok(())
    }

    /// Shows the result of an import of a heightmap.
    pub fn note_import(&mut self, result: ImportResult) {
        let layer = self.import_layer;
        let note = match result {
            Ok(info) => Note::new(info.message(), !info.two_to_one, layer),
            Err(text) => Note::new(text, true, layer),
        };
        self.status = note.text.clone();
        self.import_note = Some(note);
    }

    /// Moves the result of an export of the heightmap to the status text.
    pub fn poll_export(&mut self) {
        let Some(result) = self.globe.take_export_result() else {
            return;
        };
        let note = match result {
            Ok(info) => {
                let text = info.message();
                if self.share_exports {
                    self.share = Some(info.path);
                }
                Note::new(text, false, self.export_layer)
            }
            Err(text) => Note::new(text, true, self.export_layer),
        };
        self.status = note.text.clone();
        self.export_note = Some(note);
    }

    /// Ends the hand-off of the export file `path` to the Files app. A
    /// cancelled hand-off removes the file and the text of the export.
    pub fn end_share(&mut self, event: ExportEvent, path: &Path) {
        match event {
            ExportEvent::Saved => {
                let text = format!("Saved {} to Files", file_name(path));
                self.status = text.clone();
                self.export_note = Some(Note::new(text, false, self.export_layer));
            }
            ExportEvent::Cancelled => {
                let _ = std::fs::remove_file(path);
                self.export_note = None;
                self.status.clear();
            }
        }
    }

    /// The file name of an export of a raster layer, from the name of the
    /// world.
    pub fn export_name(&self) -> String {
        let stem = session::file_stem(&self.doc.project.world.name);
        let stem = if stem.is_empty() { "raster" } else { &stem };
        format!("{stem}.tif")
    }

    /// Starts the export of a raster layer to a GeoTIFF file. A layer that
    /// has only its reduced heightmap in the main memory gets its whole
    /// heightmap from the project file, so the file has the full detail.
    pub fn export_raster(&mut self, id: RasterId, path: PathBuf) {
        self.export_layer = Some(id);
        let project = &self.doc.project;
        let Some(raster) = project.raster(id) else {
            return;
        };
        let loaded = match &self.session {
            Some(session) if self.globe.raster_map(id).is_none() => {
                session.load_map(raster).map(Some)
            }
            _ => Ok(None),
        };
        match loaded {
            Ok(loaded) => {
                let world = &project.world;
                self.globe
                    .start_export_of(path, world, Some(raster), loaded);
            }
            Err(e) => {
                let text = format!("Cannot export {}: {e}", file_name(&path));
                self.globe.fail_export(text);
            }
        }
        if self.headless {
            self.globe.wait_export();
        }
        self.poll_export();
    }

    /// True if the selected raster layer can go to a file now.
    pub fn can_export(&self) -> bool {
        let selected = self.selected_raster();
        let held = selected.is_some_and(|id| self.globe.holds(id));
        held && !self.globe.busy() && !self.globe.exporting()
    }

    /// Runs `OpenFiles`, `Export`, `ImportHeightmap`, `ExportHeightmap`, and
    /// `NewWorld`. The
    /// dialog actions belong to the host.
    pub fn run_action(&mut self, action: Action) {
        match action {
            Action::OpenFiles(paths) => {
                for path in paths {
                    let had_layers = !self.doc.frame.entries.is_empty();
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    match self.doc.open_geojson(&path) {
                        Ok(ids) => {
                            if !had_layers {
                                self.doc.frame.view = None;
                            }
                            self.status = format!("Opened {name}: {} layers", ids.len());
                        }
                        Err(e) => self.status = format!("{name}: {e}"),
                    }
                }
                self.touch();
            }
            Action::Export(path) => {
                let page = atlas::open_page(self).clone();
                let (pipeline, fonts) = (&mut self.pipeline, &mut self.fonts);
                self.status = match headless::export_page(pipeline, &self.doc, &page, fonts, &path)
                {
                    Ok(()) => format!("Exported {}", path.display()),
                    Err(e) => format!("{e:#}"),
                };
            }
            Action::ImportHeightmap(path) => {
                self.import_note = None;
                // A second import gets its refusal from the globe, and the
                // first import keeps its layer.
                if !self.globe.importing() {
                    let new = !self.doc.project.rasters().iter().any(is_height);
                    let id = self.select_height();
                    self.import_layer = Some(id);
                    if new {
                        // The panel of the new layer shows the result.
                        self.pad.layer = Some(LayerRef::Raster(id));
                    }
                }
                if self.headless {
                    // The layer of the import has its whole heightmap.
                    self.pump_layers(true);
                }
                self.globe.start_import(path);
                if self.headless {
                    self.globe.wait_import();
                }
                self.poll_import();
            }
            Action::ImportRamp(id, path) => {
                let note = match self.import_ramp(&path) {
                    Ok((name, count)) => {
                        let text = format!("Added {name} ({count} colors)");
                        Note::new(text, false, Some(id))
                    }
                    Err(text) => Note::new(text, true, Some(id)),
                };
                self.status = note.text.clone();
                self.ramp_note = Some(note);
            }
            Action::ExportHeightmap(path) => {
                self.export_note = None;
                let project = &self.doc.project;
                match self.selected_raster().and_then(|id| project.raster(id)) {
                    Some(raster) => {
                        let id = raster.id;
                        self.export_raster(id, path);
                    }
                    None => {
                        let name = file_name(&path);
                        let text = format!("Cannot export {name}: no raster layer selected");
                        self.status = text.clone();
                        self.export_note = Some(Note::new(text, true, None));
                    }
                }
            }
            Action::NewWorld(world) => self.new_world(world),
            Action::OpenWorld(path) => self.open_world(&path),
            Action::OpenExternal(path) => self.open_external(&path),
            Action::RenameProject(..) | Action::CopyProject(_) | Action::DeleteProject(_) => {
                if let Err(text) = self.project_action(action) {
                    self.status = text;
                }
                self.reload_projects();
            }
            Action::ShowProjects => self.show_projects(),
            Action::Save => self.save(Instant::now()),
            Action::Download(path) => {
                match self.files.start_download(&path) {
                    Ok(()) => self.browser.pending = Some(path),
                    Err(e) => self.status = format!("Could not start download: {e}"),
                }
                self.reload_projects();
            }
            Action::KeepVersion(path, keep) => {
                if let Err(e) = self.files.resolve(&path, keep) {
                    self.status = format!("Could not keep version: {e}");
                }
                self.reload_projects();
            }
            Action::OpenDialog
            | Action::ExportDialog(_)
            | Action::ImportHeightmapDialog
            | Action::ImportRampDialog(_)
            | Action::ExportHeightmapDialog
            | Action::OpenProjectDialog => {}
        }
    }
}

/// Draws the whole UI.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    state.globe.stats.tick(std::time::Instant::now());
    state.poll_import();
    state.poll_export();
    state.pump_layers(state.headless);
    if state.screen == Screen::Browser {
        if state.poll_downloads(Instant::now()) {
            ui.ctx().request_repaint_after(DOWNLOAD_POLL);
        }
        browser::draw(ui, state);
        state.frames += 1;
        return;
    }
    browser::dialogs(ui, state);
    state.sync_globe();
    state.check_point_form();
    if state.layout == Layout::Pad {
        match state.workspace {
            Workspace::Globe => pad::draw(ui, state),
            Workspace::Atlas => pad::atlas::draw(ui, state),
        }
        state.frames += 1;
        return;
    }
    // Fill the whole viewport. The test harness (egui_kittest) wraps the UI in an
    // 8 point outer margin, which would show as a dark border in screenshots. In the
    // real window this rectangle equals the rectangle of `ui`, so it changes nothing.
    let screen = ui.ctx().content_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(screen), |ui| {
        ui.set_clip_rect(screen);
        panels::toolbar(ui, state);
        match state.workspace {
            Workspace::Globe => {
                panels::status(ui, state);
                panels::brush_debug(ui, state);
                panels::brush(ui, state);
                elevation::draw(ui, state);
                globe::draw(ui, state);
            }
            Workspace::Atlas => {
                panels::left(ui, state);
                dock::draw(ui, state);
                panels::status(ui, state);
                atlas::desktop(ui, state);
            }
        }
    });
    world_form::desktop(ui.ctx(), state);
    raster_form::desktop(ui.ctx(), state);
    point_form::desktop(ui.ctx(), state);
    state.frames += 1;
}
