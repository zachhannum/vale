use clap::Parser;
use kurbo::Point;
use vale_app::cli::{Args, apply_view};
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
    assert_eq!(
        a.document().unwrap(),
        vale_app::document::Document::sample()
    );
}

#[test]
fn center_and_projection() {
    let a = parse(&["--center", "-100,40", "--projection", "orthographic"]);
    let spec = a.document().unwrap().frame.projection;
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
}
