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
const NAMES: [(Panel, &str); 10] = [
    (Panel::Brush, "brush"),
    (Panel::Layers, "layers"),
    (Panel::Toolbox, "toolbox"),
    (Panel::Attributes, "attributes"),
    (Panel::Frame, "frame"),
    (Panel::Projection, "projection"),
    (Panel::Labels, "labels"),
    (Panel::Pages, "pages"),
    (Panel::Maps, "maps"),
    (Panel::Item, "item"),
];

/// The panels of a map frame in the Atlas workspace.
pub const FRAME_PANELS: [Panel; 4] = [
    Panel::Frame,
    Panel::Projection,
    Panel::Layers,
    Panel::Labels,
];

/// A group of the panels of a frame that open as the tabs of one pane.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub tabs: Vec<Panel>,
    /// The index of the open tab.
    pub active: usize,
}

/// The first arrangement of the panels of a frame: one pane with four
/// tabs.
pub fn first_arrangement() -> Vec<Group> {
    vec![Group {
        tabs: FRAME_PANELS.to_vec(),
        active: 0,
    }]
}

/// A pane of the dock.
#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub tabs: Vec<Panel>,
    /// The index of the open tab.
    pub active: usize,
    /// The height that the user gave the pane. `None`: the height of its
    /// content.
    pub height: Option<f32>,
    /// The panes that joined this pane as tabs because the dock had no room
    /// for them. They come back when the room is there.
    pub joined: Vec<Pane>,
}

impl Pane {
    pub fn new(panel: Panel) -> Pane {
        Pane {
            tabs: vec![panel],
            active: 0,
            height: None,
            joined: Vec::new(),
        }
    }

    /// A pane with these tabs.
    pub fn with_tabs(tabs: Vec<Panel>, active: usize) -> Pane {
        Pane {
            tabs,
            active,
            height: None,
            joined: Vec::new(),
        }
    }

    /// The pane as the user arranged it, and the panes that joined it for
    /// lack of room. A tab that left the pane leaves its joined pane too.
    fn split(&self) -> Vec<Pane> {
        let joined: Vec<Panel> = self.joined.iter().flat_map(|p| p.tabs.clone()).collect();
        let own: Vec<Panel> = (self.tabs.iter())
            .filter(|t| !joined.contains(t))
            .copied()
            .collect();
        let mut out = Vec::new();
        let shown = self.panel();
        for (tabs, pane) in std::iter::once((own, self)).chain(self.joined.iter().map(|j| {
            let tabs = (j.tabs.iter())
                .filter(|t| self.tabs.contains(t))
                .copied()
                .collect();
            (tabs, j)
        })) {
            if tabs.is_empty() {
                continue;
            }
            let active = match tabs.iter().position(|t| *t == shown) {
                Some(i) => i,
                None => (pane.tabs.get(pane.active))
                    .and_then(|a| tabs.iter().position(|t| t == a))
                    .unwrap_or(0),
            };
            out.push(Pane {
                tabs,
                active,
                height: pane.height,
                joined: Vec::new(),
            });
        }
        out
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
    /// The groups of the panels of a frame that a selected frame opens, in
    /// the order of the dock. The Atlas workspace uses it.
    pub arrangement: Vec<Group>,
    /// The dock is off the right edge, with its panes as they were.
    pub collapsed: bool,
}

impl Default for Dock {
    fn default() -> Dock {
        Dock {
            width: WIDTH,
            panes: Vec::new(),
            focus: None,
            room: f32::INFINITY,
            arrangement: first_arrangement(),
            collapsed: false,
        }
    }
}

impl Dock {
    /// The dock of the World workspace, which has no panels of a frame.
    pub fn world() -> Dock {
        Dock {
            arrangement: Vec::new(),
            ..Dock::default()
        }
    }

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

    /// The most panes that fit in the room.
    fn most(&self) -> usize {
        ((self.room + BAR) / (PANE_MIN + BAR)).floor().max(1.0) as usize
    }

    /// Shows a panel. An open panel becomes the open tab of its pane. A
    /// closed panel of a frame joins the pane of the other panels of its
    /// group. Another closed panel opens in a new pane at the bottom, or,
    /// with no room for a new pane, as a tab of the last pane. A panel that
    /// opens shows the dock.
    pub fn open(&mut self, panel: Panel) {
        self.focus = Some(panel);
        if let Some((i, tab)) = self.find(panel) {
            self.panes[i].active = tab;
            return;
        }
        self.collapsed = false;
        if let Some(group) = self.arrangement.iter().find(|g| g.tabs.contains(&panel))
            && let Some((i, _)) = (group.tabs.iter())
                .filter(|p| **p != panel)
                .find_map(|p| self.find(*p))
        {
            // The tab goes after the open tabs that are before it in the
            // group.
            let before: Vec<Panel> = (group.tabs.iter())
                .take_while(|p| **p != panel)
                .copied()
                .collect();
            let pane = &mut self.panes[i];
            let at = (pane.tabs.iter())
                .rposition(|t| before.contains(t))
                .map_or(0, |p| p + 1);
            pane.tabs.insert(at, panel);
            pane.active = at;
            return;
        }
        self.panes.push(Pane::new(panel));
        self.keep_in(self.room);
    }

    /// Opens the groups of the panels of a frame that are not open, as
    /// panes at the bottom, in the order of the arrangement. Each group
    /// shows its open tab of the last time.
    pub fn open_frames(&mut self) {
        let mut first = None;
        for group in self.arrangement.clone() {
            let missing: Vec<Panel> = (group.tabs.iter())
                .filter(|p| !self.is_open(**p))
                .copied()
                .collect();
            if missing.is_empty() {
                continue;
            }
            let shown = group.tabs.get(group.active);
            let active = (missing.iter()).position(|p| Some(p) == shown).unwrap_or(0);
            first.get_or_insert(missing[active]);
            self.panes.push(Pane::with_tabs(missing, active));
        }
        if first.is_some() {
            self.focus = first;
            self.collapsed = false;
            self.keep_in(self.room);
        }
    }

    /// Takes the arrangement of the panels of a frame from the panes. A
    /// closed panel stays in the group of the panels that it was with.
    pub fn learn(&mut self) {
        let mut groups: Vec<Group> = Vec::new();
        for pane in self.user_panes() {
            let tabs: Vec<Panel> = (pane.tabs.iter())
                .filter(|t| FRAME_PANELS.contains(t))
                .copied()
                .collect();
            if tabs.is_empty() {
                continue;
            }
            let active = tabs.iter().position(|t| *t == pane.panel()).unwrap_or(0);
            groups.push(Group { tabs, active });
        }
        if groups.is_empty() {
            return;
        }
        for old in &self.arrangement {
            let has = |groups: &[Group], p: &Panel| groups.iter().any(|g| g.tabs.contains(p));
            let closed: Vec<Panel> = (old.tabs.iter())
                .filter(|p| !has(&groups, p))
                .copied()
                .collect();
            if closed.is_empty() {
                continue;
            }
            let mate =
                (old.tabs.iter()).find_map(|m| groups.iter().position(|g| g.tabs.contains(m)));
            match mate {
                Some(i) => groups[i].tabs.extend(closed),
                None => groups.push(Group {
                    tabs: closed,
                    active: 0,
                }),
            }
        }
        self.arrangement = groups;
    }

    /// The panes as the user arranged them, with the panes that joined
    /// another pane for lack of room on their own.
    pub fn user_panes(&self) -> Vec<Pane> {
        self.panes.iter().flat_map(Pane::split).collect()
    }

    /// Opens a panel in a new pane at the bottom.
    pub fn open_new(&mut self, panel: Panel) {
        self.close(panel);
        self.panes.push(Pane::new(panel));
        self.focus = Some(panel);
        self.collapsed = false;
    }

    /// Opens the panels as the tabs of one new pane.
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
        self.collapsed = false;
        self.panes.push(Pane::with_tabs(tabs, 0));
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
    /// column was before the move. `to` can be the number of panes.
    pub fn move_pane(&mut self, from: usize, to: usize) {
        if from >= self.panes.len() {
            return;
        }
        let pane = self.panes.remove(from);
        let to = if to > from { to - 1 } else { to };
        self.panes.insert(to.min(self.panes.len()), pane);
    }

    /// Moves a tab out of its pane, to another pane or to a new pane. The
    /// indices of `drop` are of the column before the move.
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
            }
            Drop::Split { at } => {
                self.remove_tab(from, tab);
                let at = at.min(self.panes.len());
                self.panes.insert(at, Pane::new(panel));
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
    /// panes as they show.
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
            }
        }
    }

    /// Moves a tab of a pane with more than one tab to a new pane below
    /// its pane.
    pub fn split_tab(&mut self, panel: Panel) {
        if let Some((i, _)) = self.find(panel)
            && self.panes[i].tabs.len() > 1
        {
            self.move_tab(panel, Drop::Split { at: i + 1 });
        }
    }

    /// Puts back the first state: no panes, the first width, the first
    /// arrangement, and the dock on the screen.
    pub fn reset(&mut self) {
        self.close_all();
        self.width = WIDTH;
        self.collapsed = false;
        if !self.arrangement.is_empty() {
            self.arrangement = first_arrangement();
        }
    }

    /// Fits the panes in `room`. The panes that do not fit show as tabs of
    /// the last pane that fits. They come back as panes of their own when
    /// the room is there.
    pub fn keep_in(&mut self, room: f32) {
        self.room = room;
        let most = self.most();
        // The joined panes come back, and then the panes that do not fit
        // join again.
        if self.panes.iter().any(|p| !p.joined.is_empty()) {
            self.panes = self.user_panes();
        }
        while self.panes.len() > most {
            let extra = self.panes.pop().unwrap_or_else(|| unreachable!());
            let focus = self.focus;
            let last = self.panes.last_mut().unwrap_or_else(|| unreachable!());
            let shows = focus.and_then(|f| extra.tabs.iter().position(|t| *t == f));
            let start = last.tabs.len();
            last.tabs.extend(extra.tabs.iter().copied());
            if let Some(i) = shows {
                last.active = start + i;
            }
            last.joined.push(extra);
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
#[derive(Clone, Debug, PartialEq)]
pub struct Docks {
    pub world: Dock,
    pub atlas: Dock,
}

impl Default for Docks {
    fn default() -> Docks {
        Docks {
            world: Dock::world(),
            atlas: Dock::default(),
        }
    }
}

impl Docks {
    /// The text of the file: the width, the panes, and the collapsed state
    /// of each dock, and the arrangement of the panels of a frame in the
    /// Atlas.
    pub fn text(&self) -> String {
        let mut lines = Vec::new();
        for (key, dock) in [("world", &self.world), ("atlas", &self.atlas)] {
            lines.push(format!("{key} width {}", dock.width));
            if dock.collapsed {
                lines.push(format!("{key} collapsed"));
            }
            for pane in dock.user_panes() {
                let active = pane.active;
                let names: Vec<&str> = pane.tabs.iter().map(|t| name(*t)).collect();
                let height = pane.height.map_or("auto".to_owned(), |h| h.to_string());
                lines.push(format!("{key} pane {} {active} {height}", names.join(",")));
            }
        }
        for group in &self.atlas.arrangement {
            let names: Vec<&str> = group.tabs.iter().map(|t| name(*t)).collect();
            lines.push(format!("atlas group {} {}", names.join(","), group.active));
        }
        lines.join("\n")
    }

    /// Puts back the docks of the text of a file. A bad line has no effect.
    pub fn load(&mut self, text: &str) {
        let mut groups = Vec::new();
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
                ["collapsed"] => dock.collapsed = true,
                // The fifth field of an older file has no effect.
                ["pane", tabs, active, height] | ["pane", tabs, active, height, _] => {
                    if let Some(pane) = parse_pane(tabs, active, height)
                        && pane.tabs.iter().all(|t| !dock.is_open(*t))
                    {
                        dock.panes.push(pane);
                    }
                }
                ["group", tabs, active] if words[0] == "atlas" => {
                    groups.push(parse_pane(tabs, active, "auto"));
                }
                _ => {}
            }
        }
        // The arrangement holds each panel of a frame one time, or the
        // first arrangement stays.
        let groups: Option<Vec<Group>> = groups
            .into_iter()
            .map(|g| {
                g.map(|p| Group {
                    tabs: p.tabs,
                    active: p.active,
                })
            })
            .collect();
        if let Some(groups) = groups {
            let mut all: Vec<Panel> = groups.iter().flat_map(|g| g.tabs.clone()).collect();
            let count = all.len();
            all.sort_by_key(|p| FRAME_PANELS.iter().position(|f| f == p));
            all.dedup();
            if count == FRAME_PANELS.len() && all == FRAME_PANELS {
                self.atlas.arrangement = groups;
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
    let mut pane = Pane::with_tabs(panels, active);
    pane.height = height;
    Some(pane)
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

    fn dock(panes: &[&[Panel]]) -> Dock {
        let mut dock = Dock::world();
        for tabs in panes {
            dock.panes.push(Pane::with_tabs(tabs.to_vec(), 0));
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
    fn a_move_and_a_resize() {
        let mut d = dock(&[&[Panel::Layers], &[Panel::Brush]]);
        d.move_pane(0, 2);
        assert_eq!(tabs(&d), vec![vec![Panel::Brush], vec![Panel::Layers]]);

        d.resize(0, 50.0, &[300.0, 300.0]);
        assert_eq!(d.panes[0].height, Some(350.0));
        assert_eq!(d.panes[1].height, Some(250.0));
        d.resize(0, 1000.0, &[350.0, 250.0]);
        assert_eq!(d.panes[1].height, Some(PANE_MIN));
    }

    #[test]
    fn each_panel_opens_in_a_new_pane_and_stays() {
        let mut d = dock(&[&[Panel::Layers], &[Panel::Toolbox]]);
        d.open(Panel::Attributes);
        d.open(Panel::Brush);
        assert_eq!(
            tabs(&d),
            vec![
                vec![Panel::Layers],
                vec![Panel::Toolbox],
                vec![Panel::Attributes],
                vec![Panel::Brush]
            ]
        );
        // An open panel only shows.
        d.open(Panel::Layers);
        assert_eq!(d.panes.len(), 4);
    }

    #[test]
    fn with_no_room_a_new_panel_joins_the_last_pane() {
        let mut d = dock(&[&[Panel::Layers], &[Panel::Toolbox]]);
        d.room = 2.0 * PANE_MIN + BAR;
        d.open(Panel::Brush);
        assert_eq!(
            tabs(&d),
            vec![vec![Panel::Layers], vec![Panel::Toolbox, Panel::Brush]]
        );
        assert!(d.shows(Panel::Brush));
    }

    #[test]
    fn a_tab_moves_to_a_new_pane_below_its_pane() {
        let mut d = dock(&[&[Panel::Layers, Panel::Toolbox], &[Panel::Attributes]]);
        d.split_tab(Panel::Toolbox);
        assert_eq!(
            tabs(&d),
            vec![
                vec![Panel::Layers],
                vec![Panel::Toolbox],
                vec![Panel::Attributes]
            ]
        );
        // A pane with one tab stays.
        d.split_tab(Panel::Toolbox);
        assert_eq!(d.panes.len(), 3);
    }

    #[test]
    fn a_tab_merges_into_a_pane_and_splits_to_a_new_pane() {
        let mut d = dock(&[&[Panel::Layers, Panel::Toolbox], &[Panel::Attributes]]);
        d.move_tab(Panel::Attributes, Drop::Merge { pane: 0, index: 1 });
        assert_eq!(
            tabs(&d),
            vec![vec![Panel::Layers, Panel::Attributes, Panel::Toolbox]]
        );
        assert!(d.shows(Panel::Attributes));

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
        let mut d = dock(&[&[Panel::Layers, Panel::Toolbox]]);
        d.toggle(Panel::Toolbox);
        assert!(d.shows(Panel::Toolbox));
        d.toggle(Panel::Toolbox);
        assert_eq!(tabs(&d), vec![vec![Panel::Layers]]);
        d.open(Panel::Brush);
        d.width = 480.0;
        d.collapsed = true;
        d.reset();
        assert!(d.panes.is_empty());
        assert_eq!(d.width, WIDTH);
        assert!(!d.collapsed);
    }

    #[test]
    fn a_panel_that_opens_shows_a_collapsed_dock() {
        let mut d = dock(&[&[Panel::Layers]]);
        d.collapsed = true;
        // An open panel keeps the dock collapsed.
        d.open(Panel::Layers);
        assert!(d.collapsed);
        d.open(Panel::Toolbox);
        assert!(!d.collapsed);
        let mut d = Dock {
            collapsed: true,
            ..Dock::default()
        };
        d.open_frames();
        assert!(!d.collapsed);
    }

    #[test]
    fn the_text_keeps_the_panes_of_each_workspace() {
        let mut docks = Docks::default();
        docks.world.open_tabs(&[Panel::Layers, Panel::Toolbox]);
        docks.world.panes[0].active = 1;
        docks.world.panes[0].height = Some(320.0);
        docks.world.open_new(Panel::Brush);
        docks.world.open(Panel::Attributes);
        docks.world.width = 460.0;
        docks.world.collapsed = true;
        docks.atlas.open(Panel::Item);
        let mut back = Docks::default();
        back.load(&docks.text());
        assert_eq!(back.world.width, 460.0);
        assert_eq!(
            tabs(&back.world),
            vec![
                vec![Panel::Layers, Panel::Toolbox],
                vec![Panel::Brush],
                vec![Panel::Attributes]
            ]
        );
        assert_eq!(back.world.panes[0].active, 1);
        assert_eq!(back.world.panes[0].height, Some(320.0));
        assert!(back.world.collapsed);
        assert_eq!(tabs(&back.atlas), vec![vec![Panel::Item]]);
        assert_eq!(back.atlas.width, WIDTH);
        assert!(!back.atlas.collapsed);
    }

    #[test]
    fn a_pane_of_an_older_file_loads() {
        let mut docks = Docks::default();
        docks.load("world pane layers,toolbox 1 auto pinned\nworld pane brush 0 200");
        assert_eq!(
            tabs(&docks.world),
            vec![vec![Panel::Layers, Panel::Toolbox], vec![Panel::Brush]]
        );
        assert_eq!(docks.world.panes[0].active, 1);
        assert_eq!(docks.world.panes[1].height, Some(200.0));
    }

    #[test]
    fn a_bad_line_has_no_effect() {
        let mut docks = Docks::default();
        docks.load(
            "world width wide\nworld pane moon 0 auto\nworld pane layers 3 auto\n\
             world pane layers,layers 0 auto\nmoon width 400\nworld pane layers 0 -5\n\
             world width 9000\nworld pane toolbox 0 auto",
        );
        assert_eq!(tabs(&docks.world), vec![vec![Panel::Toolbox]]);
        assert_eq!(docks.world.width, MAX_WIDTH);
    }
    #[test]
    fn panes_that_join_for_lack_of_room_come_back_with_room() {
        let mut d = dock(&[&[Panel::Layers], &[Panel::Toolbox], &[Panel::Brush]]);
        d.panes[1].height = Some(300.0);
        d.keep_in(2.0 * PANE_MIN + BAR);
        assert_eq!(
            tabs(&d),
            vec![vec![Panel::Layers], vec![Panel::Toolbox, Panel::Brush]]
        );
        // A taller screen splits them again, as the user arranged them.
        d.keep_in(1000.0);
        assert_eq!(
            tabs(&d),
            vec![
                vec![Panel::Layers],
                vec![Panel::Toolbox],
                vec![Panel::Brush]
            ]
        );
        assert_eq!(d.panes[1].height, Some(300.0));
        // A tab that the user moves out of a joined pane stays out.
        d.keep_in(2.0 * PANE_MIN + BAR);
        d.move_tab(Panel::Brush, Drop::Merge { pane: 0, index: 1 });
        d.keep_in(1000.0);
        assert_eq!(
            tabs(&d),
            vec![vec![Panel::Layers, Panel::Brush], vec![Panel::Toolbox]]
        );
    }

    #[test]
    fn a_frame_opens_its_panels_in_the_arrangement_of_the_last_time() {
        let mut d = Dock::default();
        d.open_frames();
        assert_eq!(tabs(&d), vec![FRAME_PANELS.to_vec()]);
        // Labels moves to a pane of its own, and Projection is the open tab.
        d.move_tab(Panel::Labels, Drop::Split { at: 1 });
        d.focus(0, 1);
        d.learn();
        assert_eq!(d.arrangement.len(), 2);
        d.close_all();
        d.learn();
        assert_eq!(d.arrangement.len(), 2);
        d.open_frames();
        assert_eq!(
            tabs(&d),
            vec![
                vec![Panel::Frame, Panel::Projection, Panel::Layers],
                vec![Panel::Labels]
            ]
        );
        assert!(d.shows(Panel::Projection));
        // A closed panel stays in its group, and opens with it.
        d.close(Panel::Layers);
        d.learn();
        assert_eq!(
            d.arrangement[0].tabs,
            vec![Panel::Frame, Panel::Projection, Panel::Layers]
        );
        d.open(Panel::Layers);
        assert_eq!(
            tabs(&d)[0],
            vec![Panel::Frame, Panel::Projection, Panel::Layers]
        );
        d.reset();
        assert_eq!(d.arrangement, first_arrangement());
    }

    #[test]
    fn a_frame_opens_only_the_panels_that_are_not_open() {
        let mut d = Dock::default();
        d.open_frames();
        d.open(Panel::Pages);
        d.close(Panel::Labels);
        d.open_frames();
        assert_eq!(
            tabs(&d),
            vec![
                vec![Panel::Frame, Panel::Projection, Panel::Layers],
                vec![Panel::Pages],
                vec![Panel::Labels]
            ]
        );
        d.open_frames();
        assert_eq!(d.panes.len(), 3);
    }

    #[test]
    fn the_text_keeps_the_arrangement_of_the_atlas() {
        let mut docks = Docks::default();
        docks.atlas.arrangement = vec![
            Group {
                tabs: vec![Panel::Labels, Panel::Frame],
                active: 1,
            },
            Group {
                tabs: vec![Panel::Projection, Panel::Layers],
                active: 0,
            },
        ];
        let mut back = Docks::default();
        back.load(&docks.text());
        assert_eq!(back.atlas.arrangement, docks.atlas.arrangement);
        // An arrangement without each panel of a frame has no effect.
        let mut back = Docks::default();
        back.load("atlas group frame,projection 0\natlas group layers 0");
        assert_eq!(back.atlas.arrangement, first_arrangement());
    }
}
