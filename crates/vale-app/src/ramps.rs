//! The color ramps of the app. Each world can use them. The library is a
//! text file in the folder of the state, with one ramp on each line: the
//! name, a tab, and the hex codes of the colors.

use std::path::{Path, PathBuf};

use vale_terrain::Rgb;

/// The file in the folder of the state that holds the ramps.
const RAMPS_FILE: &str = "ramps.txt";

/// The colors of the blocks of a legend, from the lowest band.
#[derive(Clone, Debug, PartialEq)]
pub struct Ramp {
    pub name: String,
    pub colors: Vec<Rgb>,
}

#[derive(Default)]
pub struct Ramps {
    /// The folder of the file. With no folder, the ramps are only in the
    /// memory.
    dir: Option<PathBuf>,
    list: Vec<Ramp>,
}

impl Ramps {
    /// Reads the ramps of a folder, if it is not the folder of the list.
    pub fn open(&mut self, dir: Option<&Path>) {
        if self.dir.as_deref() == dir {
            return;
        }
        self.dir = dir.map(Path::to_path_buf);
        let text = dir.and_then(|d| std::fs::read_to_string(d.join(RAMPS_FILE)).ok());
        self.list = text.map_or_else(Vec::new, |t| parse(&t));
    }

    pub fn list(&self) -> &[Ramp] {
        &self.list
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
        self.list.push(Ramp {
            name: name.clone(),
            colors,
        });
        self.write()?;
        Ok(name)
    }

    /// Reverses the order of the colors of a ramp.
    pub fn reverse(&mut self, index: usize) -> std::io::Result<()> {
        if let Some(ramp) = self.list.get_mut(index) {
            ramp.colors.reverse();
        }
        self.write()
    }

    pub fn remove(&mut self, index: usize) -> std::io::Result<()> {
        if index < self.list.len() {
            self.list.remove(index);
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
        std::fs::write(&partial, text(&self.list))?;
        std::fs::rename(partial, dir.join(RAMPS_FILE))
    }
}

fn text(list: &[Ramp]) -> String {
    list.iter()
        .map(|ramp| {
            let colors: Vec<String> = ramp.colors.iter().map(|c| hex(*c)).collect();
            format!("{}\t{}\n", ramp.name, colors.join(" "))
        })
        .collect()
}

/// A line that is not a ramp does not count.
fn parse(text: &str) -> Vec<Ramp> {
    text.lines()
        .filter_map(|line| {
            let (name, colors) = line.split_once('\t')?;
            let colors: Option<Vec<Rgb>> = colors.split_whitespace().map(parse_hex).collect();
            let colors = colors.filter(|c| !c.is_empty())?;
            Some(Ramp {
                name: name.to_owned(),
                colors,
            })
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

/// The ramps that the app has, before the ramps of the library. The last two
/// are qualitative: their colors are for classes with no order.
pub const BUILT_IN: [(&str, &[Rgb]); 8] = [
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
pub const QUALITATIVE: usize = 6;

/// `n` colors of a ramp from its first color to its last color, with
/// linear steps between its colors. One color is the middle of the ramp.
pub fn sample(colors: &[Rgb], n: usize) -> Vec<Rgb> {
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
pub fn cycle(colors: &[Rgb], n: usize) -> Vec<Rgb> {
    colors.iter().copied().cycle().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramps_stay_in_the_folder() {
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
        again.reverse(0).unwrap();
        let mut reversed = Ramps::default();
        reversed.open(Some(dir.path()));
        assert_eq!(reversed.list()[0].colors, [[250, 128, 0], [1, 2, 3]]);
        again.remove(0).unwrap();
        let mut third = Ramps::default();
        third.open(Some(dir.path()));
        assert_eq!(third.list().len(), 1);
        assert_eq!(third.list()[0].colors, [[9; 3]]);
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
        assert_eq!(ramps.list().len(), 1);
    }
}
