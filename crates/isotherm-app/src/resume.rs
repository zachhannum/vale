//! The view of a world that the next start of the app puts back.

use vale_globe::flat;
use vale_globe::math::Mat3;
use vale_globe::view::{self, GlobeView};
use vale_shell::resume::{Lines, ResumeView, numbers};
use vale_sphere::{Point, ProjectionSpec};

use crate::app::Workspace;
use crate::globe::WorldView;

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
}

/// The state of Isotherm for its next start.
pub type Resume = vale_shell::resume::Resume<View>;

impl ResumeView for View {
    fn lines(&self, lines: &mut Vec<String>) {
        let world_view = match self.world_view {
            WorldView::Globe => "globe",
            WorldView::Flat => "flat",
        };
        let rot: Vec<f64> = self.globe.rot.0.iter().flatten().copied().collect();
        let center = self.flat_center;
        lines.push(format!(
            "workspace {}",
            self.workspace.name().to_lowercase()
        ));
        lines.push(format!("world_view {world_view}"));
        lines.push(format!("globe_rot {}", numbers(&rot)));
        lines.push(format!("globe_zoom {}", self.globe.zoom));
        lines.push(format!("flat_projection {}", self.flat_spec.to_proj()));
        lines.push(format!("flat_center {}", numbers(&[center.x, center.y])));
        lines.push(format!("flat_zoom {}", self.flat_zoom));
    }

    fn parse(lines: &Lines) -> Option<View> {
        let value = |key: &str| lines.value(key);
        let workspace = value("workspace")?;
        let workspace = Workspace::ALL
            .into_iter()
            .find(|w| w.name().eq_ignore_ascii_case(workspace))?;
        let world_view = match value("world_view")? {
            "globe" => WorldView::Globe,
            "flat" => WorldView::Flat,
            _ => return None,
        };
        let r = lines.floats("globe_rot", 9)?;
        let rot = Mat3([[r[0], r[1], r[2]], [r[3], r[4], r[5]], [r[6], r[7], r[8]]]);
        let globe_zoom = lines.floats("globe_zoom", 1)?[0];
        let flat_spec = ProjectionSpec::from_proj(value("flat_projection")?)?;
        let flat_center = lines.floats("flat_center", 2)?;
        let flat_zoom = lines.floats("flat_zoom", 1)?[0];
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
        })
    }
}

/// True if the rows of the matrix have the length 1 and are at right angles.
fn is_rotation(rot: &Mat3) -> bool {
    let dot = |a: usize, b: usize| (0..3).map(|i| rot.0[a][i] * rot.0[b][i]).sum::<f64>();
    (0..3)
        .flat_map(|a| (0..3).map(move |b| (a, b)))
        .all(|(a, b)| (dot(a, b) - f64::from(u8::from(a == b))).abs() < 1e-6)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use vale_sphere::ProjectionKind;

    use super::*;

    pub(crate) fn view() -> View {
        let mut globe = GlobeView::centered(-71.3, 42.125);
        globe.zoom = 3.7;
        View {
            workspace: Workspace::Planet,
            world_view: WorldView::Flat,
            globe,
            flat_spec: ProjectionSpec::new(ProjectionKind::Mercator, 12.5, 0.0),
            flat_center: Point::new(0.3, -0.1),
            flat_zoom: 2.25,
        }
    }

    #[test]
    fn the_file_gives_back_the_world_and_the_view() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Resume::read(dir.path()), None);
        let resume = Resume {
            project: Some(PathBuf::from("/worlds/Aster.gpkg")),
            view: Some(view()),
        };
        resume.write(dir.path()).unwrap();
        let back = Resume::read(dir.path()).unwrap();
        assert_eq!(back.project, resume.project);
        let got = back.view.unwrap();
        assert_eq!(got.workspace, Workspace::Planet);
        assert_eq!(got.world_view, WorldView::Flat);
        assert_eq!(got.flat_center, Point::new(0.3, -0.1));
        assert_eq!(got.flat_zoom, 2.25);
        assert_eq!(got.globe.zoom, 3.7);
        for (a, b) in got
            .globe
            .rot
            .0
            .iter()
            .flatten()
            .zip(view().globe.rot.0.iter().flatten())
        {
            assert!((a - b).abs() < 1e-12);
        }
        assert_eq!(got.flat_spec.kind, ProjectionKind::Mercator);
    }

    #[test]
    fn a_matrix_that_is_not_a_rotation_gives_no_view() {
        let mut text = Resume {
            project: Some(PathBuf::from("/w.gpkg")),
            view: Some(view()),
        }
        .text();
        let start = text.find("globe_rot ").unwrap();
        let end = start + text[start..].find('\n').unwrap();
        text.replace_range(start..end, "globe_rot 2 0 0 0 1 0 0 0 1");
        let back = Resume::parse(&text);
        assert_eq!(back.project, Some(PathBuf::from("/w.gpkg")));
        assert_eq!(back.view, None);
    }
}
