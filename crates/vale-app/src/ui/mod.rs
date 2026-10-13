//! The UI: state, actions, and the draw function.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use vale_labeler::Fonts;
use vale_sphere::LonLat;
use vale_store::{LayerId, Project, StoreError, World};

use crate::document::Document;
use crate::files::{Availability, Files, PlainFiles, SystemEvents};
use crate::globe::import::ImportResult;
use crate::globe::{Globe, Tool};
use crate::headless;
use crate::pipeline::{Composed, Pipeline, Quality, Selection, fonts};
use crate::projects::{self, Location, Locations};
use crate::resume::{self, Resume};
use crate::session::{self, Session};
use crate::workcopy;

pub mod browser;
pub mod canvas;
pub mod elevation;
pub mod globe;
pub mod inspector;
pub mod pad;
pub mod panels;
pub mod world_form;

/// A request that needs the host: a dialog or a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    OpenDialog,
    OpenFiles(Vec<PathBuf>),
    ExportDialog(ExportFormat),
    Export(PathBuf),
    ImportHeightmapDialog,
    /// Imports this equirectangular image into the heightmap of the globe.
    ImportHeightmap(PathBuf),
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

/// The text of the last import of a heightmap.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    /// The import failed, or it changed the shape of the image.
    pub warning: bool,
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
    /// The flat map of one map frame.
    Map,
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
    pub composed: Option<Composed>,
    pub texture: Option<egui::TextureHandle>,
    /// The map must be composed again.
    pub dirty: bool,
    /// Quality of the last compose.
    pub quality: Quality,
    /// Egui time in seconds.
    pub last_view_change: Option<f64>,
    /// Size of the canvas in points.
    pub canvas_size: (f64, f64),
    /// Top-left corner of the canvas in screen points. Set by the canvas each frame.
    pub canvas_origin: (f32, f32),
    pub selection: Option<Selection>,
    pub selected_layer: Option<LayerId>,
    pub cursor_lonlat: Option<LonLat>,
    /// The elevation of the heightmap under the cursor, in meters.
    pub cursor_meters: Option<f64>,
    /// Last message, for example an import error.
    pub status: String,
    /// The text of the last import of a heightmap.
    pub import_note: Option<Note>,
    /// The open form of a new world or of the world of the project.
    pub world_form: Option<world_form::WorldForm>,
    /// Requests that need the host (dialogs, files).
    pub actions: Vec<Action>,
    /// True: no timers, always `Quality::Final`.
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
    /// The edit count of the globe at the last save.
    saved_edits: u64,
    /// The time of the last save, for the save mark.
    saved_at: Option<Instant>,
    /// The last save failed.
    save_failed: bool,
    /// The state for the next start that the projects folder holds.
    resume: Option<Resume>,
    /// The time of the last write of the state for the next start.
    resume_at: Option<Instant>,
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
            texture: None,
            dirty: true,
            quality: Quality::Final,
            last_view_change: None,
            canvas_size: (0.0, 0.0),
            canvas_origin: (0.0, 0.0),
            selection: None,
            selected_layer: None,
            cursor_lonlat: None,
            cursor_meters: None,
            status: String::new(),
            import_note: None,
            world_form: None,
            actions: Vec::new(),
            headless: false,
            file_buttons: true,
            frames: 0,
            session: None,
            projects: Locations::default(),
            files: Box::new(PlainFiles),
            system: SystemEvents::default(),
            saved_edits: 0,
            saved_at: None,
            save_failed: false,
            resume: None,
            resume_at: None,
        };
        state.set_layout(Layout::default());
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

    /// True if the undo log of the project file has the edits. If not, the
    /// heightmap keeps the strokes in memory.
    fn file_log(&self) -> bool {
        let session = self.session.as_ref();
        session.is_some_and(|s| s.saves_map(&self.globe.map))
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
        let map = &mut self.globe.map;
        let result = if redo {
            session.redo(&mut self.doc, map)
        } else {
            session.undo(&mut self.doc, map)
        };
        match result {
            Ok(_) => {
                let gone = |id| self.doc.layer(id).is_none();
                if self.selected_layer.is_some_and(gone) {
                    self.selected_layer = None;
                }
                if self.selection.is_some_and(|s| gone(s.layer)) {
                    self.selection = None;
                }
                // A layer can come back with other features under its ID.
                self.pipeline = Pipeline::new();
                self.touch();
            }
            Err(e) => {
                let what = if redo { "redo" } else { "undo" };
                self.status = format!("The {what} did not work: {e}");
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
        let resume_wait = self.resume_tick(hold, now);
        let session = self.session.as_mut()?;
        let saves = session.saves();
        let globe = &mut self.globe;
        let project = &mut self.doc.project;
        let result = if globe.edits() != self.saved_edits {
            // The edit is one undo step, with no part of the next stroke.
            self.saved_edits = globe.edits();
            let saved = session.flush(project, &mut globe.map);
            let saved = saved.and_then(|()| session.save_thumbnail(&globe.map, &globe.view));
            saved.map(|()| None)
        } else {
            let hold = hold || globe.busy();
            session.tick(project, &mut globe.map, hold, now)
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
                self.status = format!("The project did not save: {e}");
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
        } else if session.waits() || self.saved_at.is_some_and(turns) {
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
        let Some(session) = &mut self.session else {
            return Ok(());
        };
        let globe = &mut self.globe;
        if globe.busy() {
            return Ok(());
        }
        self.saved_edits = globe.edits();
        let saved = session.flush(&mut self.doc.project, &mut globe.map);
        let saved = saved.and_then(|()| session.save_thumbnail(&globe.map, &globe.view));
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
            Err(e) => self.status = format!("The project did not save: {e}"),
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
        }
    }

    /// True if the world has a raster layer. The heightmap is a raster layer
    /// from its first painted tile.
    pub fn has_raster(&self) -> bool {
        !self.doc.project.rasters().is_empty() || self.globe.map.allocated().next().is_some()
    }

    /// Saves each change that waits and the picture of the globe to the
    /// project file. Returns `false` if the save failed. The world then
    /// stays open.
    pub fn save_now(&mut self) -> bool {
        let Some(session) = &mut self.session else {
            return true;
        };
        let globe = &mut self.globe;
        // The file gets each stroke that ended, and no part of a stroke that
        // still waits for its texels.
        if globe.busy() {
            globe.map.cancel_stroke();
        }
        let saved = session.flush(&mut self.doc.project, &mut globe.map);
        let saved = saved.and_then(|()| session.save_thumbnail(&globe.map, &globe.view));
        match saved.and_then(|()| self.write_back()) {
            Ok(()) => true,
            Err(e) => {
                self.status = format!("The project did not save, so the world stays open: {e}");
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
            self.status = format!("The app did not save the changes of the last run: {error}");
        } else if let Some(path) = saved.first() {
            self.status = format!("Saved the changes of the last run to \"{}\"", name(path));
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
                    "Another device changed this world. Your version is \"{}\".",
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
        self.saved_edits = self.globe.edits();
        self.selection = None;
        self.selected_layer = None;
        self.import_note = None;
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
        self.status = format!("New world: {}", world.name);
        let dir = self.projects.new_world_dir();
        let path = dir.map(|dir| session::new_project_path(dir, &world.name));
        self.doc.set_project(Project::new(world));
        self.globe.clear();
        self.start_world();
        if let Some(path) = path {
            match self.open_session(&path, None) {
                Ok(session) => self.session = Some(session),
                Err(e) => self.status = format!("The new world has no file: {e}"),
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
                    return Err("A world must have a name.".to_string());
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
                self.status = format!("Made {}", name.map_err(text)?.name);
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
            Err(e) => self.status = format!("The app did not copy the file: {e}"),
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
            .and_then(|session| session.load_map().map(|map| (session, map)));
        let (session, map) = match opened {
            Ok(opened) => opened,
            Err(e) => {
                self.status = format!("The world did not open: {e}");
                return;
            }
        };
        self.doc = doc;
        self.status = format!("Opened {}", self.doc.project.world.name);
        if !self.globe.open_map(map) {
            self.status = "This device cannot show the heightmap of the project. \
                           The project keeps it, and it does not save the painting."
                .to_string();
        }
        self.session = Some(session);
        self.start_world();
        self.show_world();
    }

    /// Marks the map as needing a new compose.
    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// Moves the result of an import of a heightmap to the status text.
    pub fn poll_import(&mut self) {
        if let Some(result) = self.globe.take_import_result() {
            self.note_import(result);
        }
    }

    /// Shows the result of an import of a heightmap.
    pub fn note_import(&mut self, result: ImportResult) {
        let note = match result {
            Ok(info) => Note {
                text: info.message(),
                warning: !info.two_to_one,
            },
            Err(text) => Note {
                text,
                warning: true,
            },
        };
        self.status = note.text.clone();
        self.import_note = Some(note);
    }

    /// Runs `OpenFiles`, `Export`, `ImportHeightmap`, and `NewWorld`. The
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
                self.status = match headless::export(&self.doc, self.canvas_size, 2.0, &path) {
                    Ok(_) => format!("Exported {}", path.display()),
                    Err(e) => format!("{e:#}"),
                };
                self.touch();
            }
            Action::ImportHeightmap(path) => {
                self.import_note = None;
                self.globe.start_import(path);
                if self.headless {
                    self.globe.wait_import();
                }
                self.poll_import();
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
                    Err(e) => self.status = format!("The download did not start: {e}"),
                }
                self.reload_projects();
            }
            Action::KeepVersion(path, keep) => {
                if let Err(e) = self.files.resolve(&path, keep) {
                    self.status = format!("The app did not keep the version: {e}");
                }
                self.reload_projects();
            }
            Action::OpenDialog
            | Action::ExportDialog(_)
            | Action::ImportHeightmapDialog
            | Action::OpenProjectDialog => {}
        }
    }
}

/// Draws the whole UI.
pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    state.globe.stats.tick(std::time::Instant::now());
    state.poll_import();
    if state.screen == Screen::Browser {
        if state.poll_downloads(Instant::now()) {
            ui.ctx().request_repaint_after(DOWNLOAD_POLL);
        }
        browser::draw(ui, state);
        state.frames += 1;
        return;
    }
    browser::dialogs(ui, state);
    if state.layout == Layout::Pad && state.workspace == Workspace::Globe {
        pad::draw(ui, state);
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
            Workspace::Map => {
                panels::left(ui, state);
                inspector::draw(ui, state);
                panels::status(ui, state);
                canvas::draw(ui, state);
            }
        }
    });
    world_form::desktop(ui.ctx(), state);
    state.frames += 1;
}
