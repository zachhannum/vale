//! The color ramps of the app. Each world can use them. The library has the
//! built-in ramps, which do not change, and then the custom ramps. The
//! custom ramps are a text file in the folder of the state, with one ramp
//! on each line: the name, a tab, and the hex codes of the colors.

use std::path::{Path, PathBuf};

use vale_store::RasterId;
use vale_terrain::{MAX_BANDS, Rgb};

/// The file in the folder of the state that holds the ramps.
const RAMPS_FILE: &str = "ramps.txt";

/// The word before the colors of a qualitative custom ramp in the file.
const QUALITATIVE_WORD: &str = "qualitative";

/// The fewest colors of a ramp that the editor makes.
pub const MIN_COLORS: usize = 2;

/// The most colors of a ramp that the editor makes.
pub const MAX_COLORS: usize = MAX_BANDS;

/// The colors of the blocks of a legend, from the lowest band.
#[derive(Clone, Debug, PartialEq)]
pub struct Ramp {
    pub name: String,
    pub colors: Vec<Rgb>,
    /// The label of each color, for example a class name such as "Cfb". A
    /// blank label means no label. A class whose value is a label takes the
    /// color of the label.
    pub labels: Vec<String>,
    /// A built-in ramp does not change, and the library does not delete it.
    pub built_in: bool,
    /// The colors are for classes with no order, so they do not blend.
    pub qualitative: bool,
}

impl Ramp {
    /// A custom ramp.
    pub fn new(name: impl Into<String>, colors: Vec<Rgb>) -> Ramp {
        Ramp {
            name: name.into(),
            labels: vec![String::new(); colors.len()],
            colors,
            built_in: false,
            qualitative: false,
        }
    }

    /// Reverses the order of the colors and of their labels.
    pub fn reverse(&mut self) {
        self.colors.reverse();
        self.labels.reverse();
    }

    /// The index of the color with this label. Case does not count.
    pub fn labeled(&self, value: &str) -> Option<usize> {
        let value = value.trim();
        (!value.is_empty())
            .then(|| {
                self.labels
                    .iter()
                    .position(|l| l.trim().eq_ignore_ascii_case(value))
            })
            .flatten()
    }

    /// `n` colors for `n` classes: the colors in order for a qualitative
    /// ramp, and even steps from the first color to the last color for
    /// another ramp.
    pub fn classes(&self, n: usize) -> Vec<Rgb> {
        if self.qualitative {
            cycle(&self.colors, n)
        } else {
            sample(&self.colors, n)
        }
    }
}

/// The library of the ramps: the built-in ramps, then the custom ramps.
pub struct Ramps {
    /// The folder of the file. With no folder, the custom ramps are only in
    /// the memory.
    dir: Option<PathBuf>,
    list: Vec<Ramp>,
}

impl Default for Ramps {
    fn default() -> Self {
        Ramps {
            dir: None,
            list: built_in(),
        }
    }
}

impl Ramps {
    /// Reads the custom ramps of a folder, if it is not the folder of the
    /// list.
    pub fn open(&mut self, dir: Option<&Path>) {
        if self.dir.as_deref() == dir {
            return;
        }
        self.dir = dir.map(Path::to_path_buf);
        let text = dir.and_then(|d| std::fs::read_to_string(d.join(RAMPS_FILE)).ok());
        self.list = built_in();
        self.list.extend(text.map_or_else(Vec::new, |t| parse(&t)));
    }

    /// The built-in ramps, then the custom ramps.
    pub fn list(&self) -> &[Ramp] {
        &self.list
    }

    /// The custom ramps.
    pub fn custom(&self) -> &[Ramp] {
        let built_in = self.list.iter().take_while(|r| r.built_in).count();
        &self.list[built_in..]
    }

    /// Adds a ramp at the end, and returns its name. A name that another
    /// ramp has gets a number.
    pub fn add(&mut self, name: &str, colors: Vec<Rgb>) -> std::io::Result<String> {
        self.add_ramp(Ramp::new(name, colors))
    }

    /// Adds a ramp at the end as a custom ramp, and returns its name. A name
    /// that another ramp has gets a number.
    pub fn add_ramp(&mut self, mut ramp: Ramp) -> std::io::Result<String> {
        ramp.name = self.free_name(&ramp.name);
        ramp.built_in = false;
        ramp.labels.resize(ramp.colors.len(), String::new());
        for label in &mut ramp.labels {
            *label = label.replace(['\t', '\n', '\r'], " ");
        }
        let name = ramp.name.clone();
        self.list.push(ramp);
        self.write()?;
        Ok(name)
    }

    /// Whether a ramp has this name.
    pub fn taken(&self, name: &str) -> bool {
        self.list.iter().any(|r| r.name == name)
    }

    /// This name, or with a number if another ramp has the name. Tabs and
    /// line breaks become spaces.
    pub fn free_name(&self, name: &str) -> String {
        let name = name.replace(['\t', '\n', '\r'], " ");
        (1..)
            .map(|i| match i {
                1 => name.clone(),
                _ => format!("{name} {i}"),
            })
            .find(|n| !self.taken(n))
            .unwrap_or_default()
    }

    /// Reverses the order of the colors of a custom ramp. A built-in ramp
    /// does not change.
    pub fn reverse(&mut self, index: usize) -> std::io::Result<()> {
        match self.list.get_mut(index) {
            Some(ramp) if !ramp.built_in => ramp.reverse(),
            _ => return Ok(()),
        }
        self.write()
    }

    /// Sets the label of color `color` of a custom ramp. A built-in ramp does
    /// not change.
    pub fn set_label(&mut self, index: usize, color: usize, text: &str) -> std::io::Result<()> {
        let text = text.replace(['\t', '\n', '\r'], " ");
        match self.list.get_mut(index) {
            Some(ramp) if !ramp.built_in && color < ramp.labels.len() => {
                ramp.labels[color] = text;
            }
            _ => return Ok(()),
        }
        self.write()
    }

    /// Deletes a custom ramp. A built-in ramp stays.
    pub fn remove(&mut self, index: usize) -> std::io::Result<()> {
        if !self.list.get(index).is_some_and(|r| !r.built_in) {
            return Ok(());
        }
        self.list.remove(index);
        self.write()
    }

    /// Deletes the custom ramps at `indices` with one write. A built-in
    /// ramp stays.
    pub fn remove_all(&mut self, indices: &[usize]) -> std::io::Result<()> {
        let before = self.list.len();
        let mut i = 0;
        self.list.retain(|ramp| {
            let keep = ramp.built_in || !indices.contains(&i);
            i += 1;
            keep
        });
        if self.list.len() == before {
            return Ok(());
        }
        self.write()
    }

    /// A stop of the app during the write leaves the old file.
    fn write(&self) -> std::io::Result<()> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        std::fs::create_dir_all(dir)?;
        let partial = dir.join(format!("{RAMPS_FILE}.partial"));
        std::fs::write(&partial, text(self.custom()))?;
        std::fs::rename(partial, dir.join(RAMPS_FILE))
    }
}

/// The page that opened the editor of a ramp, and that Back returns to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftFrom {
    Stops,
    Ramps,
}

/// A ramp in the editor, before the save.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub name: String,
    pub colors: Vec<Rgb>,
    /// The label of each color.
    pub labels: Vec<String>,
    pub qualitative: bool,
    /// The row of the color picker.
    pub selected: Option<usize>,
    pub from: DraftFrom,
    /// The raster layer whose panel shows the note of the save.
    pub layer: Option<RasterId>,
}

impl Default for Draft {
    fn default() -> Self {
        Draft::new()
    }
}

impl Draft {
    /// A new ramp from white to black, with no name.
    pub fn new() -> Draft {
        Draft {
            name: String::new(),
            colors: vec![[255; 3], [0; 3]],
            labels: vec![String::new(); 2],
            qualitative: false,
            selected: None,
            from: DraftFrom::Ramps,
            layer: None,
        }
    }

    /// A custom copy of a ramp, with its labels. The name ends in "copy".
    pub fn copy(ramp: &Ramp, ramps: &Ramps) -> Draft {
        let mut draft = Draft {
            name: ramps.free_name(&format!("{} copy", ramp.name)),
            colors: ramp.colors.clone(),
            labels: ramp.labels.clone(),
            qualitative: ramp.qualitative,
            ..Draft::new()
        };
        draft.fit();
        draft
    }

    /// A ramp of the colors and the names of bands, from the lowest band.
    pub fn from_bands(name: &str, colors: &[Rgb], names: &[String], ramps: &Ramps) -> Draft {
        let mut draft = Draft {
            name: ramps.free_name(name),
            colors: colors.to_vec(),
            labels: names.to_vec(),
            from: DraftFrom::Stops,
            ..Draft::new()
        };
        draft.fit();
        draft
    }

    /// Gives each color one label, and keeps the count of the colors from
    /// `MIN_COLORS` to `MAX_COLORS`. Missing colors copy the last color.
    fn fit(&mut self) {
        self.colors.truncate(MAX_COLORS);
        let last = self.colors.last().copied().unwrap_or([255; 3]);
        while self.colors.len() < MIN_COLORS {
            self.colors.push(last);
        }
        self.labels.resize(self.colors.len(), String::new());
    }

    pub fn can_add(&self) -> bool {
        self.colors.len() < MAX_COLORS
    }

    pub fn can_remove(&self) -> bool {
        self.colors.len() > MIN_COLORS
    }

    /// Adds a copy of the selected color, or of the last color, after it,
    /// and selects the new row.
    pub fn add_color(&mut self) -> bool {
        if !self.can_add() {
            return false;
        }
        let last = self.colors.len() - 1;
        let at = self.selected.map_or(last, |i| i.min(last));
        self.colors.insert(at + 1, self.colors[at]);
        self.labels.insert(at + 1, String::new());
        self.selected = Some(at + 1);
        true
    }

    /// Removes a color and its label.
    pub fn remove(&mut self, index: usize) -> bool {
        if !self.can_remove() || index >= self.colors.len() {
            return false;
        }
        self.colors.remove(index);
        self.labels.remove(index);
        self.selected = match self.selected {
            Some(i) if i == index => None,
            Some(i) if i > index => Some(i - 1),
            other => other,
        };
        true
    }

    /// Moves a color and its label one row toward the first color.
    pub fn move_up(&mut self, index: usize) -> bool {
        index > 0 && self.swap(index - 1, index)
    }

    /// Moves a color and its label one row toward the last color.
    pub fn move_down(&mut self, index: usize) -> bool {
        self.swap(index, index + 1)
    }

    /// Swaps two adjacent rows. The selection stays with its color.
    fn swap(&mut self, a: usize, b: usize) -> bool {
        if b >= self.colors.len() {
            return false;
        }
        self.colors.swap(a, b);
        self.labels.swap(a, b);
        self.selected = match self.selected {
            Some(i) if i == a => Some(b),
            Some(i) if i == b => Some(a),
            other => other,
        };
        true
    }

    /// The name of the save: the name with no space at the ends, and spaces
    /// for tabs and line breaks.
    fn clean_name(&self) -> String {
        self.name.trim().replace(['\t', '\n', '\r'], " ")
    }

    /// A ramp can save with a name that is not blank and that no other
    /// ramp has.
    pub fn can_save(&self, ramps: &Ramps) -> bool {
        let name = self.clean_name();
        !name.is_empty() && !ramps.taken(&name)
    }

    /// The custom ramp of the draft.
    pub fn to_ramp(&self) -> Ramp {
        Ramp {
            name: self.clean_name(),
            colors: self.colors.clone(),
            labels: self.labels.clone(),
            built_in: false,
            qualitative: self.qualitative,
        }
    }
}

/// One line for each ramp: the name, a tab, the colors, and then a tab and
/// a label for each color if a color has a label. The colors of a
/// qualitative ramp follow the word `qualitative`.
fn text(list: &[Ramp]) -> String {
    list.iter()
        .map(|ramp| {
            let mut words: Vec<String> = ramp.colors.iter().map(|c| hex(*c)).collect();
            if ramp.qualitative {
                words.insert(0, QUALITATIVE_WORD.to_owned());
            }
            let mut line = format!("{}\t{}", ramp.name, words.join(" "));
            if ramp.labels.iter().any(|l| !l.is_empty()) {
                for label in &ramp.labels {
                    line.push('\t');
                    line.push_str(label);
                }
            }
            line.push('\n');
            line
        })
        .collect()
}

/// A line that is not a ramp does not count.
fn parse(text: &str) -> Vec<Ramp> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let (name, colors) = (parts.next()?, parts.next()?);
            let mut words = colors.split_whitespace().peekable();
            let qualitative = words.next_if_eq(&QUALITATIVE_WORD).is_some();
            let colors: Option<Vec<Rgb>> = words.map(parse_hex).collect();
            let colors = colors.filter(|c| !c.is_empty())?;
            let mut ramp = Ramp::new(name, colors);
            ramp.qualitative = qualitative;
            for (label, text) in ramp.labels.iter_mut().zip(parts) {
                *label = text.to_owned();
            }
            Some(ramp)
        })
        .collect()
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn parse_hex(text: &str) -> Option<Rgb> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// The built-in ramps. The last two are qualitative, and the Köppen ramp
/// follows them.
const BUILT_IN: [(&str, &[Rgb]); 15] = [
    (
        "Viridis",
        &[
            [68, 1, 84],
            [59, 82, 139],
            [33, 145, 140],
            [94, 201, 98],
            [253, 231, 37],
        ],
    ),
    (
        "Greens",
        &[
            [237, 248, 233],
            [186, 228, 179],
            [116, 196, 118],
            [49, 163, 84],
            [0, 109, 44],
        ],
    ),
    (
        "Blues",
        &[
            [239, 243, 255],
            [189, 215, 231],
            [107, 174, 214],
            [49, 130, 189],
            [8, 81, 156],
        ],
    ),
    (
        "Reds",
        &[
            [254, 229, 217],
            [252, 174, 145],
            [251, 106, 74],
            [222, 45, 38],
            [165, 15, 21],
        ],
    ),
    (
        "Spectral",
        &[
            [215, 25, 28],
            [253, 174, 97],
            [255, 255, 191],
            [171, 221, 164],
            [43, 131, 186],
        ],
    ),
    (
        "Earth",
        &[
            [245, 240, 220],
            [217, 201, 154],
            [168, 180, 108],
            [107, 142, 78],
            [62, 92, 58],
        ],
    ),
    (
        "Hypsometric",
        &[
            [38, 115, 60],
            [72, 145, 70],
            [120, 170, 85],
            [170, 195, 105],
            [215, 215, 130],
            [235, 210, 130],
            [225, 180, 105],
            [205, 145, 80],
            [180, 110, 65],
            [150, 85, 55],
            [125, 75, 60],
            [150, 130, 120],
            [190, 180, 175],
            [230, 226, 222],
            [255, 255, 255],
        ],
    ),
    (
        "Relief",
        &[
            [156, 180, 150],
            [178, 196, 155],
            [200, 210, 162],
            [220, 220, 170],
            [234, 226, 178],
            [240, 224, 180],
            [238, 214, 176],
            [232, 206, 178],
            [236, 222, 206],
            [246, 240, 232],
        ],
    ),
    (
        "Alpine",
        &[
            [86, 130, 70],
            [120, 155, 85],
            [160, 175, 105],
            [150, 150, 110],
            [140, 125, 105],
            [125, 115, 105],
            [150, 145, 140],
            [185, 182, 180],
            [220, 220, 220],
            [250, 250, 250],
        ],
    ),
    (
        "Desert",
        &[
            [240, 225, 185],
            [232, 205, 150],
            [222, 182, 120],
            [210, 155, 95],
            [190, 125, 75],
            [165, 95, 60],
            [135, 75, 50],
            [110, 65, 50],
            [150, 120, 100],
            [200, 185, 165],
        ],
    ),
    (
        "Volcanic",
        &[
            [30, 28, 28],
            [55, 45, 42],
            [85, 55, 45],
            [120, 60, 40],
            [160, 65, 35],
            [200, 80, 30],
            [235, 120, 40],
            [250, 175, 70],
            [255, 230, 150],
        ],
    ),
    (
        "Bathymetry",
        &[
            [8, 29, 74],
            [12, 44, 104],
            [18, 62, 134],
            [28, 84, 160],
            [42, 108, 184],
            [62, 134, 204],
            [90, 160, 218],
            [124, 186, 230],
            [160, 208, 238],
            [198, 230, 246],
        ],
    ),
    (
        "Topography",
        &[
            [10, 30, 80],
            [16, 50, 115],
            [26, 76, 150],
            [42, 105, 180],
            [70, 138, 205],
            [110, 172, 225],
            [160, 205, 240],
            [200, 230, 248],
            [52, 128, 72],
            [95, 155, 80],
            [145, 180, 95],
            [195, 205, 115],
            [230, 215, 130],
            [225, 185, 105],
            [205, 150, 80],
            [180, 115, 65],
            [150, 90, 60],
            [130, 95, 80],
            [165, 150, 140],
            [205, 198, 192],
            [240, 238, 236],
            [255, 255, 255],
        ],
    ),
    (
        "Bold",
        &[
            [27, 158, 119],
            [217, 95, 2],
            [117, 112, 179],
            [231, 41, 138],
            [102, 166, 30],
            [230, 171, 2],
            [166, 118, 29],
            [102, 102, 102],
        ],
    ),
    (
        "Pastel",
        &[
            [251, 180, 174],
            [179, 205, 227],
            [204, 235, 197],
            [222, 203, 228],
            [254, 217, 166],
            [255, 255, 204],
            [229, 216, 189],
            [253, 218, 236],
        ],
    ),
];

/// The index in `BUILT_IN` of the first qualitative ramp.
const QUALITATIVE: usize = 13;

/// The name of the ramp of the climate classes of Köppen and Geiger.
pub const KOPPEN: &str = "Köppen climate";

/// The classes of Köppen and Geiger and their colors, in the common legend
/// of Beck and others (2018).
const KOPPEN_CLASSES: [(&str, Rgb); 30] = [
    ("Af", [0, 0, 255]),
    ("Am", [0, 120, 255]),
    ("Aw", [70, 170, 250]),
    ("BWh", [255, 0, 0]),
    ("BWk", [255, 150, 150]),
    ("BSh", [245, 165, 0]),
    ("BSk", [255, 220, 100]),
    ("Csa", [255, 255, 0]),
    ("Csb", [200, 200, 0]),
    ("Csc", [150, 150, 0]),
    ("Cwa", [150, 255, 150]),
    ("Cwb", [100, 200, 100]),
    ("Cwc", [50, 150, 50]),
    ("Cfa", [200, 255, 80]),
    ("Cfb", [100, 255, 80]),
    ("Cfc", [50, 200, 0]),
    ("Dsa", [255, 0, 255]),
    ("Dsb", [200, 0, 200]),
    ("Dsc", [150, 50, 150]),
    ("Dsd", [150, 100, 150]),
    ("Dwa", [170, 175, 255]),
    ("Dwb", [90, 120, 220]),
    ("Dwc", [75, 80, 180]),
    ("Dwd", [50, 0, 135]),
    ("Dfa", [0, 255, 255]),
    ("Dfb", [55, 200, 255]),
    ("Dfc", [0, 125, 125]),
    ("Dfd", [0, 70, 95]),
    ("ET", [178, 178, 178]),
    ("EF", [102, 102, 102]),
];

fn built_in() -> Vec<Ramp> {
    let mut list: Vec<Ramp> = BUILT_IN
        .iter()
        .enumerate()
        .map(|(i, (name, colors))| Ramp {
            name: (*name).to_owned(),
            colors: colors.to_vec(),
            labels: vec![String::new(); colors.len()],
            built_in: true,
            qualitative: i >= QUALITATIVE,
        })
        .collect();
    list.push(Ramp {
        name: KOPPEN.to_owned(),
        colors: KOPPEN_CLASSES.iter().map(|(_, c)| *c).collect(),
        labels: KOPPEN_CLASSES
            .iter()
            .map(|(l, _)| (*l).to_owned())
            .collect(),
        built_in: true,
        qualitative: true,
    });
    list
}

/// `n` colors of a ramp from its first color to its last color, with
/// linear steps between its colors. One color is the middle of the ramp.
fn sample(colors: &[Rgb], n: usize) -> Vec<Rgb> {
    let Some(last) = colors.len().checked_sub(1) else {
        return Vec::new();
    };
    (0..n)
        .map(|k| {
            let t = if n < 2 {
                0.5
            } else {
                k as f64 / (n - 1) as f64
            } * last as f64;
            let i = (t.floor() as usize).min(last.saturating_sub(1));
            let (a, b) = (colors[i], colors[(i + 1).min(last)]);
            let f = t - i as f64;
            std::array::from_fn(|c| (a[c] as f64 + (b[c] as f64 - a[c] as f64) * f).round() as u8)
        })
        .collect()
}

/// `n` colors of a qualitative ramp: its colors in order, again from the
/// first color after the last.
fn cycle(colors: &[Rgb], n: usize) -> Vec<Rgb> {
    colors.iter().copied().cycle().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vale_terrain::Part;

    #[test]
    fn the_custom_ramps_stay_in_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut ramps = Ramps::default();
        ramps.open(Some(dir.path()));
        let name = ramps.add("Hypso\tmetric", vec![[1, 2, 3], [250, 128, 0]]);
        assert_eq!(name.unwrap(), "Hypso metric");
        assert_eq!(
            ramps.add("Hypso metric", vec![[9; 3]]).unwrap(),
            "Hypso metric 2"
        );

        let mut again = Ramps::default();
        again.open(Some(dir.path()));
        assert_eq!(again.list(), ramps.list());
        let first = Ramps::default().list().len();
        again.reverse(first).unwrap();
        let mut reversed = Ramps::default();
        reversed.open(Some(dir.path()));
        assert_eq!(reversed.custom()[0].colors, [[250, 128, 0], [1, 2, 3]]);
        again.remove(first).unwrap();
        let mut third = Ramps::default();
        third.open(Some(dir.path()));
        assert_eq!(third.custom().len(), 1);
        assert_eq!(third.custom()[0].colors, [[9; 3]]);
        let file = std::fs::read_to_string(dir.path().join(RAMPS_FILE)).unwrap();
        assert!(!file.contains("Viridis"), "{file}");
    }

    #[test]
    fn the_built_in_ramps_come_first_and_do_not_change() {
        let mut ramps = Ramps::default();
        assert_eq!(ramps.list().len(), BUILT_IN.len() + 1);
        assert!(ramps.custom().is_empty());
        assert!(ramps.list().iter().all(|r| r.built_in));
        let viridis = ramps.list()[0].clone();
        ramps.reverse(0).unwrap();
        ramps.remove(0).unwrap();
        assert_eq!(ramps.list()[0], viridis);
        assert_eq!(ramps.add("Viridis", vec![[0; 3]]).unwrap(), "Viridis 2");
        assert!(!ramps.custom()[0].built_in);
    }

    #[test]
    fn classes_blend_or_cycle_the_colors() {
        let ramps = Ramps::default();
        let bold = ramps.list().iter().find(|r| r.name == "Bold").unwrap();
        assert!(bold.qualitative);
        assert_eq!(bold.classes(9)[8], bold.colors[0]);
        let viridis = &ramps.list()[0];
        assert!(!viridis.qualitative);
        assert_eq!(viridis.classes(5), viridis.colors);
    }

    #[test]
    fn a_sample_has_both_ends_and_the_steps_between() {
        let colors = [[0, 0, 0], [100, 200, 50]];
        assert_eq!(
            sample(&colors, 3),
            vec![[0, 0, 0], [50, 100, 25], [100, 200, 50]]
        );
        assert_eq!(sample(&colors, 1), vec![[50, 100, 25]]);
        let five = BUILT_IN[0].1;
        assert_eq!(sample(five, 5), five.to_vec());
        assert_eq!(
            cycle(&colors, 3),
            vec![[0, 0, 0], [100, 200, 50], [0, 0, 0]]
        );
    }

    #[test]
    fn the_labels_stay_in_the_folder_and_reverse_with_the_colors() {
        let dir = tempfile::tempdir().unwrap();
        let mut ramps = Ramps::default();
        ramps.open(Some(dir.path()));
        ramps.add("Zones", vec![[1; 3], [2; 3], [3; 3]]).unwrap();
        let at = ramps.list().len() - 1;
        ramps.set_label(at, 0, "Wet\tland").unwrap();
        ramps.set_label(at, 2, "Dry").unwrap();
        ramps.reverse(at).unwrap();
        let mut again = Ramps::default();
        again.open(Some(dir.path()));
        let zones = &again.custom()[0];
        assert_eq!(zones.labels, ["Dry", "", "Wet land"]);
        assert_eq!(zones.colors, [[3; 3], [2; 3], [1; 3]]);
        assert_eq!(zones.labeled("wet land "), Some(2));
        assert_eq!(zones.labeled(""), None);
        // A line of an older file has no labels.
        let old = parse("Old\t#010203 #040506\n");
        assert_eq!(old[0].labels, ["", ""]);
    }

    #[test]
    fn the_koppen_ramp_has_a_label_for_each_color() {
        let mut ramps = Ramps::default();
        let at = ramps.list().iter().position(|r| r.name == KOPPEN).unwrap();
        let koppen = ramps.list()[at].clone();
        assert!(koppen.built_in && koppen.qualitative);
        assert_eq!(koppen.colors.len(), 30);
        assert_eq!(koppen.labels.len(), 30);
        assert_eq!(
            koppen.colors[koppen.labeled("Cfb").unwrap()],
            [100, 255, 80]
        );
        ramps.set_label(at, 0, "Rain").unwrap();
        assert_eq!(ramps.list()[at], koppen);
    }

    #[test]
    fn a_bad_line_does_not_count() {
        let list = parse("A\t#010203 #ffffff\nno tab\nB\t#12345\nC\t\n");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].colors, [[1, 2, 3], [255, 255, 255]]);
    }

    #[test]
    fn the_topography_ramps_split_at_sea_level() {
        let ramps = Ramps::default();
        let colors = |name: &str| {
            let ramp = ramps.list().iter().find(|r| r.name == name).unwrap();
            assert!(ramp.built_in && !ramp.qualitative, "{name}");
            ramp.colors.clone()
        };
        let height = vale_terrain::Bands::with_range(-8000.0, 8000.0, true).unwrap();
        for land in ["Hypsometric", "Relief", "Alpine", "Desert", "Volcanic"] {
            assert_eq!(height.part(&colors(land)), Part::Land, "{land}");
        }
        assert_eq!(height.part(&colors("Bathymetry")), Part::Sea);
        let topography = colors("Topography");
        assert_eq!(height.part(&topography), Part::All);
        assert_eq!(height.part(&topography[..8]), Part::Sea);
        assert_eq!(height.part(&topography[8..]), Part::Land);
    }

    #[test]
    fn with_no_folder_the_ramps_are_in_the_memory() {
        let mut ramps = Ramps::default();
        ramps.open(None);
        ramps.add("A", vec![[0; 3]]).unwrap();
        assert_eq!(ramps.custom().len(), 1);
    }

    #[test]
    fn the_qualitative_flag_and_labels_stay_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut ramps = Ramps::default();
        ramps.open(Some(dir.path()));
        let mut zones = Ramp::new("Zones", vec![[170; 3], [187; 3]]);
        zones.qualitative = true;
        zones.labels = vec!["Wet".to_owned(), "Dry\tland".to_owned()];
        assert_eq!(ramps.add_ramp(zones).unwrap(), "Zones");
        let file = std::fs::read_to_string(dir.path().join(RAMPS_FILE)).unwrap();
        assert!(
            file.starts_with("Zones\tqualitative #aaaaaa #bbbbbb\t"),
            "{file}"
        );
        let mut again = Ramps::default();
        again.open(Some(dir.path()));
        let zones = &again.custom()[0];
        assert!(zones.qualitative && !zones.built_in);
        assert_eq!(zones.labels, ["Wet", "Dry land"]);
        assert_eq!(zones.colors, [[170; 3], [187; 3]]);
    }

    #[test]
    fn an_old_line_has_no_qualitative_flag() {
        let list = parse("Old\t#010203 #040506\n");
        assert!(!list[0].qualitative);
        assert_eq!(list[0].colors, [[1, 2, 3], [4, 5, 6]]);
    }

    #[test]
    fn a_free_name_gets_a_number() {
        let mut ramps = Ramps::default();
        assert!(ramps.taken("Viridis"));
        assert!(!ramps.taken("viridis"));
        assert_eq!(ramps.free_name("Viridis"), "Viridis 2");
        assert_eq!(ramps.free_name("Mine"), "Mine");
        ramps.add("Viridis 2", vec![[0; 3]]).unwrap();
        assert_eq!(ramps.free_name("Viridis"), "Viridis 3");
        assert_eq!(ramps.free_name("A\tB"), "A B");
    }

    #[test]
    fn a_copy_of_a_built_in_ramp_is_custom_with_its_labels() {
        let mut ramps = Ramps::default();
        let koppen = ramps.list().iter().find(|r| r.name == KOPPEN).unwrap();
        let draft = Draft::copy(koppen, &ramps);
        assert_eq!(draft.name, "Köppen climate copy");
        assert_eq!(draft.labels, koppen.labels);
        assert_eq!(draft.colors, koppen.colors);
        assert!(draft.qualitative);
        assert_eq!(draft.from, DraftFrom::Ramps);
        assert!(draft.can_save(&ramps));
        let ramp = draft.to_ramp();
        assert!(!ramp.built_in);
        ramps.add_ramp(ramp).unwrap();
        let copy = ramps.custom().last().unwrap();
        assert_eq!(copy.name, "Köppen climate copy");
        assert!(copy.qualitative && !copy.built_in);
        let again = Draft::copy(&ramps.list()[0], &ramps);
        assert_eq!(again.name, "Viridis copy");
        let koppen = ramps.list().iter().find(|r| r.name == KOPPEN).unwrap();
        assert_eq!(Draft::copy(koppen, &ramps).name, "Köppen climate copy 2");
    }

    #[test]
    fn a_draft_has_two_to_32_colors() {
        let mut draft = Draft::new();
        assert_eq!(draft.colors, [[255; 3], [0; 3]]);
        assert!(!draft.can_remove());
        assert!(!draft.remove(0));
        draft.selected = Some(0);
        assert!(draft.add_color());
        assert_eq!(draft.colors, [[255; 3], [255; 3], [0; 3]]);
        assert_eq!(draft.selected, Some(1));
        draft.selected = None;
        while draft.add_color() {}
        assert_eq!(draft.colors.len(), MAX_COLORS);
        assert_eq!(MAX_COLORS, 32);
        assert_eq!(draft.labels.len(), MAX_COLORS);
        assert_eq!(draft.selected, Some(MAX_COLORS - 1));
        assert!(!draft.can_add());
        assert!(draft.remove(MAX_COLORS - 1));
        assert_eq!(draft.selected, None);
        assert_eq!(draft.labels.len(), MAX_COLORS - 1);
    }

    #[test]
    fn a_blank_or_taken_name_cannot_save() {
        let mut ramps = Ramps::default();
        let mut draft = Draft::new();
        assert!(!draft.can_save(&ramps));
        draft.name = "   ".to_owned();
        assert!(!draft.can_save(&ramps));
        draft.name = "Viridis ".to_owned();
        assert!(!draft.can_save(&ramps));
        draft.name = " Ocean ".to_owned();
        assert!(draft.can_save(&ramps));
        assert_eq!(draft.to_ramp().name, "Ocean");
        ramps.add_ramp(draft.to_ramp()).unwrap();
        assert!(!draft.can_save(&ramps));
    }

    #[test]
    fn colors_move_with_their_labels() {
        let mut draft = Draft::new();
        draft.labels = vec!["Light".to_owned(), "Dark".to_owned()];
        draft.selected = Some(0);
        assert!(!draft.move_up(0));
        assert!(draft.move_down(0));
        assert_eq!(draft.colors, [[0; 3], [255; 3]]);
        assert_eq!(draft.labels, ["Dark", "Light"]);
        assert_eq!(draft.selected, Some(1));
        assert!(!draft.move_down(1));
        assert!(draft.move_up(1));
        assert_eq!(draft.labels, ["Light", "Dark"]);
        assert_eq!(draft.selected, Some(0));
    }

    #[test]
    fn a_draft_from_bands_takes_their_colors_and_names() {
        let ramps = Ramps::default();
        let colors = [[1; 3], [2; 3], [3; 3]];
        let names = ["Sea".to_owned(), "Land".to_owned()];
        let draft = Draft::from_bands("Viridis", &colors, &names, &ramps);
        assert_eq!(draft.name, "Viridis 2");
        assert_eq!(draft.colors, colors);
        assert_eq!(draft.labels, ["Sea", "Land", ""]);
        assert!(!draft.qualitative);
        assert_eq!(draft.from, DraftFrom::Stops);
        let one = Draft::from_bands("One", &[[7; 3]], &[], &ramps);
        assert_eq!(one.colors, [[7; 3], [7; 3]]);
        let many = Draft::from_bands("Many", &[[0; 3]; 40], &[], &ramps);
        assert_eq!(many.colors.len(), MAX_COLORS);
    }
}
