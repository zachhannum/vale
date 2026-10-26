//! The state that the next start of the app puts back: the open project and
//! its view. The state is a text file in the projects folder, with one key
//! and its values on each line.

use std::path::{Path, PathBuf};

use vale_sphere::{Point, ProjectionKind, ProjectionSpec};
use vale_store::PageId;

use crate::globe::math::Mat3;
use crate::globe::view::GlobeView;
use crate::globe::{WorldView, flat, view};
use crate::ui::Workspace;

/// The file in the projects folder that holds the state.
const RESUME_FILE: &str = "resume.txt";

/// The view of the open world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub workspace: Workspace,
    pub world_view: WorldView,
    pub globe: GlobeView,
    /// The projection of the flat view.
    pub flat_spec: ProjectionSpec,
    /// The point of the flat map at the middle of the canvas.
    pub flat_center: Point,
    pub flat_zoom: f64,
    /// The open page of the atlas. `None`: the first page.
    pub page: Option<PageId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resume {
    /// The open project file. `None`: the app shows the start screen.
    pub project: Option<PathBuf>,
    /// The view of the project. `None`: the project has its first view.
    pub view: Option<View>,
}

impl Resume {
    /// The state with no open project.
    pub const START_SCREEN: Resume = Resume {
        project: None,
        view: None,
    };

    /// The state of the last run in this projects folder. `None`: the folder
    /// has no state.
    pub fn read(dir: &Path) -> Option<Resume> {
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

    fn text(&self) -> String {
        let mut lines = Vec::new();
        if let Some(project) = &self.project {
            lines.push(format!("project {}", project.display()));
        }
        if let Some(view) = &self.view {
            let workspace = match view.workspace {
                Workspace::Globe => "globe",
                Workspace::Atlas => "atlas",
            };
            let world_view = match view.world_view {
                WorldView::Globe => "globe",
                WorldView::Flat => "flat",
            };
            let rot: Vec<f64> = view.globe.rot.0.iter().flatten().copied().collect();
            let spec = view.flat_spec;
            lines.push(format!("workspace {workspace}"));
            lines.push(format!("world_view {world_view}"));
            lines.push(format!("globe_rot {}", numbers(&rot)));
            lines.push(format!("globe_zoom {}", view.globe.zoom));
            lines.push(format!("flat_projection {}", spec.to_proj()));
            let center = view.flat_center;
            lines.push(format!("flat_center {}", numbers(&[center.x, center.y])));
            lines.push(format!("flat_zoom {}", view.flat_zoom));
            if let Some(page) = view.page {
                lines.push(format!("page {}", page.0));
            }
        }
        lines.join("\n")
    }

    /// A line that this version does not know has no effect. If a line of
    /// the view is absent or bad, the state has no view.
    fn parse(text: &str) -> Resume {
        let value = |key: &str| {
            let mut lines = text.lines().filter_map(|line| line.split_once(' '));
            lines.find_map(|(k, value)| (k == key).then_some(value))
        };
        let parse = |text: &str, count: usize| -> Option<Vec<f64>> {
            let parsed = text.split(' ').map(|v| v.parse::<f64>().ok());
            let floats: Vec<f64> = parsed.collect::<Option<_>>()?;
            (floats.len() == count && floats.iter().all(|v| v.is_finite())).then_some(floats)
        };
        let floats = |key: &str, count: usize| parse(value(key)?, count);
        let view = || {
            let workspace = match value("workspace")? {
                "globe" => Workspace::Globe,
                "atlas" | "map" => Workspace::Atlas,
                _ => return None,
            };
            let world_view = match value("world_view")? {
                "globe" => WorldView::Globe,
                "flat" => WorldView::Flat,
                _ => return None,
            };
            let r = floats("globe_rot", 9)?;
            let rot = Mat3([[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]]);
            let globe_zoom = floats("globe_zoom", 1)?[0];
            let flat_spec = flat_spec(value("flat_projection")?, parse)?;
            let flat_center = floats("flat_center", 2)?;
            let flat_zoom = floats("flat_zoom", 1)?[0];
            is_rotation(&rot).then_some(View {
                workspace,
                world_view,
                globe: GlobeView {
                    rot,
                    zoom: globe_zoom.clamp(view::ZOOM_MIN, view::ZOOM_MAX),
                },
                flat_spec,
                flat_center: Point::new(flat_center[0], flat_center[1]),
                flat_zoom: flat_zoom.clamp(flat::ZOOM_MIN, flat::ZOOM_MAX),
                page: value("page").and_then(|v| v.parse().ok()).map(PageId),
            })
        };
        Resume {
            project: value("project").map(PathBuf::from),
            view: view(),
        }
    }
}

fn numbers(values: &[f64]) -> String {
    let texts: Vec<String> = values.iter().map(f64::to_string).collect();
    texts.join(" ")
}

/// True if the rows of the matrix have the length 1 and are at right angles.
fn is_rotation(rot: &Mat3) -> bool {
    let dot = |a: usize, b: usize| (0..3).map(|i| rot.0[a][i] * rot.0[b][i]).sum::<f64>();
    let pairs = (0..3).flat_map(|a| (0..3).map(move |b| (a, b)));
    pairs
        .into_iter()
        .all(|(a, b)| (dot(a, b) - f64::from(u8::from(a == b))).abs() < 1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        let mut globe = GlobeView::centered(-71.3, 42.125);
        globe.zoom = 3.7;
        View {
            workspace: Workspace::Globe,
            world_view: WorldView::Flat,
            globe,
            flat_spec: ProjectionSpec::new(ProjectionKind::Mercator, 12.5, 0.0),
            flat_center: Point::new(0.3, -0.1),
            flat_zoom: 2.25,
            page: Some(PageId(3)),
        }
    }

    #[test]
    fn the_file_gives_back_the_project_and_the_view() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Resume::read(dir.path()), None);
        let resume = Resume {
            project: Some(dir.path().join("My world.gpkg")),
            view: Some(view()),
        };
        resume.write(dir.path()).unwrap();
        assert_eq!(Resume::read(dir.path()), Some(resume));

        Resume::START_SCREEN.write(dir.path()).unwrap();
        assert_eq!(Resume::read(dir.path()), Some(Resume::START_SCREEN));
    }

    #[test]
    fn a_bad_view_gives_the_project_with_no_view() {
        let resume = Resume {
            project: Some(PathBuf::from("/worlds/a.gpkg")),
            view: Some(view()),
        };
        let text = resume.text();
        let no_view = Resume {
            view: None,
            ..resume.clone()
        };
        let zoom = format!("globe_zoom {}", 3.7);
        assert!(text.contains(&zoom));
        for bad in ["globe_zoom NaN", "globe_zoom", "globe_zoom 1 2"] {
            assert_eq!(Resume::parse(&text.replace(&zoom, bad)), no_view, "{bad}");
        }
        let not_rotation = text.replace("globe_rot ", "globe_rot 2");
        assert_eq!(Resume::parse(&not_rotation), no_view);
        let unknown = format!("{text}\nlater_key 1");
        assert_eq!(Resume::parse(&unknown), resume);
    }

    #[test]
    fn the_atlas_workspace_keeps_its_page_and_reads_the_old_name() {
        let mut view = view();
        view.workspace = Workspace::Atlas;
        let resume = Resume {
            project: Some(PathBuf::from("/worlds/a.gpkg")),
            view: Some(view),
        };
        let text = resume.text();
        assert!(text.contains("workspace atlas") && text.contains("page 3"));
        assert_eq!(Resume::parse(&text), resume);
        let old = text.replace("workspace atlas", "workspace map");
        assert_eq!(Resume::parse(&old), resume);
        let no_page = text.replace("\npage 3", "");
        let parsed = Resume::parse(&no_page).view.unwrap();
        assert_eq!(parsed.page, None);
    }
}

/// Reads the projection of the flat view. A file of an older version has the
/// ID of the kind and the center.
fn flat_spec(
    text: &str,
    parse: impl Fn(&str, usize) -> Option<Vec<f64>>,
) -> Option<ProjectionSpec> {
    if text.starts_with('+') {
        return ProjectionSpec::from_proj(text);
    }
    let (kind, center) = text.split_once(' ')?;
    let center = parse(center, 2)?;
    let kind = ProjectionKind::from_id(kind)?;
    Some(ProjectionSpec::new(kind, center[0], center[1]).normalized())
}
