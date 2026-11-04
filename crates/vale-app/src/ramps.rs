//! The color ramps of the app. Each world can use them. The library has the
//! built-in ramps, which do not change, and then the custom ramps. The
//! custom ramps are a text file in the folder of the state, with one ramp
//! on each line: the name, a tab, and the hex codes of the colors.

use std::path::{Path, PathBuf};

use vale_terrain::Rgb;

/// The file in the folder of the state that holds the ramps.
const RAMPS_FILE: &str = "ramps.txt";

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
        let name = name.replace(['\t', '\n', '\r'], " ");
        let taken = |n: &str| self.list.iter().any(|r| r.name == n);
        let name = (1..)
            .map(|i| match i {
                1 => name.clone(),
                _ => format!("{name} {i}"),
            })
            .find(|n| !taken(n))
            .unwrap_or_default();
        self.list.push(Ramp::new(name.clone(), colors));
        self.write()?;
        Ok(name)
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

/// One line for each ramp: the name, a tab, the colors, and then a tab and
/// a label for each color if a color has a label.
fn text(list: &[Ramp]) -> String {
    list.iter()
        .map(|ramp| {
            let colors: Vec<String> = ramp.colors.iter().map(|c| hex(*c)).collect();
            let mut line = format!("{}\t{}", ramp.name, colors.join(" "));
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
            let colors: Option<Vec<Rgb>> = colors.split_whitespace().map(parse_hex).collect();
            let colors = colors.filter(|c| !c.is_empty())?;
            let mut ramp = Ramp::new(name, colors);
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
const BUILT_IN: [(&str, &[Rgb]); 8] = [
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
const QUALITATIVE: usize = 6;

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
    fn with_no_folder_the_ramps_are_in_the_memory() {
        let mut ramps = Ramps::default();
        ramps.open(None);
        ramps.add("A", vec![[0; 3]]).unwrap();
        assert_eq!(ramps.custom().len(), 1);
    }
}
