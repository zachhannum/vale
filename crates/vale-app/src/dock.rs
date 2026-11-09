//! The dock of the iPad layout: the column at the right edge of the screen
//! that holds panels. Each workspace has its own dock. The app keeps the
//! docks in a text file in the folder of the state of the last run, with one
//! key and its values on each line. The project file does not hold them.

use std::path::Path;
use std::str::FromStr;

use crate::ui::Workspace;
use crate::ui::pad::Panel;

/// The file in the state folder that holds the docks.
const DOCK_FILE: &str = "dock.txt";

/// The narrowest dock, in points.
pub const WIDTH_MIN: f32 = 280.0;
/// The widest dock, in points.
pub const WIDTH_MAX: f32 = 480.0;
/// The width of a new dock, in points.
pub const WIDTH_DEFAULT: f32 = 320.0;
/// The smallest height of the body of an open panel, in points.
pub const HEIGHT_MIN: f32 = 132.0;
/// The height of the body of a new panel, in points.
pub const HEIGHT_DEFAULT: f32 = 240.0;

/// A panel in a dock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockPanel {
    pub panel: Panel,
    /// Only the title row of the panel shows.
    pub folded: bool,
    /// The height of the body of the panel, in points.
    pub height: f32,
}

impl DockPanel {
    pub fn new(panel: Panel) -> DockPanel {
        DockPanel {
            panel,
            folded: false,
            height: HEIGHT_DEFAULT,
        }
    }
}

/// The dock of one workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct DockState {
    pub open: bool,
    /// The width, from `WIDTH_MIN` to `WIDTH_MAX`.
    pub width: f32,
    /// The panels from top to bottom, in the order of the user.
    pub panels: Vec<DockPanel>,
}

impl Default for DockState {
    fn default() -> DockState {
        DockState {
            open: false,
            width: WIDTH_DEFAULT,
            panels: Vec::new(),
        }
    }
}

impl DockState {
    pub fn set_width(&mut self, width: f32) {
        self.width = clamp_width(width);
    }

    pub fn contains(&self, panel: Panel) -> bool {
        self.panels.iter().any(|p| p.panel == panel)
    }

    /// Adds `panel` below the other panels. Returns `false` if the dock has
    /// the panel already, or if `workspace` does not have the panel.
    pub fn insert(&mut self, workspace: Workspace, panel: DockPanel) -> bool {
        if !Panel::of(workspace).contains(&panel.panel) || self.contains(panel.panel) {
            return false;
        }
        self.panels.push(panel);
        true
    }

    /// Removes `panel`. Returns `false` if the dock does not have it.
    pub fn remove(&mut self, panel: Panel) -> bool {
        let len = self.panels.len();
        self.panels.retain(|p| p.panel != panel);
        self.panels.len() != len
    }
}

fn clamp_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(WIDTH_MIN, WIDTH_MAX)
    } else {
        WIDTH_DEFAULT
    }
}

/// The docks of the two workspaces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Docks {
    pub world: DockState,
    pub atlas: DockState,
}

/// The workspaces with their names in the file.
const WORKSPACES: [(Workspace, &str); 2] =
    [(Workspace::Globe, "world"), (Workspace::Atlas, "atlas")];

impl Docks {
    pub fn get(&self, workspace: Workspace) -> &DockState {
        match workspace {
            Workspace::Globe => &self.world,
            Workspace::Atlas => &self.atlas,
        }
    }

    pub fn get_mut(&mut self, workspace: Workspace) -> &mut DockState {
        match workspace {
            Workspace::Globe => &mut self.world,
            Workspace::Atlas => &mut self.atlas,
        }
    }

    /// The text of the file.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for (workspace, name) in WORKSPACES {
            let dock = self.get(workspace);
            text += &format!("{name}.open {}\n", u8::from(dock.open));
            text += &format!("{name}.width {}\n", dock.width);
            text += &format!("{name}.panels");
            for p in &dock.panels {
                text += &format!(" {}", p.panel.id());
                if p.folded {
                    text += ":folded";
                }
                text += &format!(":{}", p.height);
            }
            text += "\n";
        }
        text
    }

    /// The docks of the text of the file. A bad line or value gets the
    /// default, and a panel that the workspace does not have is left out.
    pub fn parse(text: &str) -> Docks {
        let mut docks = Docks::default();
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let Some((prefix, key)) = words.next().and_then(|k| k.split_once('.')) else {
                continue;
            };
            let Some((workspace, _)) = WORKSPACES.iter().find(|(_, n)| *n == prefix) else {
                continue;
            };
            let dock = docks.get_mut(*workspace);
            match key {
                "open" => dock.open = words.next() == Some("1"),
                "width" => {
                    if let Some(width) = words.next().and_then(|w| w.parse().ok()) {
                        dock.set_width(width);
                    }
                }
                "panels" => {
                    for word in words {
                        if let Some(panel) = parse_panel(word) {
                            dock.insert(*workspace, panel);
                        }
                    }
                }
                _ => {}
            }
        }
        docks
    }
}

/// A panel of the file: `name`, then `:folded` if folded, then `:height`.
fn parse_panel(word: &str) -> Option<DockPanel> {
    let mut parts = word.split(':');
    let mut panel = DockPanel::new(Panel::from_id(parts.next()?)?);
    for part in parts {
        if part == "folded" {
            panel.folded = true;
        } else if let Ok(height) = part.parse::<f32>()
            && height.is_finite()
        {
            panel.height = height.max(HEIGHT_MIN);
        }
    }
    Some(panel)
}

/// A panel of the flag `--dock`: `name` or `name:folded`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub panel: Panel,
    pub folded: bool,
}

impl FromStr for Entry {
    type Err = String;

    fn from_str(text: &str) -> Result<Entry, String> {
        let (name, folded) = match text.split_once(':') {
            Some((name, "folded")) => (name, true),
            Some(_) => return Err(format!("{text}: the suffix can only be :folded")),
            None => (text, false),
        };
        let panel = Panel::from_id(name).ok_or_else(|| format!("{name}: no such panel"))?;
        Ok(Entry { panel, folded })
    }
}

/// Puts the panels of `entries` in the dock of `workspace`, and opens it.
pub fn apply(docks: &mut Docks, workspace: Workspace, entries: &[Entry]) {
    let dock = docks.get_mut(workspace);
    for entry in entries {
        let mut panel = DockPanel::new(entry.panel);
        panel.folded = entry.folded;
        dock.insert(workspace, panel);
    }
    dock.open = true;
}

/// The text of the file in the state folder `dir`. `None`: the folder
/// has no file.
pub fn read(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(DOCK_FILE)).ok()
}

/// Writes the file to the state folder `dir`. A stop of the app during the
/// write leaves the old file.
pub fn write(dir: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{DOCK_FILE}.partial"));
    std::fs::write(&partial, text)?;
    std::fs::rename(partial, dir.join(DOCK_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panels(dock: &DockState) -> Vec<Panel> {
        dock.panels.iter().map(|p| p.panel).collect()
    }

    #[test]
    fn the_text_round_trips() {
        let mut docks = Docks::default();
        docks.world.open = true;
        docks.world.set_width(400.0);
        let mut toolbox = DockPanel::new(Panel::Toolbox);
        toolbox.folded = true;
        toolbox.height = 180.0;
        docks.world.insert(Workspace::Globe, toolbox);
        docks
            .world
            .insert(Workspace::Globe, DockPanel::new(Panel::Layers));
        docks
            .atlas
            .insert(Workspace::Atlas, DockPanel::new(Panel::Pages));
        let text = docks.text();
        assert!(
            text.contains("world.panels toolbox:folded:180 layers:240\n"),
            "{text}"
        );
        assert_eq!(Docks::parse(&text), docks);
    }

    #[test]
    fn a_bad_text_gets_the_defaults() {
        assert_eq!(Docks::parse(""), Docks::default());
        let docks =
            Docks::parse("world.open yes\nworld.width wide\nworld.panels nothing:1 brush\n");
        assert_eq!(docks, Docks::default());
        let docks = Docks::parse("world.panels layers:20\n");
        assert_eq!(docks.world.panels[0].height, HEIGHT_MIN);
    }

    #[test]
    fn the_width_stays_in_its_range() {
        let mut dock = DockState::default();
        assert_eq!(dock.width, 320.0);
        dock.set_width(100.0);
        assert_eq!(dock.width, WIDTH_MIN);
        dock.set_width(900.0);
        assert_eq!(dock.width, WIDTH_MAX);
        dock.set_width(f32::NAN);
        assert_eq!(dock.width, WIDTH_DEFAULT);
        assert_eq!(Docks::parse("atlas.width 1000\n").atlas.width, WIDTH_MAX);
    }

    #[test]
    fn a_panel_goes_below_the_other_panels() {
        let mut dock = DockState::default();
        let world = Workspace::Globe;
        assert!(dock.insert(world, DockPanel::new(Panel::Toolbox)));
        assert!(dock.insert(world, DockPanel::new(Panel::Layers)));
        assert!(dock.insert(world, DockPanel::new(Panel::Attributes)));
        assert!(
            !dock.insert(world, DockPanel::new(Panel::Layers)),
            "a second one"
        );
        assert!(
            !dock.insert(world, DockPanel::new(Panel::Brush)),
            "not in the menu"
        );
        assert!(
            !dock.insert(world, DockPanel::new(Panel::Pages)),
            "an atlas panel"
        );
        let order = [Panel::Toolbox, Panel::Layers, Panel::Attributes];
        assert_eq!(panels(&dock), order);

        let mut dock = DockState::default();
        let atlas = Workspace::Atlas;
        for &panel in Panel::of(atlas).iter().rev() {
            assert!(dock.insert(atlas, DockPanel::new(panel)));
        }
        assert!(!dock.insert(atlas, DockPanel::new(Panel::Layers)));
        assert!(!dock.insert(atlas, DockPanel::new(Panel::Toolbox)));
        assert!(dock.remove(Panel::Pages));
        assert!(!dock.remove(Panel::Pages));
        assert_eq!(
            panels(&dock),
            [
                Panel::Attributes,
                Panel::Properties,
                Panel::Labels,
                Panel::MapLayers,
                Panel::Projection,
                Panel::Frame,
            ]
        );
    }

    #[test]
    fn the_order_of_the_file_stays() {
        let docks = Docks::parse("atlas.panels pages labels:folded frame\n");
        assert_eq!(
            panels(&docks.atlas),
            [Panel::Pages, Panel::Labels, Panel::Frame]
        );
        assert!(docks.atlas.panels[1].folded);
    }

    #[test]
    fn an_old_panel_is_left_out() {
        let docks = Docks::parse("atlas.panels selection:240 pages:240 maps:folded:240\n");
        assert_eq!(panels(&docks.atlas), [Panel::Pages]);
    }

    #[test]
    fn an_entry_reads_a_name_and_folded() {
        let entry: Entry = "toolbox:folded".parse().unwrap();
        assert_eq!((entry.panel, entry.folded), (Panel::Toolbox, true));
        let entry: Entry = "map-layers".parse().unwrap();
        assert_eq!((entry.panel, entry.folded), (Panel::MapLayers, false));
        assert!("selection".parse::<Entry>().is_err());
        assert!("toolbox:open".parse::<Entry>().is_err());
        assert!("wheel".parse::<Entry>().is_err());
    }

    #[test]
    fn each_panel_id_reads_back() {
        for panel in Panel::ALL {
            assert_eq!(Panel::from_id(panel.id()), Some(panel));
        }
        assert_eq!(Panel::from_id("Layers"), None);
    }
}
