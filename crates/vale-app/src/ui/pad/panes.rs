//! The dock of the iPad layout: the panes in one column on the right edge.
//! A pane holds one or more panels as tabs. This model has no egui.

use std::path::Path;

use super::Panel;
use super::geometry::ROW;

/// The width of the dock at first.
pub const WIDTH: f32 = 340.0;
/// The narrowest dock.
pub const MIN_WIDTH: f32 = 300.0;
/// The widest dock.
pub const MAX_WIDTH: f32 = 520.0;
/// From this width, each tab shows its name.
pub const NAMES_WIDTH: f32 = 420.0;
/// The height of the bar between two panes.
pub const BAR: f32 = 14.0;
/// A pane is never shorter than its header and two rows.
pub const PANE_MIN: f32 = ROW + 2.0 * 48.0;

/// The file of the docks in the state folder.
const FILE: &str = "panels.txt";

/// The names of the panels in the file.
const NAMES: [(Panel, &str); 4] = [
    (Panel::Brush, "brush"),
    (Panel::Layers, "layers"),
    (Panel::Toolbox, "toolbox"),
    (Panel::Attributes, "attributes"),
];

/// A pane of the dock.
#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub tabs: Vec<Panel>,
    /// The index of the open tab.
    pub active: usize,
    /// A pinned pane stays open until the user closes it.
    pub pinned: bool,
    /// The height that the user gave the pane. `None`: the height of its
    /// content.
    pub height: Option<f32>,
}

impl Pane {
    pub fn new(panel: Panel, pinned: bool) -> Pane {
        Pane {
            tabs: vec![panel],
            active: 0,
            pinned,
            height: None,
        }
    }

    /// The open tab.
    pub fn panel(&self) -> Panel {
        self.tabs[self.active.min(self.tabs.len() - 1)]
    }
}

/// Where a tab goes at the end of a drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drop {
    /// Into the pane with this index, as the tab with this index.
    Merge { pane: usize, index: usize },
    /// Into a new pane, at this index of the column.
    Split { at: usize },
}

/// The dock of one workspace.
#[derive(Clone, Debug, PartialEq)]
pub struct Dock {
    pub width: f32,
    pub panes: Vec<Pane>,
    /// The panel that opened or that the user chose last. A compact screen
    /// shows its pane.
    pub focus: Option<Panel>,
    /// The height of the room for the panes in the last frame.
    pub room: f32,
}

impl Default for Dock {
    fn default() -> Dock {
        Dock {
            width: WIDTH,
            panes: Vec::new(),
            focus: None,
            room: f32::INFINITY,
        }
    }
}

impl Dock {
    /// The index of the pane of a panel, and the index of its tab.
    pub fn find(&self, panel: Panel) -> Option<(usize, usize)> {
        self.panes.iter().enumerate().find_map(|(i, pane)| {
            let tab = pane.tabs.iter().position(|p| *p == panel)?;
            Some((i, tab))
        })
    }

    /// The panel is a tab of a pane.
    pub fn is_open(&self, panel: Panel) -> bool {
        self.find(panel).is_some()
    }

    /// The panel is the open tab of its pane.
    pub fn shows(&self, panel: Panel) -> bool {
        self.find(panel)
            .is_some_and(|(i, tab)| self.panes[i].active == tab)
    }

    /// The panel is in a pinned pane.
    pub fn is_pinned(&self, panel: Panel) -> bool {
        self.find(panel).is_some_and(|(i, _)| self.panes[i].pinned)
    }

    /// The most panes that fit in the room.
    fn most(&self) -> usize {
        ((self.room + BAR) / (PANE_MIN + BAR)).floor().max(1.0) as usize
    }

    /// Shows a panel. An open panel becomes the open tab of its pane. A
    /// closed panel takes the place of the unpinned pane, or else opens in
    /// a new unpinned pane at the bottom. With no room for a new pane, it
    /// becomes a tab of the last pane.
    pub fn open(&mut self, panel: Panel) {
        self.focus = Some(panel);
        if let Some((i, tab)) = self.find(panel) {
            self.panes[i].active = tab;
            return;
        }
        let pane = Pane::new(panel, false);
        if let Some(i) = self.panes.iter().position(|p| !p.pinned) {
            self.panes[i] = pane;
        } else if self.panes.len() < self.most() {
            self.panes.push(pane);
        } else if let Some(last) = self.panes.last_mut() {
            last.tabs.push(panel);
            last.active = last.tabs.len() - 1;
        }
    }

    /// Opens a panel in a new pinned pane at the bottom.
    pub fn open_pinned(&mut self, panel: Panel) {
        self.close(panel);
        self.panes.push(Pane::new(panel, true));
        self.focus = Some(panel);
    }

    /// Opens the panels as the tabs of one new pinned pane.
    pub fn open_tabs(&mut self, panels: &[Panel]) {
        for panel in panels {
            self.close(*panel);
        }
        let mut tabs = panels.to_vec();
        tabs.dedup();
        if tabs.is_empty() {
            return;
        }
        self.focus = tabs.first().copied();
        self.panes.push(Pane {
            tabs,
            active: 0,
            pinned: true,
            height: None,
        });
    }

    pub fn close(&mut self, panel: Panel) {
        let Some((i, tab)) = self.find(panel) else {
            return;
        };
        self.remove_tab(i, tab);
        if self.focus == Some(panel) {
            self.focus = self.panes.last().map(Pane::panel);
        }
    }

    /// Closes the pane with this index.
    pub fn close_pane(&mut self, i: usize) {
        if i < self.panes.len() {
            let pane = self.panes.remove(i);
            if self.focus.is_some_and(|f| pane.tabs.contains(&f)) {
                self.focus = self.panes.last().map(Pane::panel);
            }
        }
    }

    pub fn close_all(&mut self) {
        self.panes.clear();
        self.focus = None;
    }

    /// Closes a panel that shows, shows an open panel that is a tab
    /// behind another, and opens a closed panel.
    pub fn toggle(&mut self, panel: Panel) {
        if self.shows(panel) {
            self.close(panel);
        } else {
            self.open(panel);
        }
    }

    /// Pins or unpins the pane with this index.
    pub fn pin(&mut self, i: usize, pinned: bool) {
        if let Some(pane) = self.panes.get_mut(i) {
            pane.pinned = pinned;
        }
    }

    /// Makes a tab the open tab of its pane.
    pub fn focus(&mut self, pane: usize, tab: usize) {
        if let Some(p) = self.panes.get_mut(pane)
            && tab < p.tabs.len()
        {
            p.active = tab;
            self.focus = Some(p.tabs[tab]);
        }
    }

    /// Moves a pane to the place before the pane with index `to`, as the
    /// column was before the move. `to` can be the number of panes. The
    /// pane becomes pinned.
    pub fn move_pane(&mut self, from: usize, to: usize) {
        if from >= self.panes.len() {
            return;
        }
        let mut pane = self.panes.remove(from);
        pane.pinned = true;
        let to = if to > from { to - 1 } else { to };
        self.panes.insert(to.min(self.panes.len()), pane);
    }

    /// Moves a tab out of its pane, to another pane or to a new pane. The
    /// indices of `drop` are of the column before the move. The panes that
    /// change become pinned.
    pub fn move_tab(&mut self, panel: Panel, drop: Drop) {
        let Some((from, tab)) = self.find(panel) else {
            return;
        };
        match drop {
            Drop::Merge { pane, .. } if pane == from || pane >= self.panes.len() => {}
            Drop::Split { at } if self.panes[from].tabs.len() == 1 => {
                // A pane with one tab moves as a whole.
                self.move_pane(from, at);
            }
            Drop::Merge { pane, index } => {
                let gone = self.remove_tab(from, tab);
                let pane = if gone && pane > from { pane - 1 } else { pane };
                let target = &mut self.panes[pane];
                let index = index.min(target.tabs.len());
                target.tabs.insert(index, panel);
                target.active = index;
                target.pinned = true;
                if let Some(source) = self.panes.get_mut(from).filter(|_| !gone) {
                    source.pinned = true;
                }
            }
            Drop::Split { at } => {
                self.remove_tab(from, tab);
                self.panes[from].pinned = true;
                let at = at.min(self.panes.len());
                self.panes.insert(at, Pane::new(panel, true));
            }
        }
        self.focus = Some(panel);
    }

    /// Removes a tab. Returns true if its pane had no other tab, and so
    /// closed.
    fn remove_tab(&mut self, i: usize, tab: usize) -> bool {
        let pane = &mut self.panes[i];
        pane.tabs.remove(tab);
        if pane.tabs.is_empty() {
            self.panes.remove(i);
            return true;
        }
        if pane.active > tab || pane.active >= pane.tabs.len() {
            pane.active = pane.active.saturating_sub(1);
        }
        false
    }

    /// Moves `delta` points of height from the pane below the bar with
    /// index `bar` to the pane above it. `heights` are the heights of the
    /// panes as they show. The two panes become pinned.
    pub fn resize(&mut self, bar: usize, delta: f32, heights: &[f32]) {
        let (Some(&above), Some(&below)) = (heights.get(bar), heights.get(bar + 1)) else {
            return;
        };
        // A pane that is shorter than `PANE_MIN` does not get shorter.
        let (low, high) = (PANE_MIN.min(above), above + below - PANE_MIN.min(below));
        let new = (above + delta).clamp(low, high.max(low));
        for (i, height) in [(bar, new), (bar + 1, above + below - new)] {
            if let Some(pane) = self.panes.get_mut(i) {
                pane.height = Some(height);
                pane.pinned = true;
            }
        }
    }

    /// Closes the unpinned pane, for a tap on the canvas. Returns true if
    /// a pane closed.
    pub fn tap_canvas(&mut self) -> bool {
        match self.panes.iter().position(|p| !p.pinned) {
            Some(i) => {
                self.close_pane(i);
                true
            }
            None => false,
        }
    }

    /// Closes the pinned panes and puts the width back.
    pub fn reset(&mut self) {
        self.panes.retain(|p| !p.pinned);
        for pane in &mut self.panes {
            pane.height = None;
        }
        self.width = WIDTH;
        if self.focus.is_some_and(|f| !self.is_open(f)) {
            self.focus = self.panes.last().map(Pane::panel);
        }
    }

    /// Joins the panes that do not fit in `room` to the last pane that
    /// fits, as tabs.
    pub fn keep_in(&mut self, room: f32) {
        self.room = room;
        let most = self.most();
        while self.panes.len() > most {
            let extra = self.panes.pop().unwrap_or_else(|| unreachable!());
            let last = self.panes.last_mut().unwrap_or_else(|| unreachable!());
            last.tabs.extend(extra.tabs);
            last.pinned |= extra.pinned;
        }
    }

    pub fn set_width(&mut self, width: f32) {
        self.width = width.clamp(MIN_WIDTH, MAX_WIDTH);
    }
}

/// The heights of the panes in `room`. `wanted` is the height that each
/// pane wants: the height of its content or the height that the user gave
/// it. A pane that fits keeps its height, and the panes that do not fit
/// share the rest of the room. A pane is never shorter than `PANE_MIN`,
/// or than the height that it wants.
pub fn fit(wanted: &[f32], room: f32) -> Vec<f32> {
    let n = wanted.len();
    if n == 0 {
        return Vec::new();
    }
    let space = room - BAR * (n - 1) as f32;
    if wanted.iter().sum::<f32>() <= space {
        return wanted.to_vec();
    }
    let mut heights: Vec<Option<f32>> = vec![None; n];
    let mut rest = space;
    loop {
        let open = heights.iter().filter(|h| h.is_none()).count();
        if open == 0 {
            break;
        }
        let share = rest / open as f32;
        let mut changed = false;
        for (i, w) in wanted.iter().enumerate() {
            if heights[i].is_none() && *w <= share {
                heights[i] = Some(*w);
                rest -= w;
                changed = true;
            }
        }
        if !changed {
            for (i, w) in wanted.iter().enumerate() {
                if heights[i].is_none() {
                    heights[i] = Some(share.max(PANE_MIN.min(*w)));
                }
            }
            break;
        }
    }
    heights.into_iter().map(Option::unwrap_or_default).collect()
}

/// The docks of the two workspaces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Docks {
    pub world: Dock,
    pub atlas: Dock,
}

impl Docks {
    /// The text of the file: the width and the pinned panes of each dock.
    pub fn text(&self) -> String {
        let mut lines = Vec::new();
        for (key, dock) in [("world", &self.world), ("atlas", &self.atlas)] {
            lines.push(format!("{key} width {}", dock.width));
            for pane in dock.panes.iter().filter(|p| p.pinned) {
                let tabs: Vec<&str> = pane.tabs.iter().map(|t| name(*t)).collect();
                let height = pane.height.map_or("auto".to_owned(), |h| h.to_string());
                lines.push(format!(
                    "{key} pane {} {} {height}",
                    tabs.join(","),
                    pane.active
                ));
            }
        }
        lines.join("\n")
    }

    /// Puts back the docks of the text of a file. A bad line has no effect.
    pub fn load(&mut self, text: &str) {
        for line in text.lines() {
            let words: Vec<&str> = line.split(' ').collect();
            let dock = match words.first() {
                Some(&"world") => &mut self.world,
                Some(&"atlas") => &mut self.atlas,
                _ => continue,
            };
            match words[1..] {
                ["width", width] => {
                    if let Some(w) = width.parse::<f32>().ok().filter(|w| w.is_finite()) {
                        dock.set_width(w);
                    }
                }
                ["pane", tabs, active, height] => {
                    if let Some(pane) = parse_pane(tabs, active, height)
                        && pane.tabs.iter().all(|t| !dock.is_open(*t))
                    {
                        dock.panes.push(pane);
                    }
                }
                _ => {}
            }
        }
    }
}

fn name(panel: Panel) -> &'static str {
    NAMES
        .iter()
        .find_map(|(p, name)| (*p == panel).then_some(*name))
        .unwrap_or_default()
}

fn parse_pane(tabs: &str, active: &str, height: &str) -> Option<Pane> {
    let mut panels = Vec::new();
    for tab in tabs.split(',') {
        let panel = NAMES.iter().find_map(|(p, n)| (*n == tab).then_some(*p))?;
        if panels.contains(&panel) {
            return None;
        }
        panels.push(panel);
    }
    let active = active.parse::<usize>().ok().filter(|a| *a < panels.len())?;
    let height = match height {
        "auto" => None,
        h => Some(
            h.parse::<f32>()
                .ok()
                .filter(|h| h.is_finite() && *h > 0.0)?,
        ),
    };
    Some(Pane {
        tabs: panels,
        active,
        pinned: true,
        height,
    })
}

/// The text of the file in the state folder `dir`. `None`: the folder has
/// no file.
pub fn read(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(FILE)).ok()
}

/// Writes the file to the state folder `dir`. A stop of the app during the
/// write leaves the old file.
pub fn write(dir: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{FILE}.partial"));
    std::fs::write(&partial, text)?;
    std::fs::rename(partial, dir.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dock(panes: &[(&[Panel], bool)]) -> Dock {
        let mut dock = Dock::default();
        for (tabs, pinned) in panes {
            dock.panes.push(Pane {
                tabs: tabs.to_vec(),
                active: 0,
                pinned: *pinned,
                height: None,
            });
        }
        dock
    }

    fn tabs(dock: &Dock) -> Vec<Vec<Panel>> {
        dock.panes.iter().map(|p| p.tabs.clone()).collect()
    }

    #[test]
    fn fit_keeps_the_panes_that_fit_and_shrinks_the_rest() {
        assert_eq!(fit(&[200.0, 300.0], 600.0), vec![200.0, 300.0]);
        let heights = fit(&[200.0, 900.0, 900.0], 800.0);
        assert_eq!(heights[0], 200.0);
        assert_eq!(heights[1], heights[2]);
        let total: f32 = heights.iter().sum::<f32>() + 2.0 * BAR;
        assert!((total - 800.0).abs() < 0.01, "{heights:?}");
    }

    #[test]
    fn a_pane_is_never_below_its_header_and_two_rows() {
        let heights = fit(&[600.0, 600.0, 600.0, 600.0], 400.0);
        assert!(heights.iter().all(|h| *h >= PANE_MIN), "{heights:?}");
        // A pane with less content keeps its height.
        assert_eq!(fit(&[80.0, 600.0, 600.0], 300.0)[0], 80.0);
    }

    #[test]
    fn a_move_or_a_resize_pins_a_pane() {
        let mut d = dock(&[(&[Panel::Layers], false), (&[Panel::Brush], true)]);
        d.move_pane(0, 2);
        assert_eq!(tabs(&d), vec![vec![Panel::Brush], vec![Panel::Layers]]);
        assert!(d.panes[1].pinned);

        let mut d = dock(&[(&[Panel::Layers], true), (&[Panel::Brush], false)]);
        d.resize(0, 50.0, &[300.0, 300.0]);
        assert_eq!(d.panes[0].height, Some(350.0));
        assert_eq!(d.panes[1].height, Some(250.0));
        assert!(d.panes[1].pinned);
        d.resize(0, 1000.0, &[350.0, 250.0]);
        assert_eq!(d.panes[1].height, Some(PANE_MIN));
    }

    #[test]
    fn a_new_panel_replaces_the_one_unpinned_pane() {
        let mut d = dock(&[(&[Panel::Layers], true), (&[Panel::Toolbox], false)]);
        d.open(Panel::Attributes);
        assert_eq!(tabs(&d), vec![vec![Panel::Layers], vec![Panel::Attributes]]);
        d.pin(1, true);
        d.open(Panel::Brush);
        assert_eq!(d.panes.len(), 3);
        assert_eq!(d.panes.iter().filter(|p| !p.pinned).count(), 1);
        assert!(d.tap_canvas());
        assert_eq!(d.panes.len(), 2);
        assert!(!d.tap_canvas());
    }

    #[test]
    fn with_no_room_a_new_panel_joins_the_last_pane() {
        let mut d = dock(&[(&[Panel::Layers], true), (&[Panel::Toolbox], true)]);
        d.room = 2.0 * PANE_MIN + BAR;
        d.open(Panel::Brush);
        assert_eq!(
            tabs(&d),
            vec![vec![Panel::Layers], vec![Panel::Toolbox, Panel::Brush]]
        );
        assert!(d.shows(Panel::Brush));
    }

    #[test]
    fn a_tab_merges_into_a_pane_and_splits_to_a_new_pane() {
        let mut d = dock(&[
            (&[Panel::Layers, Panel::Toolbox], false),
            (&[Panel::Attributes], false),
        ]);
        d.move_tab(Panel::Attributes, Drop::Merge { pane: 0, index: 1 });
        assert_eq!(
            tabs(&d),
            vec![vec![Panel::Layers, Panel::Attributes, Panel::Toolbox]]
        );
        assert!(d.panes[0].pinned && d.shows(Panel::Attributes));

        d.move_tab(Panel::Layers, Drop::Split { at: 1 });
        assert_eq!(
            tabs(&d),
            vec![vec![Panel::Attributes, Panel::Toolbox], vec![Panel::Layers]]
        );
        d.move_tab(Panel::Toolbox, Drop::Split { at: 0 });
        assert_eq!(
            tabs(&d),
            vec![
                vec![Panel::Toolbox],
                vec![Panel::Attributes],
                vec![Panel::Layers]
            ]
        );
        // A tab merges into a pane below its own pane, whose pane closes.
        d.move_tab(Panel::Toolbox, Drop::Merge { pane: 2, index: 0 });
        assert_eq!(
            tabs(&d),
            vec![vec![Panel::Attributes], vec![Panel::Toolbox, Panel::Layers]]
        );
    }

    #[test]
    fn close_toggle_and_reset() {
        let mut d = dock(&[(&[Panel::Layers, Panel::Toolbox], true)]);
        d.toggle(Panel::Toolbox);
        assert!(d.shows(Panel::Toolbox));
        d.toggle(Panel::Toolbox);
        assert_eq!(tabs(&d), vec![vec![Panel::Layers]]);
        d.open(Panel::Brush);
        d.width = 480.0;
        d.reset();
        assert_eq!(tabs(&d), vec![vec![Panel::Brush]]);
        assert_eq!(d.width, WIDTH);
    }

    #[test]
    fn the_text_keeps_the_pinned_panes_of_each_workspace() {
        let mut docks = Docks::default();
        docks.world.open_tabs(&[Panel::Layers, Panel::Toolbox]);
        docks.world.panes[0].active = 1;
        docks.world.panes[0].height = Some(320.0);
        docks.world.open_pinned(Panel::Brush);
        docks.world.open(Panel::Attributes);
        docks.world.width = 460.0;
        docks.atlas.open_pinned(Panel::Attributes);
        let mut back = Docks::default();
        back.load(&docks.text());
        assert_eq!(back.world.width, 460.0);
        assert_eq!(
            tabs(&back.world),
            vec![vec![Panel::Layers, Panel::Toolbox], vec![Panel::Brush]]
        );
        assert_eq!(back.world.panes[0].active, 1);
        assert_eq!(back.world.panes[0].height, Some(320.0));
        assert_eq!(tabs(&back.atlas), vec![vec![Panel::Attributes]]);
        assert_eq!(back.atlas.width, WIDTH);
    }

    #[test]
    fn a_bad_line_has_no_effect() {
        let mut docks = Docks::default();
        docks.load(
            "world width wide\nworld pane maps 0 auto\nworld pane layers 3 auto\n\
             world pane layers,layers 0 auto\nmoon width 400\nworld pane layers 0 -5\n\
             world width 9000\nworld pane toolbox 0 auto",
        );
        assert_eq!(tabs(&docks.world), vec![vec![Panel::Toolbox]]);
        assert_eq!(docks.world.width, MAX_WIDTH);
    }
}
