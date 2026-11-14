//! The settings of each tool, and the switch to the last tool. The app keeps
//! the settings in a text file in the folder of the state of the last run,
//! with one key and its values on each line. The project file does not hold
//! them.

use std::path::Path;

use vale_terrain::Mode;

use crate::globe::Tool;
use crate::globe::brush::{BrushSettings, FLOW, SIZE_POINTS, STRENGTH_M};
use crate::globe::combine::PolygonMode;
use crate::globe::line::LineSettings;
use crate::globe::snap::SnapSettings;
use crate::pen::PenSwitch;

/// The file in the state folder that holds the settings.
const TOOLS_FILE: &str = "tools.txt";

/// The brush modes, with their names in the file.
const MODES: [(Mode, &str); 16] = [
    (Mode::Raise, "raise"),
    (Mode::Lower, "lower"),
    (Mode::Smooth, "smooth"),
    (Mode::Flatten, "flatten"),
    (Mode::Carve, "carve"),
    (Mode::Ridge, "ridge"),
    (Mode::Deposit, "deposit"),
    (Mode::Erode, "erode"),
    (Mode::Weather, "weather"),
    (Mode::Uplift, "uplift"),
    (Mode::Warp, "warp"),
    (Mode::OceanRidge, "ocean_ridge"),
    (Mode::Range, "range"),
    (Mode::Push, "push"),
    (Mode::Noise, "noise"),
    (Mode::Basin, "basin"),
];

/// The tools, with their names in the file.
const TOOLS: [(Tool, &str); 9] = [
    (Tool::Pan, "navigate"),
    (Tool::Brush, "brush"),
    (Tool::Line, "line"),
    (Tool::Polygon, "polygon"),
    (Tool::Point, "point"),
    (Tool::Measure, "measure"),
    (Tool::Select, "select"),
    (Tool::Split, "split"),
    (Tool::Vertex, "vertex"),
];

/// The polygon modes, with their names in the file.
const POLYGON_MODES: [(PolygonMode, &str); 3] = [
    (PolygonMode::New, "new"),
    (PolygonMode::Add, "add"),
    (PolygonMode::Subtract, "subtract"),
];

type Field<T> = (&'static str, fn(&mut BrushSettings) -> &mut T);

/// The brush settings that one mode uses and all modes share.
const FLOATS: [Field<f64>; 23] = [
    ("noise_raise_m", |b| &mut b.noise_raise_m),
    ("noise_lower_m", |b| &mut b.noise_lower_m),
    ("talus_deg", |b| &mut b.talus_deg),
    ("erosion", |b| &mut b.erosion),
    ("warp_km", |b| &mut b.warp_km),
    ("scale_km", |b| &mut b.scale_km),
    ("noise_relief", |b| &mut b.noise_relief),
    ("noise_grain", |b| &mut b.noise_grain),
    ("ridge_crest_m", |b| &mut b.ridge_crest_m),
    ("ridge_segment_km", |b| &mut b.ridge_segment_km),
    ("ridge_offset_km", |b| &mut b.ridge_offset_km),
    ("ridge_spread_mm", |b| &mut b.ridge_spread_mm),
    ("ridge_magma", |b| &mut b.ridge_magma),
    ("range_spacing_km", |b| &mut b.range_spacing_km),
    ("basin_density_km", |b| &mut b.basin_density_km),
    ("basin_age", |b| &mut b.basin_age),
    ("basin_lakes", |b| &mut b.basin_lakes),
    ("channel_km", |b| &mut b.channel_km),
    ("floodplain_km", |b| &mut b.floodplain_km),
    ("floor_m", |b| &mut b.floor_m),
    ("ceiling_m", |b| &mut b.ceiling_m),
    ("shape", |b| &mut b.shape),
    ("terrace_step_m", |b| &mut b.terrace_step_m),
];

const BOOLS: [Field<bool>; 4] = [
    ("noise_ridged", |b| &mut b.noise_ridged),
    ("noise_inverted", |b| &mut b.noise_inverted),
    ("floor_on", |b| &mut b.floor_on),
    ("ceiling_on", |b| &mut b.ceiling_on),
];

/// A tool of the tool strip. Each brush mode is a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Tool(Tool),
    Brush(Mode),
}

impl Choice {
    pub fn of(tool: Tool, mode: Mode) -> Choice {
        match tool {
            Tool::Brush => Choice::Brush(mode),
            tool => Choice::Tool(tool),
        }
    }
}

/// The settings that each brush mode keeps.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ModeSettings {
    size_points: f32,
    lock: Option<f64>,
    hardness: f64,
    flow: f64,
    strength_m: f64,
}

impl ModeSettings {
    fn of(brush: &BrushSettings) -> ModeSettings {
        ModeSettings {
            size_points: brush.size_points,
            lock: brush.lock,
            hardness: brush.hardness,
            flow: brush.flow,
            strength_m: brush.strength_m,
        }
    }

    fn apply(self, brush: &mut BrushSettings) {
        brush.size_points = self.size_points;
        brush.lock = self.lock;
        brush.hardness = self.hardness;
        brush.flow = self.flow;
        brush.strength_m = self.strength_m;
    }
}

/// The index of the smoothing of the line tool or of the polygon tool.
fn smoothing_index(tool: Tool) -> usize {
    usize::from(tool == Tool::Polygon)
}

/// The settings of each tool, and the tool before the current tool. The
/// brush and the line settings of the globe hold the settings of the
/// current tool, and this holds the settings of the other tools.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tools {
    /// The settings of each brush mode that the brush had.
    modes: Vec<(Mode, ModeSettings)>,
    /// The mode whose settings the brush holds. `None`: no frame saw the
    /// brush.
    held: Option<Mode>,
    /// The smoothing of the line tool and of the polygon tool. `None`: the
    /// tool has the smoothing of the other tool.
    smoothing: [Option<f64>; 2],
    /// The tool whose smoothing the line settings hold, the line tool or
    /// the polygon tool.
    smoothing_of: Option<Tool>,
    current: Option<Choice>,
    last: Option<Choice>,
    /// A new feature of the point tool, the line tool, or the polygon tool
    /// takes the focus in the inspector.
    pub focus_new: bool,
    /// One finger paints and draws. `None`: the default of the device.
    pub finger_paints: Option<bool>,
}

impl Tools {
    /// The tool before the current tool.
    pub fn last(&self) -> Option<Choice> {
        self.last
    }

    fn mode(&self, mode: Mode) -> Option<ModeSettings> {
        let found = self.modes.iter().find(|(m, _)| *m == mode);
        found.map(|&(_, settings)| settings)
    }

    fn keep(&mut self, mode: Mode, settings: ModeSettings) {
        match self.modes.iter_mut().find(|(m, _)| *m == mode) {
            Some(slot) => slot.1 = settings,
            None => self.modes.push((mode, settings)),
        }
    }

    /// Call this after each change of the tool. It keeps the settings of the
    /// tool that the change left, and it brings back the settings of the
    /// new tool. A brush mode with no settings gets the settings of a new
    /// brush. Returns true if a setting changed.
    pub fn observe(
        &mut self,
        tool: Tool,
        brush: &mut BrushSettings,
        line: &mut LineSettings,
    ) -> bool {
        let mut changed = false;
        match self.held {
            Some(held) if held != brush.mode => {
                self.keep(held, ModeSettings::of(brush));
                let new = self.mode(brush.mode);
                let new = new.unwrap_or_else(|| ModeSettings::of(&BrushSettings::default()));
                changed |= new != ModeSettings::of(brush);
                new.apply(brush);
            }
            _ => {}
        }
        self.held = Some(brush.mode);

        if matches!(tool, Tool::Line | Tool::Polygon) {
            match self.smoothing_of {
                Some(of) if of != tool => {
                    self.smoothing[smoothing_index(of)] = Some(line.smoothing);
                    if let Some(smoothing) = self.smoothing[smoothing_index(tool)] {
                        changed |= smoothing != line.smoothing;
                        line.smoothing = smoothing;
                    }
                }
                _ => {}
            }
            self.smoothing_of = Some(tool);
        }

        let choice = Choice::of(tool, brush.mode);
        if self.current != Some(choice) {
            if self.current.is_some() {
                self.last = self.current;
            }
            self.current = Some(choice);
        }
        changed
    }

    /// Switches the tool, for a double tap or a squeeze of the pen. The
    /// switch goes to the last tool. With `PenSwitch::Eraser`, it goes to
    /// the opposite mode where one exists: Raise and Lower switch with each
    /// other, and the Noise mode swaps its rise and its drop. A tool that
    /// `available` refuses does not change. Returns true if a setting
    /// changed.
    pub fn switch(
        &mut self,
        switch: PenSwitch,
        tool: &mut Tool,
        brush: &mut BrushSettings,
        line: &mut LineSettings,
        available: impl Fn(Choice) -> bool,
    ) -> bool {
        self.observe(*tool, brush, line);
        let target = match (switch, Choice::of(*tool, brush.mode)) {
            (PenSwitch::Eraser, Choice::Brush(Mode::Raise)) => Some(Choice::Brush(Mode::Lower)),
            (PenSwitch::Eraser, Choice::Brush(Mode::Lower)) => Some(Choice::Brush(Mode::Raise)),
            (PenSwitch::Eraser, Choice::Brush(Mode::Noise)) => {
                std::mem::swap(&mut brush.noise_raise_m, &mut brush.noise_lower_m);
                return true;
            }
            _ => self.last,
        };
        let Some(target) = target.filter(|&choice| available(choice)) else {
            return false;
        };
        match target {
            Choice::Tool(t) => *tool = t,
            Choice::Brush(mode) => {
                *tool = Tool::Brush;
                brush.mode = mode;
            }
        }
        self.observe(*tool, brush, line);
        true
    }

    /// The text of the file, with the settings of each tool.
    pub fn text(&self, brush: &BrushSettings, line: &LineSettings, snap: &SnapSettings) -> String {
        let mut tools = self.clone();
        tools.keep(brush.mode, ModeSettings::of(brush));
        if let Some(of) = tools.smoothing_of {
            tools.smoothing[smoothing_index(of)] = Some(line.smoothing);
        }
        let mut lines = Vec::new();
        let tool = match tools.current {
            Some(Choice::Brush(_)) => Some(Tool::Brush),
            Some(Choice::Tool(tool)) => Some(tool),
            None => None,
        };
        if let Some(tool) = tool.and_then(|tool| name(&TOOLS, tool)) {
            lines.push(format!("tool {tool}"));
        }
        if let Some(mode) = name(&MODES, brush.mode) {
            lines.push(format!("brush_mode {mode}"));
        }
        for (mode, name) in MODES {
            let Some(s) = tools.mode(mode) else {
                continue;
            };
            let lock = s.lock.map_or("none".to_string(), |lock| lock.to_string());
            lines.push(format!(
                "mode {name} {} {lock} {} {} {}",
                s.size_points, s.hardness, s.flow, s.strength_m
            ));
        }
        let mut brush = *brush;
        for (key, field) in FLOATS {
            lines.push(format!("{key} {}", field(&mut brush)));
        }
        for (key, field) in BOOLS {
            lines.push(format!("{key} {}", field(&mut brush)));
        }
        let smoothing = tools.smoothing.map(|s| s.unwrap_or(line.smoothing));
        lines.push(format!("smoothing {} {}", smoothing[0], smoothing[1]));
        lines.push(format!("terrain_fit {}", line.fit));
        lines.push(format!("terrain {}", line.terrain));
        lines.push(format!("reshape {}", line.reshape));
        let polygon_mode = POLYGON_MODES.iter().find(|(m, _)| *m == line.polygon_mode);
        lines.push(format!(
            "polygon_mode {}",
            polygon_mode.map_or("new", |m| m.1)
        ));
        lines.push(format!("snap {}", snap.on));
        lines.push(format!("snap_distance {}", snap.distance));
        lines.push(format!("focus_new_features {}", self.focus_new));
        if let Some(on) = self.finger_paints {
            lines.push(format!("finger_paints {on}"));
        }
        lines.join("\n")
    }

    /// Puts back the settings of the text of a file. A line that this
    /// version does not know, or a bad line, has no effect.
    pub fn load(
        &mut self,
        text: &str,
        tool: &mut Tool,
        brush: &mut BrushSettings,
        line: &mut LineSettings,
        snap: &mut SnapSettings,
    ) {
        let pairs = || text.lines().filter_map(|line| line.split_once(' '));
        let value = |key: &str| pairs().find_map(|(k, value)| (k == key).then_some(value));
        let float = |key: &str| value(key)?.parse::<f64>().ok().filter(|v| v.is_finite());
        let boolean = |key: &str| value(key)?.parse::<bool>().ok();

        let tool_name = value("tool").and_then(|name| TOOLS.iter().find(|(_, n)| *n == name));
        if let Some(&(t, _)) = tool_name {
            *tool = t;
        }
        let mode = value("brush_mode").and_then(|name| MODES.iter().find(|(_, n)| *n == name));
        if let Some(&(mode, _)) = mode {
            brush.mode = mode;
        }
        for (_, text) in pairs().filter(|(key, _)| *key == "mode") {
            if let Some((mode, settings)) = parse_mode(text) {
                self.keep(mode, settings);
            }
        }
        for (key, field) in FLOATS {
            if let Some(v) = float(key) {
                *field(brush) = v;
            }
        }
        for (key, field) in BOOLS {
            if let Some(v) = boolean(key) {
                *field(brush) = v;
            }
        }
        if let Some(settings) = self.mode(brush.mode) {
            settings.apply(brush);
        }
        self.held = Some(brush.mode);

        let smoothing = value("smoothing").map(|text| {
            let values = text.split(' ').map(|v| v.parse::<f64>().ok());
            values.map(|v| v.filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0)))
        });
        if let Some(mut values) = smoothing
            && let (Some(Some(l)), Some(Some(p)), None) =
                (values.next(), values.next(), values.next())
        {
            self.smoothing = [Some(l), Some(p)];
            let of = self.smoothing_of.unwrap_or(Tool::Line);
            line.smoothing = [l, p][smoothing_index(of)];
            self.smoothing_of = Some(of);
        }
        if let Some(v) = boolean("terrain_fit") {
            line.fit = v;
        }
        if let Some(v) = float("terrain") {
            line.terrain = v.clamp(0.0, 1.0);
        }
        if let Some(v) = boolean("reshape") {
            line.reshape = v;
        }
        let polygon_mode =
            value("polygon_mode").and_then(|name| POLYGON_MODES.iter().find(|(_, n)| *n == name));
        if let Some(&(mode, _)) = polygon_mode {
            line.polygon_mode = mode;
        }
        if let Some(v) = boolean("snap") {
            snap.on = v;
        }
        if let Some(v) = boolean("focus_new_features") {
            self.focus_new = v;
        }
        if let Some(v) = boolean("finger_paints") {
            self.finger_paints = Some(v);
        }
        if let Some(v) = float("snap_distance").filter(|&v| v > 0.0) {
            snap.distance = v;
        }
    }
}

/// The name in the file of `key`.
fn name<K: PartialEq>(list: &[(K, &'static str)], key: K) -> Option<&'static str> {
    list.iter().find(|(k, _)| *k == key).map(|e| e.1)
}

/// The values of a line of a brush mode: the name of the mode, the size,
/// the lock, the hardness, the flow, and the strength.
fn parse_mode(text: &str) -> Option<(Mode, ModeSettings)> {
    let parts: Vec<&str> = text.split(' ').collect();
    let [name, size, lock, hardness, flow, strength] = parts[..] else {
        return None;
    };
    let mode = MODES.iter().find(|(_, n)| *n == name)?.0;
    let float = |text: &str| text.parse::<f64>().ok().filter(|v| v.is_finite());
    let lock = match lock {
        "none" => None,
        lock => Some(float(lock).filter(|&v| v > 0.0)?),
    };
    let settings = ModeSettings {
        size_points: (float(size)? as f32).clamp(*SIZE_POINTS.start(), *SIZE_POINTS.end()),
        lock,
        hardness: float(hardness)?.clamp(0.0, 1.0),
        flow: float(flow)?.clamp(*FLOW.start(), *FLOW.end()),
        strength_m: float(strength)?.clamp(*STRENGTH_M.start(), *STRENGTH_M.end()),
    };
    Some((mode, settings))
}

/// The text of the file in the state folder `dir`. `None`: the folder
/// has no file.
pub fn read(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(TOOLS_FILE)).ok()
}

/// Writes the file to the state folder `dir`. A stop of the app during the
/// write leaves the old file.
pub fn write(dir: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let partial = dir.join(format!("{TOOLS_FILE}.partial"));
    std::fs::write(&partial, text)?;
    std::fs::rename(partial, dir.join(TOOLS_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Globe {
        tools: Tools,
        tool: Tool,
        brush: BrushSettings,
        line: LineSettings,
    }

    impl Globe {
        fn new() -> Globe {
            let mut globe = Globe {
                tools: Tools::default(),
                tool: Tool::Brush,
                brush: BrushSettings::default(),
                line: LineSettings::default(),
            };
            globe.observe();
            globe
        }

        fn observe(&mut self) {
            self.tools
                .observe(self.tool, &mut self.brush, &mut self.line);
        }

        fn choose(&mut self, choice: Choice) {
            match choice {
                Choice::Tool(tool) => self.tool = tool,
                Choice::Brush(mode) => {
                    self.tool = Tool::Brush;
                    self.brush.mode = mode;
                }
            }
            self.observe();
        }

        fn switch(&mut self, switch: PenSwitch) -> bool {
            let (brush, line) = (&mut self.brush, &mut self.line);
            self.tools
                .switch(switch, &mut self.tool, brush, line, |_| true)
        }

        fn choice(&self) -> Choice {
            Choice::of(self.tool, self.brush.mode)
        }
    }

    #[test]
    fn a_switch_goes_between_the_current_tool_and_the_last_tool() {
        let mut globe = Globe::new();
        // With no last tool, the switch does nothing.
        assert!(!globe.switch(PenSwitch::Last));
        assert_eq!(globe.choice(), Choice::Brush(Mode::Raise));

        globe.choose(Choice::Brush(Mode::Smooth));
        globe.switch(PenSwitch::Last);
        assert_eq!(globe.choice(), Choice::Brush(Mode::Raise));
        globe.switch(PenSwitch::Last);
        assert_eq!(globe.choice(), Choice::Brush(Mode::Smooth));

        // Each tool of the strip is a tool of the switch.
        for tool in [
            Tool::Pan,
            Tool::Line,
            Tool::Polygon,
            Tool::Point,
            Tool::Measure,
            Tool::Select,
            Tool::Split,
            Tool::Vertex,
        ] {
            globe.choose(Choice::Brush(Mode::Erode));
            globe.choose(Choice::Tool(tool));
            globe.switch(PenSwitch::Last);
            assert_eq!(globe.choice(), Choice::Brush(Mode::Erode), "{tool:?}");
            globe.switch(PenSwitch::Last);
            assert_eq!(globe.choice(), Choice::Tool(tool));
        }

        // A tool that is not available does not change.
        globe.choose(Choice::Brush(Mode::Carve));
        let (brush, line) = (&mut globe.brush, &mut globe.line);
        let refuses = |choice| choice != Choice::Tool(Tool::Vertex);
        assert!(
            !globe
                .tools
                .switch(PenSwitch::Last, &mut globe.tool, brush, line, refuses)
        );
        assert_eq!(globe.choice(), Choice::Brush(Mode::Carve));
    }

    #[test]
    fn the_eraser_switch_goes_to_the_opposite_mode() {
        let mut globe = Globe::new();
        globe.choose(Choice::Tool(Tool::Line));
        globe.choose(Choice::Brush(Mode::Raise));
        globe.switch(PenSwitch::Eraser);
        assert_eq!(globe.choice(), Choice::Brush(Mode::Lower));
        globe.switch(PenSwitch::Eraser);
        assert_eq!(globe.choice(), Choice::Brush(Mode::Raise));

        globe.choose(Choice::Brush(Mode::Noise));
        globe.brush.noise_raise_m = 1000.0;
        globe.brush.noise_lower_m = 200.0;
        globe.switch(PenSwitch::Eraser);
        assert_eq!(globe.choice(), Choice::Brush(Mode::Noise));
        assert_eq!(globe.brush.noise_raise_m, 200.0);
        assert_eq!(globe.brush.noise_lower_m, 1000.0);

        // A mode with no opposite goes to the last tool.
        globe.choose(Choice::Brush(Mode::Smooth));
        globe.switch(PenSwitch::Eraser);
        assert_eq!(globe.choice(), Choice::Brush(Mode::Noise));
        globe.choose(Choice::Brush(Mode::Raise));
        globe.choose(Choice::Tool(Tool::Line));
        globe.switch(PenSwitch::Eraser);
        assert_eq!(globe.choice(), Choice::Brush(Mode::Raise));
    }

    #[test]
    fn each_brush_mode_keeps_its_settings() {
        let mut globe = Globe::new();
        globe.brush.size_points = 150.0;
        globe.brush.lock = Some(0.1);
        globe.brush.hardness = 0.9;
        globe.brush.flow = 0.4;
        globe.brush.strength_m = 3000.0;
        let raise = ModeSettings::of(&globe.brush);

        // A new mode has the settings of a new brush.
        globe.choose(Choice::Brush(Mode::Erode));
        assert_eq!(
            ModeSettings::of(&globe.brush),
            ModeSettings::of(&BrushSettings::default())
        );
        globe.brush.size_points = 8.0;
        let erode = ModeSettings::of(&globe.brush);

        globe.switch(PenSwitch::Last);
        assert_eq!(ModeSettings::of(&globe.brush), raise);
        globe.switch(PenSwitch::Last);
        assert_eq!(ModeSettings::of(&globe.brush), erode);

        // A mode keeps its settings while another tool is the tool.
        globe.choose(Choice::Tool(Tool::Line));
        globe.choose(Choice::Brush(Mode::Raise));
        assert_eq!(ModeSettings::of(&globe.brush), raise);
    }

    #[test]
    fn the_line_tool_and_the_polygon_tool_each_keep_their_smoothing() {
        let mut globe = Globe::new();
        globe.choose(Choice::Tool(Tool::Line));
        globe.line.smoothing = 0.1;
        globe.choose(Choice::Tool(Tool::Polygon));
        globe.line.smoothing = 0.9;
        globe.choose(Choice::Brush(Mode::Raise));
        globe.choose(Choice::Tool(Tool::Line));
        assert_eq!(globe.line.smoothing, 0.1);
        globe.choose(Choice::Tool(Tool::Polygon));
        assert_eq!(globe.line.smoothing, 0.9);
    }

    #[test]
    fn the_settings_are_the_same_after_a_write_and_a_read() {
        let mut globe = Globe::new();
        globe.brush.size_points = 150.0;
        globe.brush.lock = Some(0.25);
        globe.choose(Choice::Brush(Mode::Erode));
        globe.brush.size_points = 8.0;
        globe.brush.hardness = 0.75;
        globe.brush.noise_ridged = true;
        globe.brush.channel_km = 2.5;
        globe.choose(Choice::Tool(Tool::Line));
        globe.line.smoothing = 0.1;
        globe.line.fit = true;
        globe.choose(Choice::Tool(Tool::Polygon));
        globe.line.smoothing = 0.9;
        globe.line.polygon_mode = PolygonMode::Subtract;
        let mut snap = SnapSettings {
            distance: 20.0,
            ..SnapSettings::default()
        };
        snap.on = false;
        assert!(!globe.tools.focus_new);
        globe.tools.focus_new = true;
        // With no choice, the file has no line for the finger.
        let text = globe.tools.text(&globe.brush, &globe.line, &snap);
        assert!(!text.contains("finger_paints"));
        globe.tools.finger_paints = Some(false);

        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()), None);
        let text = globe.tools.text(&globe.brush, &globe.line, &snap);
        // The snap targets are in the project file.
        assert!(!text.contains("snap_vertices") && !text.contains("snap_edges"));
        write(dir.path(), &text).unwrap();

        let mut next = Globe::new();
        let mut next_snap = SnapSettings::default();
        let text = read(dir.path()).unwrap();
        let (brush, line) = (&mut next.brush, &mut next.line);
        let mut tool = Tool::Pan;
        next.tools
            .load(&text, &mut tool, brush, line, &mut next_snap);
        assert_eq!(tool, Tool::Polygon);
        assert_eq!(next.brush.mode, Mode::Erode);
        assert_eq!(next.brush.size_points, 8.0);
        next.choose(Choice::Brush(Mode::Raise));
        assert_eq!(next.brush.size_points, 150.0);
        assert_eq!(next.brush.lock, Some(0.25));
        assert!(next.brush.noise_ridged);
        assert_eq!(next.brush.channel_km, 2.5);
        assert_eq!(next_snap, snap);
        assert!(next.line.fit);
        assert_eq!(next.line.polygon_mode, PolygonMode::Subtract);
        assert!(next.tools.focus_new);
        assert_eq!(next.tools.finger_paints, Some(false));

        next.choose(Choice::Brush(Mode::Erode));
        assert_eq!((next.brush.size_points, next.brush.hardness), (8.0, 0.75));
        next.choose(Choice::Tool(Tool::Line));
        assert_eq!(next.line.smoothing, 0.1);
        next.choose(Choice::Tool(Tool::Polygon));
        assert_eq!(next.line.smoothing, 0.9);
    }

    #[test]
    fn a_bad_line_has_no_effect() {
        let mut globe = Globe::new();
        let mut snap = SnapSettings::default();
        let text = "mode raise 99 none 0.5\n\
                    mode erode NaN none 0.5 1 1500\n\
                    mode lower 999 none 2 0 1\n\
                    flood 3\n\
                    talus_deg inf\n\
                    snap_distance -4\n\
                    smoothing 0.5";
        let (brush, line) = (&mut globe.brush, &mut globe.line);
        globe
            .tools
            .load(text, &mut Tool::Pan, brush, line, &mut snap);
        assert_eq!(globe.brush, BrushSettings::default());
        assert_eq!(globe.line, LineSettings::default());
        assert_eq!(snap, SnapSettings::default());
        // A file of an older version with the lines of the snap types loads.
        let old = "snap false\nsnap_vertices false\nsnap_edges false\nsnap_distance 20";
        let (brush, line) = (&mut globe.brush, &mut globe.line);
        globe
            .tools
            .load(old, &mut Tool::Pan, brush, line, &mut snap);
        assert!(!snap.on);
        assert_eq!(snap.distance, 20.0);
        assert_eq!(snap.targets, SnapSettings::default().targets);
        // A value out of its limits goes to the limit.
        globe.choose(Choice::Brush(Mode::Lower));
        assert_eq!(globe.brush.size_points, *SIZE_POINTS.end());
        assert_eq!(globe.brush.hardness, 1.0);
        assert_eq!(globe.brush.flow, *FLOW.start());
        assert_eq!(globe.brush.strength_m, *STRENGTH_M.start());
    }
}
