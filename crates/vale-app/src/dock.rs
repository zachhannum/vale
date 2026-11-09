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

/// A panel in a dock, as a group of its own.
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

/// A group of the dock: one panel, or a stack of panels with a tab for
/// each. Only the body of the active panel shows.
#[derive(Clone, Debug, PartialEq)]
pub struct DockGroup {
    /// The panels in the order of their tabs. A group has at least one.
    pub panels: Vec<Panel>,
    /// The index of the active panel in `panels`.
    pub active: usize,
    /// Only the title row of the group shows.
    pub folded: bool,
    /// The height of the body of the group, in points.
    pub height: f32,
}

impl DockGroup {
    fn of(panel: DockPanel) -> DockGroup {
        DockGroup {
            panels: vec![panel.panel],
            active: 0,
            folded: panel.folded,
            height: panel.height,
        }
    }

    /// The panel whose body shows.
    pub fn active_panel(&self) -> Panel {
        self.panels[self.active.min(self.panels.len() - 1)]
    }

    /// The group has more than one panel, and shows a tab row.
    pub fn is_stack(&self) -> bool {
        self.panels.len() > 1
    }
}

/// A place in the dock where a dragged panel can go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropAt {
    /// A group of its own, before the group at this index. The number of
    /// the groups is the place below the last group.
    Between(usize),
    /// A new tab of the group at this index.
    Stack(usize),
}

/// The dock of one workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct DockState {
    pub open: bool,
    /// The width, from `WIDTH_MIN` to `WIDTH_MAX`.
    pub width: f32,
    /// The groups from top to bottom, in the order of the user.
    pub groups: Vec<DockGroup>,
}

impl Default for DockState {
    fn default() -> DockState {
        DockState {
            open: false,
            width: WIDTH_DEFAULT,
            groups: Vec::new(),
        }
    }
}

impl DockState {
    pub fn set_width(&mut self, width: f32) {
        self.width = clamp_width(width);
    }

    pub fn contains(&self, panel: Panel) -> bool {
        self.position(panel).is_some()
    }

    /// The index of the group of `panel`, and the index of its tab.
    pub fn position(&self, panel: Panel) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(g, group)| {
            let i = group.panels.iter().position(|&p| p == panel)?;
            Some((g, i))
        })
    }

    /// The group of `panel`.
    pub fn group_of(&self, panel: Panel) -> Option<&DockGroup> {
        self.position(panel).map(|(g, _)| &self.groups[g])
    }

    /// The panels from top to bottom, each with the fold and the height of
    /// its group.
    pub fn panels(&self) -> Vec<DockPanel> {
        let mut out = Vec::new();
        for g in &self.groups {
            out.extend(g.panels.iter().map(|&panel| DockPanel {
                panel,
                folded: g.folded,
                height: g.height,
            }));
        }
        out
    }

    /// True if `panel` is the active panel of its group.
    pub fn is_active(&self, panel: Panel) -> bool {
        self.group_of(panel)
            .is_some_and(|g| g.active_panel() == panel)
    }

    /// Makes `panel` the active panel of its group. Returns `false` if the
    /// dock does not have it.
    pub fn activate(&mut self, panel: Panel) -> bool {
        let Some((g, i)) = self.position(panel) else {
            return false;
        };
        self.groups[g].active = i;
        true
    }

    /// Adds `panel` as a group below the other groups. Returns `false` if
    /// the dock has the panel already, or if `workspace` does not have the
    /// panel.
    pub fn insert(&mut self, workspace: Workspace, panel: DockPanel) -> bool {
        self.insert_group(workspace, DockGroup::of(panel))
    }

    /// Adds `group` below the other groups, without the panels that the
    /// dock has already or that `workspace` does not have. Returns `false`
    /// if no panel is left.
    pub fn insert_group(&mut self, workspace: Workspace, mut group: DockGroup) -> bool {
        let active = group.active_panel();
        let mut panels: Vec<Panel> = Vec::new();
        for p in group.panels {
            if Panel::of(workspace).contains(&p) && !self.contains(p) && !panels.contains(&p) {
                panels.push(p);
            }
        }
        if panels.is_empty() {
            return false;
        }
        group.active = panels.iter().position(|&p| p == active).unwrap_or(0);
        group.panels = panels;
        group.height = group.height.max(HEIGHT_MIN);
        self.groups.push(group);
        true
    }

    /// Removes `panel` from its group. A group with no panel goes away.
    /// Returns `false` if the dock does not have the panel.
    pub fn remove(&mut self, panel: Panel) -> bool {
        let Some((g, i)) = self.position(panel) else {
            return false;
        };
        let group = &mut self.groups[g];
        group.panels.remove(i);
        if group.panels.is_empty() {
            self.groups.remove(g);
        } else if group.active > i || group.active >= group.panels.len() {
            group.active = group.active.saturating_sub(1);
        }
        true
    }

    /// Adds `panel` if the dock does not have it, makes it the active panel
    /// of its group, and opens the group.
    pub fn show(&mut self, workspace: Workspace, panel: Panel) {
        self.insert(workspace, DockPanel::new(panel));
        if let Some((g, i)) = self.position(panel) {
            self.groups[g].active = i;
            self.groups[g].folded = false;
        }
    }

    /// True if a drop of `panel` at `at` changes the dock.
    pub fn can_drop(&self, panel: Panel, at: DropAt) -> bool {
        let Some((g, _)) = self.position(panel) else {
            return false;
        };
        let alone = !self.groups[g].is_stack();
        match at {
            DropAt::Stack(t) => t < self.groups.len() && t != g,
            DropAt::Between(t) => t <= self.groups.len() && !(alone && (t == g || t == g + 1)),
        }
    }

    /// True if a move of the group of `panel` before the group at `t`
    /// changes the dock.
    pub fn can_move_group(&self, panel: Panel, t: usize) -> bool {
        self.position(panel)
            .is_some_and(|(g, _)| t <= self.groups.len() && t != g && t != g + 1)
    }

    /// Moves the group of `panel` before the group at `t`, or below the
    /// last group if `t` is the number of the groups. Returns `false` if
    /// nothing changes.
    pub fn move_group(&mut self, panel: Panel, t: usize) -> bool {
        if !self.can_move_group(panel, t) {
            return false;
        }
        let Some((g, _)) = self.position(panel) else {
            return false;
        };
        let group = self.groups.remove(g);
        self.groups.insert(if t > g { t - 1 } else { t }, group);
        true
    }

    /// Moves `panel` to `at`. A panel that is alone in its group moves with
    /// the fold and the height of the group. A panel that leaves a stack
    /// takes the height of the stack and opens. A panel that goes into a
    /// stack is its active panel. Returns `false` if nothing changes.
    pub fn move_panel(&mut self, panel: Panel, at: DropAt) -> bool {
        if !self.can_drop(panel, at) {
            return false;
        }
        let Some((g, _)) = self.position(panel) else {
            return false;
        };
        let source = self.groups[g].clone();
        let alone = !source.is_stack();
        self.remove(panel);
        let shift = |t: usize| if alone && t > g { t - 1 } else { t };
        match at {
            DropAt::Between(t) => {
                let group = DockGroup {
                    panels: vec![panel],
                    active: 0,
                    folded: alone && source.folded,
                    height: source.height,
                };
                self.groups.insert(shift(t), group);
            }
            DropAt::Stack(t) => {
                let group = &mut self.groups[shift(t)];
                group.panels.push(panel);
                group.active = group.panels.len() - 1;
            }
        }
        true
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
            for g in &dock.groups {
                let ids: Vec<&str> = g.panels.iter().map(|p| p.id()).collect();
                text += &format!(" {}", ids.join("+"));
                if g.active > 0 {
                    text += &format!(":tab={}", g.active);
                }
                if g.folded {
                    text += ":folded";
                }
                text += &format!(":{}", g.height);
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
                        if let Some(group) = parse_group(word) {
                            dock.insert_group(*workspace, group);
                        }
                    }
                }
                _ => {}
            }
        }
        docks
    }
}

/// A group of the file: the names of its panels joined by `+`, then
/// `:tab=` and the index of the active panel if it is not the first, then
/// `:folded` if folded, then `:height`. A name that is not a panel is left
/// out.
fn parse_group(word: &str) -> Option<DockGroup> {
    let mut parts = word.split(':');
    let panels: Vec<Panel> = parts
        .next()?
        .split('+')
        .filter_map(Panel::from_id)
        .collect();
    if panels.is_empty() {
        return None;
    }
    let mut group = DockGroup {
        panels,
        active: 0,
        folded: false,
        height: HEIGHT_DEFAULT,
    };
    for part in parts {
        if part == "folded" {
            group.folded = true;
        } else if let Some(tab) = part.strip_prefix("tab=") {
            group.active = tab.parse().unwrap_or(0);
        } else if let Ok(height) = part.parse::<f32>()
            && height.is_finite()
        {
            group.height = height.max(HEIGHT_MIN);
        }
    }
    group.active = group.active.min(group.panels.len() - 1);
    Some(group)
}

/// A group of the flag `--dock`: the names of its panels joined by `+`,
/// then `:folded` if folded.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub panels: Vec<Panel>,
    pub folded: bool,
}

impl Entry {
    /// An entry of one open panel.
    pub fn new(panel: Panel) -> Entry {
        Entry {
            panels: vec![panel],
            folded: false,
        }
    }
}

impl FromStr for Entry {
    type Err = String;

    fn from_str(text: &str) -> Result<Entry, String> {
        let (names, folded) = match text.split_once(':') {
            Some((names, "folded")) => (names, true),
            Some(_) => return Err(format!("{text}: the suffix can only be :folded")),
            None => (text, false),
        };
        let panels = names
            .split('+')
            .map(|name| Panel::from_id(name).ok_or_else(|| format!("{name}: no such panel")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Entry { panels, folded })
    }
}

/// Puts the groups of `entries` in the dock of `workspace`, and opens it.
pub fn apply(docks: &mut Docks, workspace: Workspace, entries: &[Entry]) {
    let dock = docks.get_mut(workspace);
    for entry in entries {
        let group = DockGroup {
            panels: entry.panels.clone(),
            active: 0,
            folded: entry.folded,
            height: HEIGHT_DEFAULT,
        };
        dock.insert_group(workspace, group);
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
        dock.panels().iter().map(|p| p.panel).collect()
    }

    /// The panels of each group.
    fn groups(dock: &DockState) -> Vec<Vec<Panel>> {
        dock.groups.iter().map(|g| g.panels.clone()).collect()
    }

    fn world(panels: &[Panel]) -> DockState {
        let mut dock = DockState::default();
        for &p in panels {
            dock.insert(Workspace::Globe, DockPanel::new(p));
        }
        dock
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
        assert_eq!(docks.world.groups[0].height, HEIGHT_MIN);
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
        assert!(docks.atlas.groups[1].folded);
    }

    #[test]
    fn an_old_panel_is_left_out() {
        let docks = Docks::parse("atlas.panels selection:240 pages:240 maps:folded:240\n");
        assert_eq!(panels(&docks.atlas), [Panel::Pages]);
    }

    #[test]
    fn an_entry_reads_names_and_folded() {
        let entry: Entry = "toolbox:folded".parse().unwrap();
        assert_eq!((entry.panels, entry.folded), (vec![Panel::Toolbox], true));
        let entry: Entry = "map-layers".parse().unwrap();
        assert_eq!(entry, Entry::new(Panel::MapLayers));
        let entry: Entry = "layers+attributes:folded".parse().unwrap();
        let stack = vec![Panel::Layers, Panel::Attributes];
        assert_eq!((entry.panels, entry.folded), (stack, true));
        assert!("layers+wheel".parse::<Entry>().is_err());
        assert!("selection".parse::<Entry>().is_err());
        assert!("toolbox:open".parse::<Entry>().is_err());
        assert!("wheel".parse::<Entry>().is_err());
    }

    #[test]
    fn a_stack_round_trips_with_its_active_tab() {
        let mut docks = Docks::default();
        let mut dock = world(&[Panel::Layers, Panel::Attributes, Panel::Toolbox]);
        assert!(dock.move_panel(Panel::Toolbox, DropAt::Stack(0)));
        dock.groups[0].folded = true;
        dock.groups[0].height = 300.0;
        docks.world = dock;
        let text = docks.text();
        assert!(
            text.contains("world.panels layers+toolbox:tab=1:folded:300 attributes:240\n"),
            "{text}"
        );
        assert_eq!(Docks::parse(&text), docks);
        // A bad tab, a name that is not a panel, and a panel in two groups.
        let docks = Docks::parse("world.panels layers+brush+toolbox:tab=7 toolbox+attributes\n");
        assert_eq!(
            groups(&docks.world),
            [vec![Panel::Layers, Panel::Toolbox], vec![Panel::Attributes]]
        );
        assert_eq!(docks.world.groups[0].active, 1);
    }

    #[test]
    fn the_old_text_reads_as_groups_of_one_panel() {
        let docks = Docks::parse("world.panels toolbox:folded:180 layers:240\n");
        assert_eq!(
            groups(&docks.world),
            [vec![Panel::Toolbox], vec![Panel::Layers]]
        );
        assert!(docks.world.groups[0].folded);
        assert_eq!(docks.world.groups[0].height, 180.0);
    }

    #[test]
    fn a_remove_leaves_a_stack_or_a_plain_group() {
        let mut dock = world(&[Panel::Layers, Panel::Attributes, Panel::Toolbox]);
        dock.move_panel(Panel::Attributes, DropAt::Stack(0));
        dock.move_panel(Panel::Toolbox, DropAt::Stack(0));
        assert_eq!(
            groups(&dock),
            [vec![Panel::Layers, Panel::Attributes, Panel::Toolbox]]
        );
        assert!(dock.is_active(Panel::Toolbox));
        assert!(dock.activate(Panel::Attributes));
        assert!(dock.remove(Panel::Layers));
        assert!(dock.is_active(Panel::Attributes), "the active panel stays");
        assert!(dock.remove(Panel::Attributes));
        assert_eq!(groups(&dock), [vec![Panel::Toolbox]]);
        assert!(!dock.groups[0].is_stack());
        assert!(dock.remove(Panel::Toolbox));
        assert!(dock.groups.is_empty());
    }

    #[test]
    fn a_move_reorders_stacks_and_unstacks() {
        let [l, a, t] = [Panel::Layers, Panel::Attributes, Panel::Toolbox];
        let mut dock = world(&[l, a, t]);
        dock.groups[0].folded = true;
        dock.groups[0].height = 200.0;
        // A group moves down with its fold and its height.
        assert!(dock.move_panel(l, DropAt::Between(3)));
        assert_eq!(groups(&dock), [vec![a], vec![t], vec![l]]);
        assert!(dock.groups[2].folded);
        assert_eq!(dock.groups[2].height, 200.0);
        // A drop next to its own place changes nothing.
        assert!(!dock.move_panel(l, DropAt::Between(2)));
        assert!(!dock.move_panel(l, DropAt::Between(3)));
        assert!(!dock.move_panel(l, DropAt::Stack(2)));
        assert!(!dock.move_panel(l, DropAt::Between(4)));
        assert!(dock.move_panel(l, DropAt::Between(0)));
        assert_eq!(groups(&dock), [vec![l], vec![a], vec![t]]);
        // A stack, and an unstack between the groups.
        assert!(dock.move_panel(t, DropAt::Stack(0)));
        assert_eq!(groups(&dock), [vec![l, t], vec![a]]);
        assert!(dock.is_active(t));
        assert!(dock.move_panel(l, DropAt::Between(2)));
        assert_eq!(groups(&dock), [vec![t], vec![a], vec![l]]);
        assert!(!dock.groups[2].folded, "a panel that leaves a stack opens");
        assert!(dock.move_panel(a, DropAt::Stack(0)));
        // A panel of a stack can go above or below its stack.
        assert!(dock.move_panel(a, DropAt::Between(0)));
        assert_eq!(groups(&dock), [vec![a], vec![t], vec![l]]);
        assert!(!dock.move_panel(Panel::Pages, DropAt::Between(0)));
    }

    #[test]
    fn a_stack_moves_as_a_whole() {
        let [l, a, t] = [Panel::Layers, Panel::Attributes, Panel::Toolbox];
        let mut dock = world(&[l, a, t]);
        dock.move_panel(a, DropAt::Stack(0));
        assert_eq!(groups(&dock), [vec![l, a], vec![t]]);
        assert!(!dock.move_group(a, 0));
        assert!(!dock.move_group(a, 1));
        assert!(dock.move_group(l, 2));
        assert_eq!(groups(&dock), [vec![t], vec![l, a]]);
        assert!(dock.is_active(a), "the stack keeps its active panel");
        assert!(dock.move_group(a, 0));
        assert_eq!(groups(&dock), [vec![l, a], vec![t]]);
    }

    #[test]
    fn the_flag_puts_stacks_in_the_dock() {
        let mut docks = Docks::default();
        let entries: Vec<Entry> = ["layers+attributes", "toolbox:folded"]
            .iter()
            .map(|e| e.parse().unwrap())
            .collect();
        apply(&mut docks, Workspace::Globe, &entries);
        assert!(docks.world.open);
        assert_eq!(
            groups(&docks.world),
            [vec![Panel::Layers, Panel::Attributes], vec![Panel::Toolbox]]
        );
        assert!(docks.world.is_active(Panel::Layers));
        assert!(docks.world.groups[1].folded);
    }

    #[test]
    fn each_panel_id_reads_back() {
        for panel in Panel::ALL {
            assert_eq!(Panel::from_id(panel.id()), Some(panel));
        }
        assert_eq!(Panel::from_id("Layers"), None);
    }
}
