//! The screen of the app: the globe, the workspace switch, the panel of
//! each workspace, and the view card.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, pos2, vec2};
use isotherm_fields::{Means, Period};
use isotherm_import::vale_project::HeightLayer;
use isotherm_import::{Samples, Scale};
use isotherm_world::presets::OrbitPreset;
use isotherm_world::{Planet, Terrain};
use vale_shell::pad::icons::{self, Icon};
use vale_shell::pad::metrics::{PAD, TOUCH, WidthClass};
use vale_shell::pad::motion;
use vale_shell::pad::number::{slider_row, stepper_row};
use vale_shell::pad::theme;
use vale_shell::pad::widgets::{self, Controls, PANEL_ROW};
use vale_sphere::{ProjectionKind, ProjectionSpec};

mod ask;
mod planet;

pub use ask::{ASK_TEXT, ASK_TITLE, Ask, START_AGAIN};
pub use planet::*;

use crate::cli::Args;
use crate::document::{self, Change, Document, Read};
use crate::explore::{self, Explore, Layer};
use crate::globe::{Globe, Overlay, WorldView};
use crate::memory;
use crate::resume::View;
use crate::sim::{Run, Simulation, Speed};
use crate::thumbnail;
use crate::worlds::{self, Folders};
use isotherm_world::World;
use vale_globe::view::GlobeView;
use vale_shell::files::{Files, PlainFiles};
use vale_shell::workcopy::WorkCopy;

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Workspace {
    Worlds,
    Terrain,
    Planet,
    Simulate,
    Explore,
    Export,
}

impl Workspace {
    pub const ALL: [Workspace; 6] = [
        Workspace::Worlds,
        Workspace::Terrain,
        Workspace::Planet,
        Workspace::Simulate,
        Workspace::Explore,
        Workspace::Export,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Workspace::Worlds => "Worlds",
            Workspace::Terrain => "Terrain",
            Workspace::Planet => "Planet",
            Workspace::Simulate => "Simulate",
            Workspace::Explore => "Explore",
            Workspace::Export => "Export",
        }
    }

    /// The empty state of a workspace with no content yet.
    pub fn hint(self) -> Option<&'static str> {
        match self {
            Workspace::Worlds => Some("Saved worlds come in a later version."),
            Workspace::Export => Some("Export comes in a later version."),
            Workspace::Terrain | Workspace::Planet | Workspace::Simulate | Workspace::Explore => {
                None
            }
        }
    }

    /// The workspace has a panel of rows.
    pub fn has_panel(self) -> bool {
        matches!(
            self,
            Workspace::Terrain | Workspace::Planet | Workspace::Simulate | Workspace::Explore
        )
    }

    /// The workspace has the view card.
    pub fn has_view(self) -> bool {
        matches!(self, Workspace::Terrain | Workspace::Explore)
    }
}

/// The arrangement of the controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Layout {
    /// A top bar and a side panel.
    Desktop,
    /// The cards of the iPad and the iPhone.
    Pad,
}

impl Layout {
    /// The layout of this platform: pad on iOS, desktop elsewhere.
    pub fn of_platform() -> Layout {
        if cfg!(target_os = "ios") {
            Layout::Pad
        } else {
            Layout::Desktop
        }
    }
}

/// The file that the file picker asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A heightmap or a Vale project, for the terrain of the world.
    Import,
    /// A world file.
    Open,
    /// A Vale project, for a new world.
    ValeProject,
}

/// The step of an import that waits for the user.
pub enum Pending {
    /// An image with no units, and the heights of black and white.
    Scale {
        name: String,
        samples: Samples,
        black_m: f64,
        white_m: f64,
    },
    /// A Vale project with more than one height layer.
    Layers {
        path: PathBuf,
        name: String,
        layers: Vec<HeightLayer>,
    },
}

/// The name of the button of the workspace menu on a compact screen.
pub const WORKSPACE_MENU: &str = "Workspace";
pub const IMPORT: &str = "Import heightmap…";
pub const SEA_LEVEL: &str = "Sea level";
pub const BLACK: &str = "Black";
pub const WHITE: &str = "White";
pub const UNDO: &str = "Undo";
pub const REDO: &str = "Redo";
pub const PROJECTION: &str = "Projection";
pub const SCALE_HINT: &str = "Set the heights of black and white.";
pub const LAYERS_HINT: &str = "Choose a height layer.";
pub const TEMPERATURE: &str = "Temperature";
pub const ICE_COVER: &str = "Ice cover";
pub const MONTH: &str = "Month";
pub const CONTOURS: &str = "Contours";
pub const RUN_HINT: &str = "Run the model to see the results.";
pub const EMPTY_PERIOD: &str = "No values for this period yet.";
pub const RUN: &str = "Run";
pub const PAUSE: &str = "Pause";
pub const SPEED: &str = "Speed";
pub const QUALITY: &str = "Quality";
pub const COARSE_ONLY: &str = "This device has memory for the coarse grid only.";
/// The names of the face sizes of the quality setting.
pub const QUALITY_NAMES: [(usize, &str); 4] = [
    (32, "Coarse"),
    (64, "Medium"),
    (128, "Fine"),
    (256, "Finest"),
];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The file types of an import.
pub const IMPORT_TYPES: [&str; 4] = ["png", "tif", "tiff", "gpkg"];
/// The type of a GeoPackage file for the Files app.
#[cfg(target_os = "ios")]
const GEOPACKAGE: &str = "org.opengeospatial.geopackage";

/// The space between a card and the edge of the safe area.
const MARGIN: f32 = 16.0;
/// The space between the top of the safe area and the top row, and between
/// the top of a screen with no safe area and the top row.
const TOP_GAP: f32 = 4.0;
const TOP_MARGIN: f32 = 12.0;
/// The width of the workspace menu.
const MENU_WIDTH: f32 = 240.0;
/// The widths of the panel card and the view card.
const PANEL_WIDTH: f32 = 360.0;
const VIEW_WIDTH: f32 = 260.0;
/// The width of the list of the projections.
const LIST_WIDTH: f32 = 280.0;
/// The space above and below the rows of a card.
const CARD_PAD: f32 = 8.0;
/// The corners of the top of a bottom sheet.
const SHEET_RADIUS: u8 = 40;

/// The space of the grabber at the top of the bottom sheet.
const GRABBER: f32 = 16.0;

/// The header of the bottom sheet: the grabber and the title of the
/// workspace. A tap or a drag on it collapses or expands the sheet.
const SHEET_HEADER: f32 = GRABBER + TOUCH;

/// The drag in points that moves the sheet to the other detent.
const SHEET_SNAP: f32 = 40.0;
/// The height of the top bar and the width of the side panel of the
/// desktop layout.
const BAR: f32 = 56.0;
const SIDE: f32 = 320.0;
/// The range of the heights of black and white, in meters.
const SCALE_RANGE: f64 = 50_000.0;
const SCALE_STEP: f64 = 100.0;

pub struct IsothermApp {
    pub workspace: Workspace,
    pub layout: Layout,
    /// The workspace menu of a compact screen is open.
    pub menu: bool,
    /// The list of the projections is open.
    pub projections: bool,
    pub doc: Document,
    pub globe: Globe,
    /// The step of an import that waits for the user.
    pub pending: Option<Pending>,
    /// The last error, as one short sentence.
    pub status: Option<String>,
    /// The sea level at the start of a drag of the slider.
    sea_start: Option<f64>,
    /// The planet at the start of a drag of a slider of the Planet panel.
    planet_start: Option<Planet>,
    /// A change that waits for the user to delete the run.
    pub ask: Option<Ask>,
    /// The orbit preset of the last tap. Two orbit presets can give the
    /// same orbit, and the row then shows this one.
    orbit_pick: Option<OrbitPreset>,
    /// The folders of the world files. `None`: an import can only replace
    /// the terrain of an open world file.
    pub folders: Option<Folders>,
    /// The file services of the folders.
    pub files: Box<dyn Files>,
    /// The list of the worlds must be read again.
    pub worlds_stale: bool,
    /// The folder of the resume file. `None`: the next start does not
    /// open this world.
    pub state_dir: Option<PathBuf>,
    /// The file picker that the user asked for. The app shows it after the
    /// frame.
    pub pick: Option<Pick>,
    /// The file picker of iOS that waits for the user.
    pub picking: Option<Pick>,
    /// The iCloud folder that the system gave, for the next frame. The
    /// inner `None`: the device has no iCloud Drive.
    pub cloud_dir: Arc<Mutex<Option<Option<PathBuf>>>>,
    /// The open world file and its view, for the save of the iOS life
    /// cycle.
    pub resume: Rc<RefCell<Option<(PathBuf, View)>>>,
    /// The controls of the last frame.
    pub controls: Controls,
    /// The cards of the last frame.
    pub cards: Vec<(&'static str, Rect)>,
    build: String,
    /// The safe area of a screenshot. `None`: the safe area of the system.
    pub safe_area: Option<egui::SafeAreaInsets>,
    /// The monthly and yearly means of the last run.
    means: Option<Means>,
    /// The count of the calls of `set_means`.
    means_generation: u64,
    pub explore: Explore,
    /// The bottom sheet of a compact screen shows only its header.
    pub sheet_collapsed: bool,
    /// The distance of the drag of the sheet header, down from its start.
    sheet_drag: Option<f32>,
    /// The layer, the period, and the means generation of the overlay on
    /// the globe.
    overlay_shown: Option<(Layer, Period, u64)>,
    /// The run of the model. The save of the iOS life cycle shares it.
    pub run: Rc<RefCell<Run>>,
    /// The face sizes that the quality setting offers.
    pub qualities: Vec<usize>,
    /// The face size of the next run.
    pub quality: usize,
    /// A new run steps on a worker thread.
    pub threaded: bool,
    pub speed: Speed,
    /// The save of the run when the app leaves the foreground.
    #[cfg(target_os = "ios")]
    pub lifecycle: Option<vale_shell::lifecycle::Observer>,
    #[cfg(target_os = "ios")]
    pub picker: Option<vale_shell::picker::Picker>,
    #[cfg(target_os = "ios")]
    pub picked: vale_shell::picker::PickQueue,
}

/// Sets the style and the fonts of the app.
pub fn setup(ctx: &egui::Context) {
    theme::add_fonts(ctx);
    ctx.all_styles_mut(|style| style.visuals = egui::Visuals::dark());
}

/// A height as the app shows it, for example "−1,200 m".
pub fn meters_text(m: f64) -> String {
    format!("{} m", grouped(m))
}

/// A whole number with groups of three digits and a true minus sign, for
/// example "−1,200".
pub fn grouped(value: f64) -> String {
    let n = value.round() as i64;
    let digits = n.unsigned_abs().to_string();
    let mut text = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            text.push(',');
        }
        text.push(c);
    }
    let sign = if n < 0 { "−" } else { "" };
    format!("{sign}{text}")
}

impl IsothermApp {
    /// The app with `doc` open.
    pub fn new(doc: Document, layout: Layout) -> IsothermApp {
        let globe = Globe::new(&doc.world.terrain.heights);
        let qualities = memory::qualities(memory::available());
        let sim = restored_run(&doc, false);
        let quality = sim.as_ref().map_or_else(
            || memory::default_quality(&qualities),
            Simulation::face_size,
        );
        let run = Run {
            sim,
            path: doc.path.clone(),
            work: None,
        };
        let mut app = IsothermApp {
            workspace: Workspace::Explore,
            layout,
            menu: false,
            projections: false,
            doc,
            globe,
            pending: None,
            status: None,
            sea_start: None,
            planet_start: None,
            ask: None,
            orbit_pick: None,
            folders: None,
            files: Box::new(PlainFiles),
            worlds_stale: true,
            state_dir: None,
            pick: None,
            picking: None,
            cloud_dir: Arc::default(),
            resume: Rc::default(),
            controls: Controls::new(),
            cards: Vec::new(),
            build: crate::build::label(),
            safe_area: None,
            sheet_collapsed: false,
            sheet_drag: None,
            means: None,
            means_generation: 0,
            explore: Explore::default(),
            overlay_shown: None,
            run: Rc::new(RefCell::new(run)),
            qualities,
            quality,
            threaded: false,
            speed: Speed::default(),
            #[cfg(target_os = "ios")]
            lifecycle: None,
            #[cfg(target_os = "ios")]
            picker: None,
            #[cfg(target_os = "ios")]
            picked: Default::default(),
        };
        app.take_means();
        app
    }

    /// Moves the steps of the run to a worker thread, for the window.
    pub fn set_threaded(&mut self) {
        self.threaded = true;
        let mut run = self.run.borrow_mut();
        run.sim = run.sim.take().map(Simulation::threaded);
    }

    /// Shows the means of a month that ended in the run.
    fn take_means(&mut self) {
        let news = self
            .run
            .borrow_mut()
            .sim
            .as_mut()
            .and_then(Simulation::take_means);
        if let Some(means) = news {
            self.set_means(Some(means));
        }
    }

    /// Steps the run by one frame, and shows the months that ended.
    fn step_run(&mut self, ctx: &egui::Context) {
        {
            let mut run = self.run.borrow_mut();
            if let Some(sim) = run.sim.as_mut() {
                sim.speed = self.speed;
                sim.frame();
                if sim.running || sim.busy() {
                    ctx.request_repaint();
                }
            }
        }
        self.take_means();
    }

    /// Stops the run and deletes it, because the world changed.
    pub fn clear_run(&mut self) {
        self.run.borrow_mut().sim = None;
        self.set_means(None);
        if let Some(file) = &mut self.doc.file {
            let result = file
                .clear_run()
                .map_err(|e| format!("Could not delete the run: {e}."));
            self.saved(result);
        }
    }

    /// Notes a save to the world file, or shows its error.
    fn saved(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.run.borrow_mut().changed(),
            Err(e) => self.status = Some(e),
        }
    }

    /// Writes the run to the world file.
    pub fn save_run(&self) -> Result<(), String> {
        self.run.borrow_mut().save()
    }

    /// Starts or pauses the run. A pause saves the run.
    pub fn toggle_run(&mut self) {
        let result = {
            let mut run = self.run.borrow_mut();
            let sim = run.sim.get_or_insert_with(|| {
                Simulation::new(&self.doc.world, self.quality, self.threaded)
            });
            sim.running = !sim.running;
            if sim.running { Ok(()) } else { run.save() }
        };
        if let Err(e) = result {
            self.status = Some(e);
        }
    }

    /// A run exists, running or paused.
    pub fn has_run(&self) -> bool {
        self.run.borrow().sim.is_some()
    }

    /// Applies a change of the world. While a run exists, the change waits
    /// until the user agrees to delete the run.
    pub fn change(&mut self, ask: Ask) {
        if self.has_run() {
            self.ask = Some(ask);
        } else {
            self.apply_change(ask);
        }
    }

    /// Deletes the run and applies a change, as one undo step.
    fn apply_change(&mut self, ask: Ask) {
        match ask {
            Ask::Planet(planet) => {
                let old = std::mem::replace(&mut self.doc.world.planet, *planet);
                self.clear_run();
                let result = self.doc.save_planet(&old);
                self.saved(result);
            }
            Ask::SeaLevel(m) => {
                let terrain = &mut self.doc.world.terrain;
                let old = terrain.sea_level_m;
                terrain.sea_level_m = terrain.clamp_sea(m);
                self.clear_run();
                let result = self.doc.save_sea_level(old);
                self.saved(result);
                self.save_thumbnail();
            }
            Ask::Undo => self.undo(),
            Ask::Redo => self.redo(),
        }
    }

    /// Deletes the run and applies the change that waits.
    pub fn start_again(&mut self) {
        if let Some(ask) = self.ask.take() {
            self.clear_run();
            self.apply_change(ask);
        }
    }

    pub fn running(&self) -> bool {
        self.run
            .borrow()
            .sim
            .as_ref()
            .is_some_and(|sim| sim.running)
    }

    /// Runs a new run of `years` years at face size `n` and waits for it,
    /// saves it to the world file, and gives the yearly means as a line of
    /// text.
    pub fn run_years(&mut self, years: u64, face_size: usize) -> Result<String, String> {
        self.quality = face_size;
        let mut sim = Simulation::new(&self.doc.world, face_size, false);
        sim.run_steps(years * u64::from(isotherm_sim::STEPS_PER_YEAR));
        self.run.borrow_mut().sim = Some(sim);
        self.take_means();
        self.save_run()?;
        let means = self.means().ok_or("The run has no means.")?;
        let (mean, diff) = crate::sim::summary(means).ok_or("The run is shorter than one year.")?;
        Ok(format!(
            "year {years}: global mean {mean:.1} °C, equator minus pole {diff:.1} °C"
        ))
    }

    /// Sets the face size of the next run. A new size deletes the run.
    pub fn set_quality(&mut self, face_size: usize) {
        if face_size != self.quality {
            self.quality = face_size;
            self.clear_run();
        }
    }

    /// The app with the sample world.
    pub fn sample(layout: Layout) -> Result<IsothermApp, String> {
        Ok(IsothermApp::new(Document::sample()?, layout))
    }

    /// The app that the arguments ask for. With no `--project`, the app
    /// opens the last world in the state folder `state`, at its last view.
    /// With no last world, it opens the start screen over the sample world.
    pub fn from_args(args: &Args, state: Option<&Path>) -> Result<IsothermApp, String> {
        IsothermApp::from_args_in(args, state, None, Box::new(PlainFiles))
    }

    /// `from_args` with the folders of the worlds. The last world opens
    /// as a working copy if it is in iCloud Drive.
    pub fn from_args_in(
        args: &Args,
        state: Option<&Path>,
        folders: Option<Folders>,
        files: Box<dyn Files>,
    ) -> Result<IsothermApp, String> {
        let last = state.and_then(worlds::last_world);
        let ((doc, work), view) = match (&args.project, last) {
            (Some(path), _) => ((Document::open(path)?, None), None),
            (None, Some((path, view))) => match Document::open_in(&path, folders.as_ref(), &*files)
            {
                Ok(open) => (open, view),
                Err(_) => ((Document::sample()?, None), None),
            },
            (None, None) => ((Document::sample()?, None), None),
        };
        let start_screen = state.is_some() && args.project.is_none() && doc.file.is_none();
        let mut app = IsothermApp::new(doc, args.layout());
        app.set_work(work);
        app.folders = folders;
        app.files = files;
        app.state_dir = state.map(Path::to_path_buf);
        app.workspace = args.workspace;
        app.sheet_collapsed = args.sheet_detent == crate::cli::SheetDetent::Title;
        app.globe.world_view = args.view;
        app.globe.flat.set_spec(args.flat_spec());
        app.safe_area = args.safe_area;
        if let Some(view) = view {
            app.apply_view(&view);
        }
        if start_screen {
            app.workspace = Workspace::Worlds;
        }
        Ok(app)
    }

    /// The view of the open world.
    pub fn view(&self) -> View {
        View {
            workspace: self.workspace,
            world_view: self.globe.world_view,
            globe: self.globe.view,
            flat_spec: self.globe.flat.spec(),
            flat_center: self.globe.flat.center,
            flat_zoom: self.globe.flat.zoom,
        }
    }

    /// Puts back a view of the world.
    pub fn apply_view(&mut self, view: &View) {
        self.workspace = view.workspace;
        self.globe.world_view = view.world_view;
        self.globe.view = view.globe;
        self.globe.flat.set_spec(view.flat_spec);
        self.globe.flat.center = view.flat_center;
        self.globe.flat.zoom = view.flat_zoom;
    }

    /// Reads an import file. A heightmap in meters and a Vale project with
    /// one height layer go into the world at once. Else the panel asks
    /// for the rest.
    pub fn import_path(&mut self, path: &Path) {
        self.status = None;
        self.pending = None;
        match document::read_import(path) {
            Ok(Read::Terrain { name, terrain }) => self.finish_import(&name, *terrain),
            Ok(Read::Scale { name, samples }) => {
                let scale = Scale::default();
                self.pending = Some(Pending::Scale {
                    name,
                    samples,
                    black_m: scale.black_m,
                    white_m: scale.white_m,
                });
            }
            Ok(Read::Layers { path, name, layers }) => {
                self.pending = Some(Pending::Layers { path, name, layers });
            }
            Err(e) => self.status = Some(e),
        }
    }

    /// Puts an imported terrain in the world, and shows it. With no open
    /// world file, the import makes a new world file.
    fn finish_import(&mut self, name: &str, terrain: Terrain) {
        self.pending = None;
        if self.doc.file.is_none() {
            let world = World {
                name: name.to_owned(),
                planet: self.doc.world.planet.clone(),
                terrain,
            };
            self.create_world(world);
            self.save_thumbnail();
            return;
        }
        let result = self.doc.import(name, terrain, None);
        self.globe.set_terrain(&self.doc.world.terrain.heights);
        self.clear_run();
        self.saved(result);
        self.save_thumbnail();
    }

    /// Keeps the open world file as the world of the next start.
    pub fn remember(&self) {
        if let (Some(dir), Some(path)) = (&self.state_dir, &self.doc.path) {
            // The next start opens the start screen if this fails.
            let _ = worlds::remember(dir, path, self.view());
        }
    }

    /// Gives the run the working copy of the open world file.
    fn set_work(&mut self, work: Option<WorkCopy>) {
        let mut run = self.run.borrow_mut();
        run.path = match &work {
            Some(work) => Some(work.path().to_path_buf()),
            None => self.doc.path.clone(),
        };
        run.work = work;
    }

    /// Writes the picture of the open world to its file.
    pub fn save_thumbnail(&mut self) {
        let Some(file) = &mut self.doc.file else {
            return;
        };
        let picture = thumbnail::render(&self.doc.world.terrain, &self.globe.view, thumbnail::SIZE);
        let result = file
            .set_thumbnail(&picture)
            .map_err(|e| format!("Could not save the picture of the world: {e}."));
        self.saved(result);
    }

    /// Copies the changed working copy to iCloud Drive.
    fn write_back(&mut self) {
        if let Err(e) = self.run.borrow_mut().write_back(&*self.files) {
            self.status = Some(e);
        }
        self.follow_source();
    }

    /// Follows the world to a new file. A write back makes a new file if
    /// another device changed the file.
    fn follow_source(&mut self) {
        let moved = {
            let run = self.run.borrow();
            let source = run.work.as_ref().map(WorkCopy::source);
            source
                .filter(|source| Some(*source) != self.doc.path.as_deref())
                .map(Path::to_path_buf)
        };
        if let Some(source) = moved {
            self.doc.path = Some(source);
            self.worlds_stale = true;
            self.remember();
        }
    }

    /// Saves each change of the open world: its picture, its run, and its
    /// working copy.
    pub fn flush(&mut self) {
        if self.doc.file.is_none() {
            return;
        }
        self.save_thumbnail();
        if let Err(e) = self.save_run() {
            self.status = Some(e);
        }
        self.write_back();
        self.worlds_stale = true;
    }

    /// Opens a world file of the folders, at the start view in the Explore
    /// workspace.
    pub fn open_world(&mut self, path: &Path) {
        self.status = None;
        if self.doc.path.as_deref() != Some(path) {
            self.flush();
            match Document::open_in(path, self.folders.as_ref(), &*self.files) {
                Ok((doc, work)) => self.install(doc, work),
                Err(e) => {
                    self.status = Some(e);
                    return;
                }
            }
        }
        self.enter();
    }

    /// Makes a new world from the built-in Earth, and opens it.
    pub fn new_world(&mut self) {
        self.status = None;
        match Document::sample() {
            Ok(doc) => {
                self.create_world(doc.world);
                self.enter();
            }
            Err(e) => self.status = Some(e),
        }
    }

    /// Makes a new world from the first height layer of a Vale project,
    /// and opens it.
    pub fn new_from_vale(&mut self, path: &Path) {
        self.status = None;
        match document::vale_world(path) {
            Ok(world) => {
                self.create_world(world);
                self.enter();
            }
            Err(e) => self.status = Some(e),
        }
    }

    /// Opens a world file from outside the folders. A copy of the file
    /// goes to the folder of new worlds, and the copy opens.
    pub fn open_file(&mut self, path: &Path) {
        self.status = None;
        let file = document::file_name(path);
        match isotherm_store::summary(path) {
            Ok(_) => {}
            Err(isotherm_store::StoreError::NotAWorld)
                if isotherm_import::vale_project::world(path).is_ok() =>
            {
                self.status = Some(format!(
                    "{file} is a Vale project. Use New from Vale project…"
                ));
                return;
            }
            Err(e) => {
                self.status = Some(format!("Could not open {file}: {e}."));
                return;
            }
        }
        let Some(folders) = &self.folders else {
            self.open_world(path);
            return;
        };
        if folders.location_of(path).is_some() {
            self.open_world(path);
            return;
        }
        let dir = folders.new_world_dir();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let to = worlds::new_path(dir, &stem);
        let copy = std::fs::create_dir_all(dir).and_then(|()| {
            self.files
                .write(&to, &mut |to| std::fs::copy(path, to).map(drop))
        });
        match copy {
            Ok(()) => {
                self.worlds_stale = true;
                self.open_world(&to);
            }
            Err(e) => self.status = Some(format!("Could not copy {file}: {e}.")),
        }
    }

    /// Gives a world a new name. The name of the file stays.
    pub fn rename_world(&mut self, path: &Path, name: &str) {
        self.status = None;
        self.worlds_stale = true;
        if self.doc.path.as_deref() == Some(path) {
            if let Some(file) = &mut self.doc.file {
                let result = file
                    .set_name(name)
                    .map_err(|e| format!("Could not rename the world: {e}."));
                if result.is_ok() {
                    self.doc.world.name = name.to_owned();
                }
                self.saved(result);
            }
            return;
        }
        let Some(folders) = &self.folders else {
            return;
        };
        if let Err(e) = folders.rename(path, name, &*self.files) {
            self.status = Some(e);
        }
    }

    /// Makes a copy of a world, named with " copy". Returns its path.
    pub fn duplicate_world(&mut self, path: &Path) -> Option<PathBuf> {
        self.status = None;
        self.worlds_stale = true;
        if self.doc.path.as_deref() == Some(path) {
            self.flush();
        }
        let folders = self.folders.as_ref()?;
        match folders.duplicate(path, &*self.files) {
            Ok(to) => Some(to),
            Err(e) => {
                self.status = Some(e);
                None
            }
        }
    }

    /// Deletes a world file. If it is the open world, the sample world
    /// opens first, with no file.
    pub fn delete_world(&mut self, path: &Path) {
        self.status = None;
        self.worlds_stale = true;
        if self.doc.path.as_deref() == Some(path) {
            if let Some(work) = &mut self.run.borrow_mut().work {
                // The file goes, so the changes of the working copy go too.
                work.forget();
            }
            match Document::sample() {
                Ok(doc) => self.install(doc, None),
                Err(e) => {
                    self.status = Some(e);
                    return;
                }
            }
        }
        let Some(folders) = &self.folders else {
            return;
        };
        if let Err(e) = folders.delete(path, &*self.files) {
            self.status = Some(e);
        }
    }

    /// Makes a new world file in the folder of new worlds, and puts it in
    /// place of the open world.
    fn create_world(&mut self, world: World) {
        let Some(folders) = &self.folders else {
            self.status = Some("This device has no folder for worlds.".to_owned());
            return;
        };
        let dir = folders.new_world_dir().to_path_buf();
        let created = Document::create(world, &dir, Some(folders), &*self.files);
        match created {
            Ok((doc, work)) => {
                self.flush();
                self.install(doc, work);
                self.remember();
                self.worlds_stale = true;
            }
            Err(e) => self.status = Some(e),
        }
    }

    /// Puts `doc` in place of the open world, with its saved run.
    fn install(&mut self, doc: Document, work: Option<WorkCopy>) {
        self.globe.set_terrain(&doc.world.terrain.heights);
        let sim = restored_run(&doc, self.threaded);
        if let Some(sim) = &sim {
            self.quality = sim.face_size();
        }
        // The old database closes before its working copy goes.
        let old = std::mem::replace(&mut self.doc, doc);
        drop(old);
        self.run.borrow_mut().sim = sim;
        self.set_work(work);
        self.set_means(None);
        self.take_means();
        self.overlay_shown = None;
        self.pending = None;
        self.ask = None;
        self.explore.pin = None;
    }

    /// Shows the open world at the start view in the Explore workspace,
    /// and keeps it as the world of the next start.
    fn enter(&mut self) {
        self.globe.view = GlobeView::centered(crate::globe::START.0, crate::globe::START.1);
        self.workspace = Workspace::Explore;
        let has_picture = self.run.borrow().path.as_ref().is_some_and(|db| {
            isotherm_store::summary(db).is_ok_and(|summary| summary.thumbnail.is_some())
        });
        if self.doc.file.is_some() && !has_picture {
            self.save_thumbnail();
        }
        self.remember();
    }

    pub fn undo(&mut self) {
        let change = self.doc.undo();
        self.apply(change);
    }

    pub fn redo(&mut self) {
        let change = self.doc.redo();
        self.apply(change);
    }

    fn apply(&mut self, change: Result<Option<Change>, String>) {
        match change {
            Ok(Some(change)) => {
                if matches!(change, Change::Terrain) {
                    self.globe.set_terrain(&self.doc.world.terrain.heights);
                }
                self.clear_run();
                self.save_thumbnail();
            }
            Ok(None) => {}
            Err(e) => self.status = Some(e),
        }
    }

    /// Sets the means that the Explore workspace shows.
    pub fn set_means(&mut self, means: Option<Means>) {
        self.means = means;
        self.means_generation += 1;
    }

    pub fn means(&self) -> Option<&Means> {
        self.means.as_ref()
    }

    /// The field of the shown layer, if the run keeps it.
    fn field(&self) -> Option<&isotherm_fields::Field> {
        let means = self.means.as_ref()?;
        let output = self.explore.layer.output();
        means
            .outputs()
            .contains(&output)
            .then(|| means.field(output))
    }

    /// Puts the shown layer and period on the globe in the Explore
    /// workspace, and no overlay in the other workspaces.
    fn sync_overlay(&mut self) {
        let period = self.explore.period;
        let layer = self.explore.layer;
        let filled = self
            .field()
            .is_some_and(|field| field.get(period).is_some());
        let wanted = (self.workspace == Workspace::Explore && filled).then_some((
            layer,
            period,
            self.means_generation,
        ));
        if wanted == self.overlay_shown {
            return;
        }
        self.overlay_shown = wanted;
        let overlay = wanted.and_then(|_| {
            let field = self.field()?;
            let bands = layer.bands();
            Some(Overlay {
                face_size: field.face_size,
                levels: layer.levels(&bands, field.get(period)?),
                bands,
            })
        });
        self.globe.set_overlay(overlay);
    }

    /// The text of the readout, or `None` with no place or no value.
    pub fn readout(&self) -> Option<String> {
        if self.workspace != Workspace::Explore {
            return None;
        }
        let place = self.explore.place()?;
        let value = explore::value_at(self.field()?, self.explore.period, place)?;
        Some(format!(
            "{} · {}",
            self.explore.layer.value_text(value),
            explore::place_text(place)
        ))
    }

    /// Draws one frame.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let cloud = self.cloud_dir.lock().ok().and_then(|mut dir| dir.take());
        if let (Some(dir), Some(folders)) = (cloud, &mut self.folders) {
            folders.set_cloud(dir);
            self.worlds_stale = true;
        }
        if self.run.borrow().write_back_due() {
            self.write_back();
        } else {
            self.follow_source();
        }
        let now = self
            .doc
            .path
            .as_ref()
            .map(|path| (path.clone(), self.view()));
        if *self.resume.borrow() != now {
            *self.resume.borrow_mut() = now;
        }
        self.step_run(ui.ctx());
        self.sync_overlay();
        self.globe.contours = self.explore.contours;
        let ctx = ui.ctx().clone();
        let screen = ctx.viewport_rect();
        let safe = match self.safe_area {
            Some(insets) => screen - insets.0,
            None => ctx.content_rect(),
        };
        let mut controls = Controls::new();
        self.cards.clear();
        // The cards are glass when the app has a wgpu renderer.
        widgets::set_backdrop(&ctx, self.globe.backdrop());
        match self.layout {
            Layout::Desktop => self.draw_desktop(ui, screen, safe, &mut controls),
            Layout::Pad => self.draw_pad(ui, screen, safe, &mut controls),
        }
        if self.projections && self.globe.world_view == WorldView::Flat && self.workspace.has_view()
        {
            self.projection_list(&ctx, &mut controls, safe);
        } else {
            self.projections = false;
        }
        if self.ask.is_some() {
            self.ask_card(&ctx, &mut controls, screen, safe);
        }
        self.controls = controls;
    }

    /// The globe in `rect`, with the canvas below it.
    fn canvas(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let bands = self.doc.world.terrain.bands();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect);
            ui.painter().rect_filled(rect, 0.0, theme::CANVAS);
            let resp = self.globe.show(ui, rect, theme::CANVAS, &bands);
            let at = |pos: Option<egui::Pos2>| pos.and_then(|pos| self.globe.direction_at(pos));
            self.explore.hover = match resp.hovered() && !resp.dragged() {
                true => at(resp.hover_pos()),
                false => None,
            };
            if resp.clicked() {
                self.explore.pin = at(resp.interact_pointer_pos());
            }
        });
    }

    /// The desktop layout: a top bar with the workspace switch, a side panel
    /// at the left, and the globe in the rest of the window.
    fn draw_desktop(
        &mut self,
        ui: &mut egui::Ui,
        screen: Rect,
        safe: Rect,
        controls: &mut Controls,
    ) {
        let bar = Rect::from_min_max(screen.min, pos2(screen.right(), safe.top() + BAR));
        let side = Rect::from_min_max(
            pos2(screen.left(), bar.bottom()),
            pos2(safe.left() + SIDE, screen.bottom()),
        );
        let globe = Rect::from_min_max(pos2(side.right(), bar.bottom()), screen.max);
        self.canvas(ui, globe);
        let painter = ui.painter().clone();
        // The panels are opaque, with the color of a card.
        let [r, g, b, _] = theme::card().to_srgba_unmultiplied();
        let fill = egui::Color32::from_rgb(r, g, b);
        painter.rect_filled(bar, 0.0, fill);
        painter.rect_filled(side, 0.0, fill);
        let line = egui::Stroke::new(1.0, theme::divider());
        painter.hline(bar.x_range(), bar.bottom(), line);
        painter.vline(side.right(), side.y_range(), line);

        let names = Workspace::ALL.map(Workspace::name);
        let width = widgets::segments_width(ui, &names);
        let row = TOUCH + 2.0 * PAD;
        let switch = Rect::from_center_size(
            pos2(safe.center().x, safe.top() + BAR / 2.0),
            vec2(width, row),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(switch));
        self.switch_segments(&mut child, controls);
        self.cards.push(("workspace-switch", switch));

        let body = Rect::from_min_max(
            pos2(side.left() + 8.0, side.top() + CARD_PAD),
            pos2(side.right() - 8.0, safe.bottom() - CARD_PAD),
        );
        self.cards.push(("panel", body));
        let layout = egui::Layout::top_down(egui::Align::Min);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(layout));
        child.set_clip_rect(body);
        self.scroll_body(
            &mut child,
            "desktop-panel",
            controls,
            |app, ui, controls| {
                app.panel_rows(ui, controls, true);
            },
        );
        self.readout_card(ui.ctx(), globe.shrink(MARGIN));
    }

    /// The readout card at the bottom middle of `area`.
    fn readout_card(&mut self, ctx: &egui::Context, area: Rect) {
        if let Some(text) = self.readout() {
            let rect = explore::readout_card(ctx, area, &text);
            self.cards.push((explore::READOUT, rect));
        }
    }

    /// The pad layout: the globe fills the screen, and the cards float over
    /// it in the safe area.
    fn draw_pad(&mut self, ui: &mut egui::Ui, screen: Rect, safe: Rect, controls: &mut Controls) {
        let ctx = ui.ctx().clone();
        let class = WidthClass::of(screen.width());
        let top = safe.top()
            + if safe.top() > screen.top() {
                TOP_GAP
            } else {
                TOP_MARGIN
            };
        // The canvas and the globe fill the screen, also the bands outside
        // the safe area, and the glass of the cards shows them below the
        // cards. The cards stay in the safe area. A layer painter clips to
        // the safe area, so the canvas paints in this ui, which clips to
        // the screen.
        self.canvas(ui, screen);

        let switch = self.switch_rect(ui, safe, class, top);
        let backdrop = self.globe.backdrop();
        let current = self.workspace;
        self.cards.push(("workspace-switch", switch));
        widgets::card(
            &ctx,
            backdrop,
            "workspace-switch",
            switch,
            egui::Order::Middle,
            CornerRadius::same(255),
            |ui| {
                if class == WidthClass::Compact {
                    let open = self.menu;
                    let menu =
                        widgets::menu_button(ui, controls, WORKSPACE_MENU, current.name(), open);
                    if menu.clicked() {
                        self.menu = !open;
                    }
                    return;
                }
                self.switch_segments(ui, controls);
            },
        );
        let below = switch.bottom() + 12.0;
        let room = Rect::from_min_max(
            pos2(safe.left() + MARGIN, below),
            pos2(safe.right() - MARGIN, safe.bottom() - MARGIN),
        );
        if class == WidthClass::Compact {
            if self.workspace.has_panel() {
                self.sheet(&ctx, controls, safe);
            }
        } else {
            if self.workspace.has_view() {
                let height = self.measured(&ctx, "view-card", 2.0 * PANEL_ROW);
                let rect = Rect::from_min_size(
                    pos2(room.right() - VIEW_WIDTH, room.top()),
                    vec2(VIEW_WIDTH, height.min(room.height())),
                );
                self.floating_card(&ctx, controls, "view-card", rect, |app, ui, controls| {
                    app.view_rows(ui, controls);
                });
            }
            if matches!(self.workspace, Workspace::Simulate | Workspace::Planet) {
                let width = PANEL_WIDTH.min(room.width());
                let height = self.measured(&ctx, "panel", 4.0 * PANEL_ROW);
                let rect = Rect::from_min_size(room.min, vec2(width, height.min(room.height())));
                self.floating_card(&ctx, controls, "panel", rect, |app, ui, controls| match app
                    .workspace
                {
                    Workspace::Planet => app.planet_rows(ui, controls),
                    _ => app.simulate_rows(ui, controls),
                });
            }
            if matches!(self.workspace, Workspace::Terrain | Workspace::Explore) {
                let width = PANEL_WIDTH.min(room.width() - VIEW_WIDTH - MARGIN);
                let height = self.measured(&ctx, "panel", 3.0 * PANEL_ROW);
                let rect = Rect::from_min_size(room.min, vec2(width, height.min(room.height())));
                self.floating_card(&ctx, controls, "panel", rect, |app, ui, controls| match app
                    .workspace
                {
                    Workspace::Explore => app.explore_rows(ui, controls),
                    _ => app.terrain_rows(ui, controls),
                });
            }
        }
        // The readout stays above the sheet of a compact screen.
        let sheet = self.cards.iter().find(|(id, _)| *id == "sheet");
        let bottom = sheet.map_or(room.bottom(), |(_, rect)| rect.top() - 12.0);
        let area = Rect::from_min_max(room.min, pos2(room.right(), bottom));
        self.readout_card(&ctx, area);
        if class == WidthClass::Compact && self.menu {
            self.workspace_menu(&ctx, controls, switch);
        } else {
            self.menu = false;
        }
        self.hint_card(ui, safe);
    }

    /// The segmented list of the workspaces, in the max rect of `ui`.
    fn switch_segments(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let items: Vec<(&str, bool)> = Workspace::ALL.iter().map(|w| (w.name(), true)).collect();
        let active = Workspace::ALL.iter().position(|&w| w == self.workspace);
        if let Some(index) = widgets::segments(ui, controls, &items, active.unwrap_or(0)) {
            self.workspace = Workspace::ALL[index];
        }
    }

    /// The height of the rows of a card in the last frame, with the space
    /// above and below them.
    fn measured(&self, ctx: &egui::Context, id: &str, default: f32) -> f32 {
        let id = egui::Id::new(id).with("height");
        ctx.data(|d| d.get_temp::<f32>(id)).unwrap_or(default) + 2.0 * CARD_PAD
    }

    /// The rows of a card in a scroll area. Keeps their height for the
    /// next frame.
    fn scroll_body(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        controls: &mut Controls,
        rows: impl FnOnce(&mut IsothermApp, &mut egui::Ui, &mut Controls),
    ) {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let width = ui.available_width();
        let out = egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(8, 0))
                    .show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                        rows(self, ui, controls);
                    });
            });
        let height = out.content_size.y;
        let key = egui::Id::new(id).with("height");
        let ctx = ui.ctx();
        let old = ctx.data(|d| d.get_temp::<f32>(key));
        if old != Some(height) {
            ctx.data_mut(|d| d.insert_temp(key, height));
            ctx.request_repaint();
        }
    }

    /// A card with rows, in `rect`.
    fn floating_card(
        &mut self,
        ctx: &egui::Context,
        controls: &mut Controls,
        id: &'static str,
        rect: Rect,
        rows: impl FnOnce(&mut IsothermApp, &mut egui::Ui, &mut Controls),
    ) {
        self.cards.push((id, rect));
        let radius = CornerRadius::same(theme::CARD_RADIUS as u8);
        let backdrop = self.globe.backdrop();
        widgets::card(ctx, backdrop, id, rect, egui::Order::Middle, radius, |ui| {
            let body = rect.shrink2(vec2(8.0, CARD_PAD));
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(layout));
            self.scroll_body(&mut child, id, controls, rows);
        });
    }

    /// The bottom sheet of a compact screen, with the panel and the view
    /// rows below a header. It is at most half as high as the safe area,
    /// and only as high as its header when it is collapsed.
    fn sheet(&mut self, ctx: &egui::Context, controls: &mut Controls, safe: Rect) {
        let id = "sheet";
        let full = (self.measured(ctx, id, 4.0 * PANEL_ROW) + SHEET_HEADER)
            .min(safe.height() / 2.0)
            .max(SHEET_HEADER + CARD_PAD);
        let open = motion::value(
            ctx,
            egui::Id::new("sheet-open"),
            if self.sheet_collapsed { 0.0 } else { 1.0 },
            motion::SNAPPY,
        );
        let low = SHEET_HEADER + CARD_PAD;
        let height = (low + (full - low) * open - self.sheet_drag.unwrap_or(0.0)).clamp(low, full);
        let rect = Rect::from_min_max(
            pos2(safe.left() + 8.0, safe.bottom() - 8.0 - height),
            pos2(safe.right() - 8.0, safe.bottom() - 8.0),
        );
        self.cards.push((id, rect));
        let radius = CornerRadius {
            nw: SHEET_RADIUS,
            ne: SHEET_RADIUS,
            sw: theme::CARD_RADIUS as u8,
            se: theme::CARD_RADIUS as u8,
        };
        let backdrop = self.globe.backdrop();
        widgets::card(ctx, backdrop, id, rect, egui::Order::Middle, radius, |ui| {
            let header = Rect::from_min_size(rect.min, vec2(rect.width(), SHEET_HEADER));
            self.sheet_header(ui, controls, header, (height - low) / (full - low).max(1.0));
            let body = Rect::from_min_max(
                pos2(rect.left() + 8.0, header.bottom()),
                pos2(rect.right() - 8.0, rect.bottom() - CARD_PAD),
            );
            if body.height() < 1.0 {
                return;
            }
            let layout = egui::Layout::top_down(egui::Align::Min);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(body)
                    .layout(layout)
                    .id_salt("sheet-body"),
            );
            child.set_clip_rect(body);
            self.scroll_body(&mut child, id, controls, |app, ui, controls| {
                app.panel_rows(ui, controls, false);
            });
        });
    }

    /// The header of the bottom sheet in `rect`: the grabber and the title
    /// of the workspace. A tap collapses or expands the sheet, and a drag
    /// moves it with the finger. `open` is the part of the way from the
    /// collapsed sheet to the open sheet, from 0 to 1.
    fn sheet_header(&mut self, ui: &mut egui::Ui, controls: &mut Controls, rect: Rect, open: f32) {
        let handle = Rect::from_center_size(
            pos2(rect.center().x, rect.top() + GRABBER / 2.0 + 2.0),
            vec2(36.0, 5.0),
        );
        ui.painter().rect_filled(handle, 2.5, theme::divider());
        ui.painter().text(
            pos2(rect.left() + 20.0, rect.top() + GRABBER + TOUCH / 2.0),
            Align2::LEFT_CENTER,
            self.workspace.name(),
            theme::title(),
            theme::TEXT,
        );
        let label = if self.sheet_collapsed {
            "Expand sheet"
        } else {
            "Collapse sheet"
        };
        let resp = ui
            .interact(rect, ui.id().with("sheet-header"), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Grab);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        controls.push(widgets::Control {
            name: label.to_owned(),
            rect,
        });
        if resp.clicked() {
            self.sheet_collapsed = !self.sheet_collapsed;
        }
        if resp.dragged() {
            *self.sheet_drag.get_or_insert(0.0) += resp.drag_delta().y;
        }
        if resp.drag_stopped() {
            let drag = self.sheet_drag.take().unwrap_or(0.0);
            if drag > SHEET_SNAP {
                self.sheet_collapsed = true;
            } else if drag < -SHEET_SNAP {
                self.sheet_collapsed = false;
            }
            // The sheet moves on from where the finger left it.
            motion::set(ui.ctx(), egui::Id::new("sheet-open"), open);
        }
    }

    /// The rows of the panel of the workspace, with the view rows below
    /// them. `hint`: a workspace with no content shows its empty state.
    fn panel_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls, hint: bool) {
        if self.workspace == Workspace::Terrain {
            self.terrain_rows(ui, controls);
            ui.add_space(12.0);
        }
        if self.workspace == Workspace::Explore {
            self.explore_rows(ui, controls);
            ui.add_space(12.0);
        }
        if self.workspace == Workspace::Simulate {
            self.simulate_rows(ui, controls);
        }
        if self.workspace == Workspace::Planet {
            self.planet_rows(ui, controls);
        }
        if self.workspace.has_view() {
            self.view_rows(ui, controls);
        }
        if hint && let Some(text) = self.workspace.hint() {
            ui.add_space(8.0);
            wrapped(ui, text, theme::body(), theme::TEXT);
            if self.workspace == Workspace::Worlds {
                ui.add_space(4.0);
                wrapped(ui, &self.build, theme::hint(), theme::MUTE);
            }
        }
    }

    /// The rows of the Terrain panel: the world and its undo buttons, the
    /// import, and the sea level.
    fn terrain_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        self.title_row(ui, controls);
        match self.pending.take() {
            Some(Pending::Scale {
                name,
                samples,
                black_m,
                white_m,
            }) => self.scale_rows(ui, controls, name, samples, black_m, white_m),
            Some(Pending::Layers { path, name, layers }) => {
                self.layer_rows(ui, controls, path, name, layers)
            }
            None => {
                if widgets::button_row(ui, controls, IMPORT, true).clicked() {
                    self.pick = Some(Pick::Import);
                }
                self.sea_row(ui, controls);
            }
        }
        if let Some(status) = &self.status {
            ui.add_space(4.0);
            wrapped(ui, status, theme::hint(), theme::WARN);
            ui.add_space(4.0);
        }
    }

    /// The rows of the Explore panel: the layer, the month, the contours,
    /// and the legend.
    fn explore_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        if self.means.is_none() {
            ui.add_space(4.0);
            wrapped(ui, RUN_HINT, theme::body(), theme::TEXT);
            ui.add_space(4.0);
            return;
        }
        for layer in Layer::ALL {
            let on = layer == self.explore.layer;
            if widgets::text_row(ui, controls, layer.name(), 0.0, on, true).clicked() {
                self.explore.layer = layer;
            }
        }
        let mut value = explore::period_index(self.explore.period) as f64;
        let text = explore::period_name(self.explore.period);
        let resp = slider_row(ui, controls, MONTH, &mut value, 0.0..=12.0, text);
        if resp.changed {
            self.explore.period = explore::period_at(value.round().clamp(0.0, 12.0) as usize);
        }
        widgets::switch_row(ui, controls, CONTOURS, CONTOURS, &mut self.explore.contours);
        ui.add_space(4.0);
        let layer = self.explore.layer;
        explore::legend(ui, &layer.bands(), layer.unit());
        let filled = self
            .field()
            .is_some_and(|field| field.get(self.explore.period).is_some());
        if !filled {
            wrapped(ui, EMPTY_PERIOD, theme::hint(), theme::MUTE);
        }
    }

    /// The rows of the Simulate panel: Run or Pause, the year and the month,
    /// the speed, the quality, and the cost of the run.
    fn simulate_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let name = if self.running() { PAUSE } else { RUN };
        if widgets::button_row(ui, controls, name, true).clicked() {
            self.toggle_run();
        }
        let (step, ms, size) = match self.run.borrow().sim.as_ref() {
            Some(sim) => (sim.step_count(), sim.ms_per_step(), sim.face_size()),
            None => (0, 0.0, self.quality),
        };
        let label = |ui: &mut egui::Ui, text: &str| -> Rect {
            let (row, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
            let inner = Rect::from_min_max(pos2(row.left() + PAD, row.top()), row.max);
            let layout = egui::Layout::left_to_right(egui::Align::Center);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(layout));
            let text = egui::RichText::new(text)
                .font(theme::body())
                .color(theme::TEXT);
            child.add(egui::Label::new(text).selectable(false));
            row
        };
        label(ui, &year_text(step));

        let row = label(ui, SPEED);
        let names = Speed::ALL.map(Speed::name);
        let active = Speed::ALL.iter().position(|&s| s == self.speed);
        if let Some(i) = small_segments_in(ui, controls, row, &names, active.unwrap_or(0)) {
            self.speed = Speed::ALL[i];
        }

        let row = label(ui, QUALITY);
        let mut sizes = self.qualities.clone();
        if !sizes.contains(&size) {
            sizes.push(size);
        }
        let names: Vec<&str> = sizes.iter().map(|&n| quality_name(n)).collect();
        let active = sizes.iter().position(|&n| n == size).unwrap_or(0);
        if let Some(i) = small_segments_in(ui, controls, row, &names, active) {
            self.set_quality(sizes[i]);
        }
        ui.add_space(4.0);
        let mb = memory::run_bytes(size) as f64 / f64::from(1 << 20);
        wrapped(ui, &cost_text(ms, mb), theme::hint(), theme::MUTE);
        if self.qualities.len() == 1 {
            ui.add_space(4.0);
            wrapped(ui, COARSE_ONLY, theme::hint(), theme::MUTE);
        }
        if let Some(status) = &self.status {
            ui.add_space(4.0);
            wrapped(ui, status, theme::hint(), theme::WARN);
        }
        ui.add_space(4.0);
    }

    /// The name of the world, and the Undo and Redo buttons.
    fn title_row(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
        let redo = Rect::from_min_size(
            pos2(row.right() - TOUCH, row.center().y - TOUCH / 2.0),
            vec2(TOUCH, TOUCH),
        );
        let undo = redo.translate(vec2(-TOUCH - 4.0, 0.0));
        let name = Rect::from_min_max(row.min, pos2(undo.left() - 8.0, row.bottom()));
        let label = egui::RichText::new(&self.doc.world.name)
            .font(theme::title())
            .color(theme::TEXT);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name).layout(layout));
        child.add(egui::Label::new(label).truncate());
        let busy = self.pending.is_some();
        let can_undo = self.doc.can_undo() && !busy;
        let can_redo = self.doc.can_redo() && !busy;
        if widgets::icon_button_at(ui, controls, undo, UNDO, Icon::Undo, false, can_undo).clicked()
        {
            self.change(Ask::Undo);
        }
        if widgets::icon_button_at(ui, controls, redo, REDO, Icon::Redo, false, can_redo).clicked()
        {
            self.change(Ask::Redo);
        }
    }

    /// The sea level slider. A drag moves the coast on the globe, and the
    /// end of the drag is one undo step. While a run exists, a drag asks to
    /// delete the run first.
    fn sea_row(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let terrain = &self.doc.world.terrain;
        let before = terrain.sea_level_m;
        let range = terrain.low_m..=terrain.high_m;
        let mut value = before;
        let text = meters_text(before);
        let resp = slider_row(ui, controls, SEA_LEVEL, &mut value, range, &text);
        if resp.started {
            if self.has_run() {
                self.ask = Some(Ask::SeaLevel(value));
                return;
            }
            self.sea_start = Some(before);
            self.clear_run();
        }
        if resp.changed && self.sea_start.is_some() {
            let terrain = &mut self.doc.world.terrain;
            terrain.sea_level_m = terrain.clamp_sea(value);
        }
        if resp.released
            && let Some(old) = self.sea_start.take()
        {
            let result = self.doc.save_sea_level(old);
            self.saved(result);
            self.save_thumbnail();
        }
    }

    /// The heights of black and white of an image with no units.
    fn scale_rows(
        &mut self,
        ui: &mut egui::Ui,
        controls: &mut Controls,
        name: String,
        samples: Samples,
        mut black_m: f64,
        mut white_m: f64,
    ) {
        wrapped(ui, SCALE_HINT, theme::hint(), theme::MUTE);
        ui.add_space(4.0);
        let text = meters_text(black_m);
        let range = -SCALE_RANGE..=white_m - SCALE_STEP;
        stepper_row(ui, controls, BLACK, &mut black_m, SCALE_STEP, range, &text);
        let text = meters_text(white_m);
        let range = black_m + SCALE_STEP..=SCALE_RANGE;
        stepper_row(ui, controls, WHITE, &mut white_m, SCALE_STEP, range, &text);
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), PANEL_ROW), Sense::hover());
        let right = pos2(row.right(), row.center().y);
        let import = widgets::accent_button(ui, controls, right, "Import");
        let right = pos2(import.rect.left() - 8.0, row.center().y);
        if plain_button(ui, controls, right, "Cancel").clicked() {
            return;
        }
        if import.clicked() {
            let scale = Scale { black_m, white_m };
            match document::scale_terrain(&samples, scale) {
                Ok(terrain) => self.finish_import(&name, terrain),
                Err(e) => self.status = Some(e),
            }
            return;
        }
        self.pending = Some(Pending::Scale {
            name,
            samples,
            black_m,
            white_m,
        });
    }

    /// The height layers of a Vale project. A tap on a layer imports it.
    fn layer_rows(
        &mut self,
        ui: &mut egui::Ui,
        controls: &mut Controls,
        path: PathBuf,
        name: String,
        layers: Vec<HeightLayer>,
    ) {
        wrapped(ui, LAYERS_HINT, theme::hint(), theme::MUTE);
        ui.add_space(4.0);
        let mut chosen = None;
        for layer in &layers {
            let id = format!("Layer {}", layer.name);
            if widgets::text_row_as(ui, controls, &id, &layer.name, 0.0, false, true).clicked() {
                chosen = Some(layer.id);
            }
        }
        if widgets::text_row(ui, controls, "Cancel", 0.0, false, true).clicked() {
            return;
        }
        if let Some(id) = chosen {
            match document::layer_terrain(&path, id) {
                Ok(terrain) => self.finish_import(&name, terrain),
                Err(e) => self.status = Some(e),
            }
            return;
        }
        self.pending = Some(Pending::Layers { path, name, layers });
    }

    /// The Globe and Flat switch, and in the flat view, the projection.
    fn view_rows(&mut self, ui: &mut egui::Ui, controls: &mut Controls) {
        let views = [WorldView::Globe, WorldView::Flat];
        let items: Vec<(&str, bool)> = views.iter().map(|v| (v.name(), true)).collect();
        let (row, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), TOUCH + 2.0 * PAD),
            Sense::hover(),
        );
        let track = row;
        ui.painter()
            .rect_filled(track, theme::capsule(track), theme::track());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(track));
        let active = views.iter().position(|&v| v == self.globe.world_view);
        if let Some(i) = segments_fill(&mut child, controls, &items, active.unwrap_or(0)) {
            self.globe.world_view = views[i];
        }
        if self.globe.world_view != WorldView::Flat {
            return;
        }
        ui.add_space(4.0);
        let kind = self.globe.flat.spec().kind;
        let open = self.projections;
        let resp = widgets::text_row(ui, controls, PROJECTION, 0.0, open, true);
        let rect = resp.rect;
        let chevron =
            Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y), vec2(18.0, 18.0));
        icons::paint(ui.painter(), chevron, Icon::Down, 18.0, theme::MUTE);
        let room = (chevron.left()
            - 6.0
            - rect.left()
            - widgets::text_width(ui, PROJECTION, theme::body())
            - 12.0)
            .max(0.0);
        let galley = ui.painter().layout(
            kind.name().to_owned(),
            theme::body(),
            theme::MUTE,
            f32::INFINITY,
        );
        if galley.size().x <= room {
            let at = pos2(
                chevron.left() - 6.0 - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            ui.painter().galley(at, galley, theme::MUTE);
        }
        if resp.clicked() {
            self.projections = !open;
        }
        let key = egui::Id::new("projection-row");
        ui.data_mut(|d| d.insert_temp(key, rect));
    }

    /// The list of the projections, next to the Projection row. A tap on
    /// a row picks the projection and closes the list.
    fn projection_list(&mut self, ctx: &egui::Context, controls: &mut Controls, safe: Rect) {
        let Some(row) = ctx.data(|d| d.get_temp::<Rect>(egui::Id::new("projection-row"))) else {
            return;
        };
        let inset = widgets::menu_inset();
        let kinds = ProjectionKind::ALL;
        let full = 2.0 * inset.y + kinds.len() as f32 * PANEL_ROW;
        let bounds = safe.shrink(8.0);
        let width = LIST_WIDTH.min(bounds.width());
        // Below the row if it fits, else above it, else as high as the
        // screen allows.
        let below = bounds.bottom() - row.bottom() - 8.0;
        let above = row.top() - 8.0 - bounds.top();
        let (top, height) = if full <= below {
            (row.bottom() + 8.0, full)
        } else if full <= above {
            (row.top() - 8.0 - full, full)
        } else if below >= above {
            (row.bottom() + 8.0, below)
        } else {
            (bounds.top(), above)
        };
        let left = row.left().clamp(bounds.left(), bounds.right() - width);
        let rect = Rect::from_min_size(pos2(left, top), vec2(width, height));
        let pressed = ctx.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        if pressed.is_some_and(|p| !rect.contains(p) && !row.contains(p)) {
            self.projections = false;
            return;
        }
        self.cards.push(("projections", rect));
        let spec = self.globe.flat.spec();
        let mut chosen = None;
        let origin = row.center().clamp(rect.min, rect.max);
        let backdrop = widgets::backdrop(ctx);
        widgets::menu_card(ctx, backdrop, "projections", rect, origin, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("projections")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    for kind in kinds {
                        let on = kind == spec.kind;
                        if widgets::text_row(ui, controls, kind.name(), 0.0, on, true).clicked() {
                            chosen = Some(kind);
                        }
                    }
                });
        });
        if let Some(kind) = chosen {
            self.globe.flat.set_spec(ProjectionSpec { kind, ..spec });
            self.projections = false;
        }
    }

    /// The card of the workspace switch: a segmented list at the middle of
    /// the top row, or a menu button at its left end on a compact screen.
    fn switch_rect(&self, ui: &egui::Ui, safe: Rect, class: WidthClass, top: f32) -> Rect {
        let row = TOUCH + 2.0 * PAD;
        if class == WidthClass::Compact {
            let width = widgets::menu_button_width(ui, self.workspace.name());
            return Rect::from_min_size(pos2(safe.left() + MARGIN, top), vec2(width, row));
        }
        let names = Workspace::ALL.map(Workspace::name);
        let width = widgets::segments_width(ui, &names);
        Rect::from_min_size(pos2(safe.center().x - width / 2.0, top), vec2(width, row))
    }

    /// The menu of the workspaces below the menu button.
    fn workspace_menu(&mut self, ctx: &egui::Context, controls: &mut Controls, button: Rect) {
        let inset = widgets::menu_inset();
        let height = 2.0 * inset.y + Workspace::ALL.len() as f32 * PANEL_ROW;
        let at = pos2(button.left(), button.bottom() + 8.0);
        let rect = Rect::from_min_size(at, vec2(MENU_WIDTH, height));
        let pressed = ctx.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        if pressed.is_some_and(|p| !rect.contains(p) && !button.contains(p)) {
            self.menu = false;
            return;
        }
        self.cards.push(("workspace-menu", rect));
        let current = self.workspace;
        let mut chosen = None;
        let origin = button.center().clamp(rect.min, rect.max);
        let backdrop = widgets::backdrop(ctx);
        widgets::menu_card(ctx, backdrop, "workspace-menu", rect, origin, |ui| {
            for workspace in Workspace::ALL {
                let on = workspace == current;
                let name = workspace.name();
                if widgets::text_row(ui, controls, name, 0.0, on, true).clicked() {
                    chosen = Some(workspace);
                }
            }
        });
        if let Some(workspace) = chosen {
            self.workspace = workspace;
            self.menu = false;
        }
    }

    /// The card with the empty state of the workspace, at the bottom of the
    /// screen. The Worlds workspace also shows which build this is.
    fn hint_card(&mut self, ui: &egui::Ui, safe: Rect) {
        let Some(hint) = self.workspace.hint() else {
            return;
        };
        let build = (self.workspace == Workspace::Worlds).then_some(self.build.as_str());
        let pad = widgets::BODY_PAD;
        let mut width = widgets::text_width(ui, hint, theme::body());
        if let Some(build) = build {
            width = width.max(widgets::text_width(ui, build, theme::hint()));
        }
        let width = (width + 2.0 * pad).min(safe.width() - 2.0 * MARGIN);
        let line = 24.0;
        let lines = if build.is_some() { 2.0 } else { 1.0 };
        let height = lines * line + 2.0 * 12.0;
        let rect = Rect::from_min_size(
            pos2(
                safe.center().x - width / 2.0,
                safe.bottom() - MARGIN - height,
            ),
            vec2(width, height),
        );
        self.cards.push(("workspace-hint", rect));
        widgets::card(
            ui.ctx(),
            widgets::backdrop(ui.ctx()),
            "workspace-hint",
            rect,
            egui::Order::Middle,
            CornerRadius::same(theme::CARD_RADIUS as u8),
            |ui| {
                let body = ui.max_rect().shrink2(vec2(pad, 12.0));
                let text = |ui: &mut egui::Ui, at: f32, text: &str, font, color| {
                    let rect = Rect::from_min_size(pos2(body.left(), at), vec2(body.width(), line));
                    let label = egui::RichText::new(text).font(font).color(color);
                    ui.put(rect, egui::Label::new(label).truncate());
                };
                text(ui, body.top(), hint, theme::body(), theme::TEXT);
                if let Some(build) = build {
                    text(ui, body.top() + line, build, theme::hint(), theme::MUTE);
                }
            },
        );
    }

    /// Takes the paths that the Files app picked, and shows the picker
    /// that the user asked for.
    pub fn run_picker(&mut self) {
        #[cfg(target_os = "ios")]
        {
            let paths = self.picked.take();
            if !paths.is_empty() {
                let pick = self.picking.take().unwrap_or(Pick::Import);
                for path in paths {
                    self.picked_file(pick, &path);
                }
            }
            if let Some(pick) = self.pick.take()
                && let Some(picker) = &self.picker
            {
                let types: &[&str] = match pick {
                    Pick::Import => &["public.png", "public.tiff", GEOPACKAGE],
                    Pick::Open | Pick::ValeProject => &[GEOPACKAGE],
                };
                if picker.pick(types) {
                    self.picking = Some(pick);
                }
            }
        }
        #[cfg(not(target_os = "ios"))]
        if let Some(pick) = self.pick.take() {
            let path = match pick {
                Pick::Import => vale_shell::picker::pick_file("Heightmap", &IMPORT_TYPES),
                Pick::Open => vale_shell::picker::pick_file("World", &[worlds::EXTENSION]),
                Pick::ValeProject => {
                    vale_shell::picker::pick_file("Vale project", &[worlds::EXTENSION])
                }
            };
            if let Some(path) = path {
                self.picked_file(pick, &path);
            }
        }
    }

    /// Uses a file that the file picker gave.
    pub fn picked_file(&mut self, pick: Pick, path: &Path) {
        match pick {
            Pick::Import => self.import_path(path),
            Pick::Open => self.open_file(path),
            Pick::ValeProject => self.new_from_vale(path),
        }
    }
}

/// The run that a world file keeps. A saved run opens at its own size,
/// also a size that this device does not offer for a new run.
fn restored_run(doc: &Document, threaded: bool) -> Option<Simulation> {
    let saved = doc.file.as_ref()?.load_run().ok().flatten()?;
    Simulation::restore(&doc.world, saved, threaded).ok()
}

/// A text that wraps to the width of `ui`.
/// The year and the month of the next step, for example "Year 3 · Mar".
pub fn year_text(step: u64) -> String {
    let per_year = u64::from(isotherm_sim::STEPS_PER_YEAR);
    let month = (step % per_year) as usize / isotherm_sim::STEPS_PER_MONTH as usize;
    format!("Year {} · {}", step / per_year + 1, MONTHS[month])
}

/// The time of a step and the memory of a run, for example
/// "12 ms per step · 7 MB".
pub fn cost_text(ms_per_step: f64, mb: f64) -> String {
    format!("{ms_per_step:.0} ms per step · {mb:.0} MB")
}

pub fn quality_name(face_size: usize) -> &'static str {
    QUALITY_NAMES
        .iter()
        .find(|(n, _)| *n == face_size)
        .map_or("Custom", |(_, name)| name)
}

/// A small segmented control at the right of `row`.
fn small_segments_in(
    ui: &egui::Ui,
    controls: &mut Controls,
    row: Rect,
    labels: &[&str],
    active: usize,
) -> Option<usize> {
    let width = widgets::small_segments_width(ui, labels).min(row.width() * 0.7);
    let height = (TOUCH - 8.0).min(row.height());
    let rect = Rect::from_min_size(
        pos2(row.right() - width, row.center().y - height / 2.0),
        vec2(width, height),
    );
    widgets::small_segments(ui, controls, rect, labels, active)
}

/// A button with a text and no fill. Its right edge and its middle are at
/// `right`.
fn plain_button(
    ui: &egui::Ui,
    controls: &mut Controls,
    right: egui::Pos2,
    name: &str,
) -> egui::Response {
    let width = widgets::text_width(ui, name, theme::body()) + 36.0;
    let rect = Rect::from_min_size(
        pos2(right.x - width, right.y - TOUCH / 2.0),
        vec2(width, TOUCH),
    );
    let resp = widgets::hit(ui, controls, rect, name, Sense::click(), true, None);
    widgets::fill(ui, rect, &resp, false);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        name,
        theme::body(),
        theme::TEXT,
    );
    resp
}

fn wrapped(ui: &mut egui::Ui, text: &str, font: egui::FontId, color: egui::Color32) {
    let label = egui::RichText::new(text).font(font).color(color);
    ui.add(egui::Label::new(label).wrap());
}

/// A segmented list whose items share the width of the max rect of `ui`.
fn segments_fill(
    ui: &mut egui::Ui,
    controls: &mut Controls,
    items: &[(&str, bool)],
    active: usize,
) -> Option<usize> {
    let card = ui.max_rect();
    let width = (card.width() - 2.0 * PAD) / items.len() as f32;
    let mut chosen = None;
    for (index, &(label, enabled)) in items.iter().enumerate() {
        let min = pos2(card.left() + PAD + index as f32 * width, card.top() + PAD);
        let rect = Rect::from_min_size(min, vec2(width, TOUCH));
        let on = index == active;
        let resp = widgets::hit(ui, controls, rect, label, Sense::click(), enabled, Some(on));
        if on {
            let thumb = rect.shrink(widgets::HIGHLIGHT_INSET);
            ui.painter()
                .rect_filled(thumb, theme::capsule(thumb), theme::raise());
        }
        widgets::fill(ui, rect, &resp, false);
        let color = widgets::fade(if on { theme::TEXT } else { theme::MUTE }, enabled);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            theme::body(),
            color,
        );
        if resp.clicked() && !on {
            chosen = Some(index);
        }
    }
    chosen
}

impl eframe::App for IsothermApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.run_picker();
        self.draw(ui);
    }

    fn on_exit(&mut self) {
        self.flush();
        self.remember();
        if let Some(e) = &self.status {
            eprintln!("isotherm-app: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_year_counter_starts_at_year_one_in_january() {
        assert_eq!(year_text(0), "Year 1 · Jan");
        assert_eq!(year_text(2 * 360 + 2 * 30 + 5), "Year 3 · Mar");
        assert_eq!(cost_text(12.4, 7.2), "12 ms per step · 7 MB");
    }

    #[test]
    fn a_height_has_groups_of_three_digits_and_a_unit() {
        assert_eq!(meters_text(120.4), "120 m");
        assert_eq!(meters_text(-1200.0), "−1,200 m");
        assert_eq!(meters_text(32767.0), "32,767 m");
        assert_eq!(meters_text(0.0), "0 m");
    }
}
