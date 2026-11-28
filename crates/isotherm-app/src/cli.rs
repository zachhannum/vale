//! Command-line arguments.

use std::path::PathBuf;

use clap::Parser;
use eframe::egui;
use vale_sphere::{ProjectionKind, ProjectionSpec};

use crate::app::{Layout, Workspace};
use crate::globe::WorldView;

#[derive(Parser, Clone, Debug)]
#[command(
    name = "isotherm-app",
    version,
    about = "Isotherm: a climate simulator for invented planets"
)]
pub struct Args {
    /// Open this world file.
    #[arg(long, value_name = "FILE")]
    pub project: Option<PathBuf>,
    /// The workspace that the app opens.
    #[arg(long, value_enum, default_value_t = Workspace::Explore)]
    pub workspace: Workspace,
    /// The arrangement of the controls. The default is pad on iOS and
    /// desktop elsewhere.
    #[arg(long, value_enum)]
    pub layout: Option<Layout>,
    /// The view of the world.
    #[arg(long, value_enum, default_value_t)]
    pub view: WorldView,
    /// The projection of the flat view.
    #[arg(long, value_name = "ID", default_value = "equirectangular", value_parser = parse_projection)]
    pub flat_projection: ProjectionKind,
    /// Write the UI to a PNG and stop.
    #[arg(long, value_name = "FILE")]
    pub screenshot: Option<PathBuf>,
    /// The size of the screenshot in points.
    #[arg(long, value_name = "WxH", default_value = "1440x900", value_parser = parse_size)]
    pub size: (f32, f32),
    /// The safe area of the screenshot, in points. An iPhone 16 in portrait
    /// has 59,0,34,0.
    #[arg(long, value_name = "T,R,B,L", value_parser = parse_safe_area)]
    pub safe_area: Option<egui::SafeAreaInsets>,
}

impl Args {
    /// The arguments of a start with none.
    pub fn none() -> Args {
        Args::parse_from(["isotherm-app"])
    }

    pub fn layout(&self) -> Layout {
        self.layout.unwrap_or_else(Layout::of_platform)
    }

    pub fn flat_spec(&self) -> ProjectionSpec {
        ProjectionSpec::new(self.flat_projection, 0.0, 0.0)
    }
}

fn parse_projection(text: &str) -> Result<ProjectionKind, String> {
    ProjectionKind::from_id(text).ok_or_else(|| {
        let ids: Vec<&str> = ProjectionKind::ALL.iter().map(|k| k.id()).collect();
        format!("unknown projection; use one of {}", ids.join(", "))
    })
}

fn parse_size(text: &str) -> Result<(f32, f32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or("use WxH, for example 1194x834")?;
    let parse = |v: &str| v.trim().parse::<f32>().map_err(|e| e.to_string());
    let (w, h) = (parse(w)?, parse(h)?);
    if !(w >= 1.0 && h >= 1.0 && w.is_finite() && h.is_finite()) {
        return Err(format!("the size must be at least 1x1, got {text}"));
    }
    Ok((w, h))
}

/// The insets of `--safe-area`: the top, the right, the bottom, and the left.
fn parse_safe_area(text: &str) -> Result<egui::SafeAreaInsets, String> {
    let bands: Vec<f32> = text
        .split(',')
        .map(|b| b.trim().parse::<f32>().map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    let &[top, right, bottom, left] = bands.as_slice() else {
        return Err(format!("use four bands, got {text}"));
    };
    if bands.iter().any(|b| !(b.is_finite() && *b >= 0.0)) {
        return Err(format!("each band must be 0 or more, got {text}"));
    }
    Ok(egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left,
        right,
        top,
        bottom,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_workspace_and_layout_parse() {
        let args = Args::try_parse_from([
            "isotherm-app",
            "--project",
            "world.gpkg",
            "--workspace",
            "terrain",
            "--layout",
            "pad",
            "--view",
            "flat",
            "--flat-projection",
            "mercator",
            "--size",
            "390x844",
            "--safe-area",
            "59,0,34,0",
        ])
        .unwrap();
        assert_eq!(args.project, Some(PathBuf::from("world.gpkg")));
        assert_eq!(args.workspace, Workspace::Terrain);
        assert_eq!(args.layout(), Layout::Pad);
        assert_eq!(args.view, WorldView::Flat);
        assert_eq!(args.flat_projection, ProjectionKind::Mercator);
        assert_eq!(args.size, (390.0, 844.0));
        assert_eq!(args.safe_area.unwrap().0.top, 59.0);
    }

    #[test]
    fn each_workspace_has_a_name_on_the_command_line() {
        for (name, workspace) in [
            ("worlds", Workspace::Worlds),
            ("planet", Workspace::Planet),
            ("simulate", Workspace::Simulate),
            ("explore", Workspace::Explore),
            ("export", Workspace::Export),
        ] {
            let args = Args::try_parse_from(["isotherm-app", "--workspace", name]).unwrap();
            assert_eq!(args.workspace, workspace);
        }
    }

    #[test]
    fn the_defaults_and_the_bad_values() {
        let args = Args::none();
        assert_eq!(args.workspace, Workspace::Explore);
        assert_eq!(args.size, (1440.0, 900.0));
        assert_eq!(args.layout(), Layout::of_platform());
        for bad in [
            ["--size", "100"],
            ["--safe-area", "1,2,3"],
            ["--flat-projection", "nope"],
            ["--workspace", "nope"],
        ] {
            let parse = Args::try_parse_from(["isotherm-app", bad[0], bad[1]]);
            assert!(parse.is_err(), "{bad:?}");
        }
    }
}
