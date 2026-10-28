use clap::Parser;
use kurbo::Point;
use vale_app::cli::{Args, apply_view};
use vale_app::document::Document;
use vale_app::pipeline::Pipeline;
use vale_sphere::ProjectionKind;

fn parse(args: &[&str]) -> Args {
    let mut v = vec!["vale-app"];
    v.extend_from_slice(args);
    Args::try_parse_from(v).unwrap()
}

#[test]
fn defaults() {
    let a = parse(&[]);
    assert_eq!(a.size().unwrap(), (1280.0, 800.0));
    // Each sample has new feature UUIDs, so the projects are not equal.
    let (doc, sample) = (a.document().unwrap(), Document::sample());
    assert_eq!(doc.frame, sample.frame);
    assert_eq!(doc.project.world, sample.project.world);
    let shape = |d: &Document| -> Vec<(String, usize)> {
        let layers = d.project.layers().iter();
        layers.map(|l| (l.name.clone(), l.features.len())).collect()
    };
    assert_eq!(shape(&doc), shape(&sample));
}

#[test]
fn center_and_projection() {
    let a = parse(&["--center", "-100,40", "--projection", "orthographic"]);
    let doc = a.document().unwrap();
    let spec = doc.projection(doc.first_map());
    assert_eq!(spec.kind, ProjectionKind::Orthographic);
    assert_eq!((spec.lon0, spec.lat0), (-100.0, 40.0));
}

#[test]
fn bad_names() {
    let e = parse(&["--projection", "nope"]).document().unwrap_err();
    assert!(e.to_string().contains("equal-earth"));
    let e = parse(&["--hide", "Nope"]).document().unwrap_err();
    assert!(e.to_string().contains("Land"));
}

#[test]
fn sizes() {
    assert_eq!(
        parse(&["--size", "800x600"]).size().unwrap(),
        (800.0, 600.0)
    );
    assert!(parse(&["--size", "800"]).size().is_err());
}

#[test]
fn safe_areas() {
    let insets = parse(&["--safe-area", "59,0,34,0"])
        .safe_area()
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(
        (insets.top, insets.right, insets.bottom, insets.left),
        (59.0, 0.0, 34.0, 0.0)
    );
    assert!(parse(&[]).safe_area().unwrap().is_none());
    assert!(parse(&["--safe-area", "59,0,34"]).safe_area().is_err());
    assert!(parse(&["--safe-area", "59,0,-34,0"]).safe_area().is_err());
}

#[test]
fn look_at_and_zoom() {
    let a = parse(&["--look-at", "139.7,35.7", "--zoom", "4"]);
    let size = a.size().unwrap();
    let mut doc = a.document().unwrap();
    let mut p = Pipeline::new();
    let fit = p.fit_scale(&doc, size).unwrap();
    apply_view(&a, &mut doc, &mut p, size).unwrap();
    let q = p
        .lonlat_to_page(&doc, size, [139.7, 35.7])
        .unwrap()
        .unwrap();
    assert!((q - Point::new(640.0, 400.0)).hypot() < 1e-6);
    let scale = doc.frame.view.unwrap().scale;
    assert!((scale / (4.0 * fit) - 1.0).abs() < 1e-12);
}

#[test]
fn the_layout_and_the_panels_come_from_the_flags() {
    use vale_app::ui::pad::Panel;
    use vale_app::ui::{Layout, Workspace};
    let args = Args::parse_from(["vale-app"]);
    assert_eq!(args.layout, Layout::Desktop);
    assert!(args.panel.is_empty());
    let flags = ["--layout", "pad", "--panel", "brush", "--panel", "layers"];
    let args = Args::parse_from(["vale-app"].into_iter().chain(flags));
    assert_eq!(args.workspace, Workspace::Globe);
    assert_eq!(args.layout, Layout::Pad);
    assert_eq!(args.panel, vec![Panel::Brush, Panel::Layers]);
    assert_eq!(args.world_form, None);
    let args = Args::parse_from(["vale-app", "--world-form", "new"]);
    assert_eq!(
        args.world_form,
        Some(vale_app::ui::world_form::FormMode::New)
    );
}

#[test]
fn the_export_of_the_heightmap_needs_a_project() {
    let args = parse(&["--project", "w.gpkg", "--export-heightmap", "w.tif"]);
    assert_eq!(args.export_heightmap, Some("w.tif".into()));
    assert_eq!(parse(&[]).export_heightmap, None);
    let error = Args::try_parse_from(["vale-app", "--export-heightmap", "w.tif"]).unwrap_err();
    assert_eq!(
        error.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    assert!(error.to_string().contains("--project"), "{error}");
}

#[test]
fn atlas_exports_take_a_project_and_a_one_based_page() {
    let atlas = parse(&["--project", "w.gpkg", "--export-atlas", "atlas.pdf"]);
    assert_eq!(atlas.export_atlas, Some("atlas.pdf".into()));

    let page = parse(&["--project", "w.gpkg", "--export-page", "2", "page.pdf"]);
    assert_eq!(page.page_export().unwrap(), Some((2, "page.pdf".into())));
    let zero = parse(&["--project", "w.gpkg", "--export-page", "0", "page.png"]);
    assert!(
        zero.page_export()
            .unwrap_err()
            .to_string()
            .contains("above zero")
    );

    for flags in [
        vec!["--export-atlas", "atlas.pdf"],
        vec!["--export-page", "1", "page.pdf"],
    ] {
        let args = std::iter::once("vale-app").chain(flags);
        let error = Args::try_parse_from(args).unwrap_err();
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert!(error.to_string().contains("--project"), "{error}");
    }
}
