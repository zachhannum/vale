//! The state that the next start of an app puts back: the open project and
//! its view. The state is a text file in the projects folder, with one key
//! and its values on each line.

use std::path::{Path, PathBuf};

/// The file in the projects folder that holds the state.
const RESUME_FILE: &str = "resume.txt";

/// The view of an open project, as lines of the state file.
pub trait ResumeView: Sized {
    /// Adds a line for each key of the view: the key, a space, and the
    /// values.
    fn lines(&self, out: &mut Vec<String>);

    /// Reads the view. `None`: a line of the view is absent or bad.
    fn parse(lines: &Lines) -> Option<Self>;
}

/// The lines of a state file.
#[derive(Clone, Copy, Debug)]
pub struct Lines<'a> {
    text: &'a str,
}

impl<'a> Lines<'a> {
    pub fn new(text: &'a str) -> Lines<'a> {
        Lines { text }
    }

    /// The values of the first line with this key.
    pub fn value(&self, key: &str) -> Option<&'a str> {
        let mut lines = self.text.lines().filter_map(|line| line.split_once(' '));
        lines.find_map(|(k, value)| (k == key).then_some(value))
    }

    /// The values of the line with this key, as `count` finite numbers.
    pub fn floats(&self, key: &str, count: usize) -> Option<Vec<f64>> {
        floats(self.value(key)?, count)
    }
}

/// Reads `count` finite numbers that spaces separate.
pub fn floats(text: &str, count: usize) -> Option<Vec<f64>> {
    let parsed = text.split(' ').map(|v| v.parse::<f64>().ok());
    let floats: Vec<f64> = parsed.collect::<Option<_>>()?;
    (floats.len() == count && floats.iter().all(|v| v.is_finite())).then_some(floats)
}

/// Writes numbers with a space between each two.
pub fn numbers(values: &[f64]) -> String {
    let texts: Vec<String> = values.iter().map(f64::to_string).collect();
    texts.join(" ")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resume<V> {
    /// The open project file. `None`: the app shows the start screen.
    pub project: Option<PathBuf>,
    /// The view of the project. `None`: the project has its first view.
    pub view: Option<V>,
}

impl<V> Resume<V> {
    /// The state with no open project.
    pub const START_SCREEN: Resume<V> = Resume {
        project: None,
        view: None,
    };
}

impl<V: ResumeView> Resume<V> {
    /// The state of the last run in this projects folder. `None`: the folder
    /// has no state.
    pub fn read(dir: &Path) -> Option<Resume<V>> {
        let text = std::fs::read_to_string(dir.join(RESUME_FILE)).ok()?;
        Some(Resume::parse(&text))
    }

    /// Writes the state to the projects folder. A stop of the app during the
    /// write leaves the old file.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let partial = dir.join(format!("{RESUME_FILE}.partial"));
        std::fs::write(&partial, self.text())?;
        std::fs::rename(partial, dir.join(RESUME_FILE))
    }

    /// The text of the state file.
    pub fn text(&self) -> String {
        let mut lines = Vec::new();
        if let Some(project) = &self.project {
            lines.push(format!("project {}", project.display()));
        }
        if let Some(view) = &self.view {
            view.lines(&mut lines);
        }
        lines.join("\n")
    }

    /// A line that this version does not know has no effect. If a line of
    /// the view is absent or bad, the state has no view.
    pub fn parse(text: &str) -> Resume<V> {
        let lines = Lines::new(text);
        Resume {
            project: lines.value("project").map(PathBuf::from),
            view: V::parse(&lines),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Stub {
        name: String,
        center: [f64; 2],
    }

    impl ResumeView for Stub {
        fn lines(&self, out: &mut Vec<String>) {
            out.push(format!("name {}", self.name));
            out.push(format!("center {}", numbers(&self.center)));
        }

        fn parse(lines: &Lines) -> Option<Stub> {
            let center = lines.floats("center", 2)?;
            Some(Stub {
                name: lines.value("name")?.to_string(),
                center: [center[0], center[1]],
            })
        }
    }

    #[test]
    fn the_file_gives_back_the_project_and_the_view() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Resume::<Stub>::read(dir.path()), None);
        let resume = Resume {
            project: Some(dir.path().join("My world.gpkg")),
            view: Some(Stub {
                name: "two words".to_string(),
                center: [0.25, -12.5],
            }),
        };
        resume.write(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join("resume.txt")).unwrap();
        let project = dir.path().join("My world.gpkg");
        let expected = format!(
            "project {}\nname two words\ncenter 0.25 -12.5",
            project.display()
        );
        assert_eq!(text, expected);
        assert_eq!(Resume::read(dir.path()), Some(resume.clone()));

        Resume::<Stub>::START_SCREEN.write(dir.path()).unwrap();
        assert_eq!(Resume::read(dir.path()), Some(Resume::<Stub>::START_SCREEN));
    }

    #[test]
    fn a_bad_view_gives_the_project_with_no_view() {
        let text = "project /a.gpkg\nname a\ncenter 1 NaN\nlater_key 1";
        let resume = Resume::<Stub>::parse(text);
        assert_eq!(resume.project, Some(PathBuf::from("/a.gpkg")));
        assert_eq!(resume.view, None);
        assert_eq!(floats("1 2", 2), Some(vec![1.0, 2.0]));
        assert_eq!(floats("1 2", 3), None);
    }
}
