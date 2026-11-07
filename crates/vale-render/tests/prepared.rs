//! Regions of a prepared list against the full page.

use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, Options, Placement, PointPlacement,
    TextStyle, place_labels,
};
use vale_render::{
    Affine, BezPath, Color, DisplayList, FillRule, GlyphRun, Item, Point, PreparedList, Rect, Rgba,
    render_png, render_region, render_rgba,
};

fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x0, y0));
    p.line_to((x1, y0));
    p.line_to((x1, y1));
    p.line_to((x0, y1));
    p.close_path();
    p
}

fn fill(path: BezPath, color: Color) -> Item {
    Item::Fill {
        path,
        color,
        rule: FillRule::NonZero,
    }
}

/// A glyph run for the text `Vale` at `position`.
fn glyphs(position: Point) -> GlyphRun {
    let mut fonts = Fonts::new();
    fonts
        .register(
            std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/NotoSans-Regular.ttf"
            ))
            .unwrap(),
        )
        .unwrap();
    let input = LabelInput {
        bounds: Rect::new(0.0, 0.0, 200.0, 100.0),
        classes: vec![LabelClass {
            name: "p".into(),
            style: TextStyle::new("Noto Sans", 20.0),
            priority: 0,
            margin: 0.0,
            placement: Placement::Point(PointPlacement::default()),
            duplicate_distance: None,
        }],
        features: vec![Feature {
            id: 1,
            class: 0,
            text: "Vale".into(),
            priority: 0.0,
            geometry: Geometry::Point {
                position,
                symbol_radius: 2.0,
            },
        }],
        obstacles: vec![],
        parallels: Vec::new(),
        options: Options::default(),
    };
    let out = place_labels(&mut fonts, &input);
    let label = out.placed.first().expect("the label is placed");
    let run = &label.glyph_runs[0];
    GlyphRun {
        font: run.font.clone(),
        font_size: run.font_size,
        normalized_coords: run.normalized_coords.clone(),
        glyphs: run.glyphs.iter().map(|g| (g.id, g.transform)).collect(),
        text: label.text.clone(),
        color: Color::rgb(20, 20, 20),
    }
}

fn checker_png() -> Vec<u8> {
    let mut list = DisplayList::new(2.0, 2.0);
    list.push(fill(rect_path(0.0, 0.0, 1.0, 1.0), Color::rgb(255, 0, 0)));
    list.push(fill(
        rect_path(1.0, 1.0, 2.0, 2.0),
        Color::rgba(0, 0, 255, 128),
    ));
    render_png(&list, 1.0).unwrap()
}

/// A page with each kind of item. The glyph run, an antialiased clip edge, and a thick
/// stroke cross the line x = 50.
fn sampler() -> DisplayList {
    let mut list = DisplayList::new(200.0, 100.0);
    list.push(Item::PopClip);
    list.push(fill(
        rect_path(0.0, 0.0, 200.0, 100.0),
        Color::rgb(244, 241, 232),
    ));
    let mut ring = rect_path(10.3, 10.3, 90.0, 90.0);
    ring.extend(rect_path(30.0, 30.0, 70.0, 70.0));
    list.push(Item::PushClip {
        path: ring,
        rule: FillRule::EvenOdd,
    });
    list.push(fill(
        rect_path(0.0, 0.0, 200.0, 100.0),
        Color::rgb(60, 120, 90),
    ));
    // A clip edge at x = 50.2, which is not on a pixel edge.
    list.push(Item::PushClip {
        path: rect_path(0.0, 0.0, 50.2, 100.0),
        rule: FillRule::NonZero,
    });
    list.push(fill(
        rect_path(0.0, 40.0, 200.0, 60.0),
        Color::rgba(200, 40, 40, 160),
    ));
    list.push(Item::PopClip);
    list.push(Item::PopClip);
    // A thick stroke whose join reaches past the line x = 50.
    let mut join = BezPath::new();
    join.move_to((18.0, 60.0));
    join.line_to((48.0, 80.0));
    join.line_to((18.0, 100.0));
    list.push(Item::Stroke {
        path: join,
        color: Color::rgb(160, 30, 30),
        width: 8.0,
        round: false,
    });
    let mut line = BezPath::new();
    line.move_to((100.0, 20.0));
    line.line_to((140.0, 60.0));
    line.line_to((190.0, 15.0));
    list.push(Item::Stroke {
        path: line.clone(),
        color: Color::rgb(30, 60, 160),
        width: 6.0,
        round: true,
    });
    list.push(Item::Layer {
        alpha: 0.5,
        items: vec![
            fill(rect_path(120.0, 70.0, 160.0, 95.0), Color::rgb(0, 0, 255)),
            fill(rect_path(140.0, 75.0, 180.0, 98.0), Color::rgb(0, 0, 255)),
        ],
    });
    list.push(Item::Rasterize {
        bounds: Rect::new(100.0, 0.0, 200.0, 100.0),
        items: vec![Item::Stroke {
            path: line,
            color: Color::rgba(255, 255, 255, 200),
            width: 1.5,
            round: false,
        }],
    });
    list.push(Item::Image {
        png: checker_png(),
        rect: Rect::new(150.0, 60.0, 190.0, 100.0),
        interpolate: false,
    });
    list.push(Item::Glyphs(glyphs(Point::new(50.0, 25.0))));
    // A clip that stays open to the end.
    list.push(Item::PushClip {
        path: rect_path(0.0, 0.0, 30.0, 30.0),
        rule: FillRule::NonZero,
    });
    list.push(fill(
        rect_path(0.0, 0.0, 200.0, 100.0),
        Color::rgba(255, 200, 0, 100),
    ));
    list
}

/// The part of `image` at `x`, `y` with the size `w` by `h`.
fn crop(image: &Rgba, x: usize, y: usize, w: usize, h: usize) -> Rgba {
    let data = (y..y + h)
        .flat_map(|row| &image.data[(row * image.width + x) * 4..][..w * 4])
        .copied()
        .collect();
    Rgba {
        width: w,
        height: h,
        data,
    }
}

/// The largest difference of a channel.
fn most(a: &Rgba, b: &Rgba) -> u8 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    a.data
        .iter()
        .zip(&b.data)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

#[test]
fn a_region_is_the_crop_of_the_page() {
    let list = sampler();
    let prepared = PreparedList::new(&list).unwrap();
    let scale = 2.0;
    let full = render_rgba(&list, scale).unwrap();
    let (x, y, w, h) = (37, 21, 150, 90);
    let transform = Affine::translate((-(x as f64), -(y as f64))) * Affine::scale(scale);
    let (region, drawn) = render_region(&prepared, transform, w as u32, h as u32).unwrap();
    assert_eq!(drawn, prepared.visible_count(transform, w as u32, h as u32));
    let d = most(&region, &crop(&full, x, y, w, h));
    assert!(d <= 1, "a channel differs by {d}");
}

#[test]
fn two_regions_side_by_side_are_the_page() {
    let list = sampler();
    let prepared = PreparedList::new(&list).unwrap();
    let scale = 2.0;
    let full = render_rgba(&list, scale).unwrap();
    // The seam is at x = 100 pixels, which is x = 50 on the page.
    assert_eq!((full.width, full.height), (400, 200));
    for (x, w) in [(0, 100), (100, 300)] {
        let transform = Affine::translate((-(x as f64), 0.0)) * Affine::scale(scale);
        let (region, _) = render_region(&prepared, transform, w as u32, 200).unwrap();
        let d = most(&region, &crop(&full, x, 0, w, 200));
        assert!(d <= 1, "a channel of the region at {x} differs by {d}");
    }
}

#[test]
fn a_region_skips_the_items_outside_it() {
    // A page that is too large to draw whole.
    let mut list = DisplayList::new(100_000.0, 100_000.0);
    for i in 0..1000 {
        let x = f64::from(i) * 100.0;
        list.push(fill(
            rect_path(x, x, x + 50.0, x + 50.0),
            Color::rgb(0, 0, 255),
        ));
    }
    assert!(render_rgba(&list, 1.0).is_err());
    let prepared = PreparedList::new(&list).unwrap();
    assert_eq!(prepared.bounds(), Rect::new(0.0, 0.0, 99_950.0, 99_950.0));
    // A region of 256 pixels around the page point (50 000, 50 000) at 1 pixel per point.
    let transform = Affine::translate((-50_000.0, -50_000.0));
    let (region, drawn) = render_region(&prepared, transform, 256, 256).unwrap();
    assert_eq!(drawn, 3);
    assert_eq!(prepared.visible_count(transform, 256, 256), 3);
    // The square at 50 000 fills the first pixel.
    assert_eq!(&region.data[..4], &[0, 0, 255, 255]);
    // A region far from the page draws nothing.
    let away = Affine::translate((-200_000.0, 0.0));
    let (empty, drawn) = render_region(&prepared, away, 64, 64).unwrap();
    assert_eq!(drawn, 0);
    assert!(empty.data.iter().all(|&c| c == 0));
}

#[test]
fn a_region_inside_a_stroked_rectangle_skips_the_stroke() {
    let mut list = DisplayList::new(1000.0, 1000.0);
    list.push(Item::Stroke {
        path: rect_path(0.0, 0.0, 1000.0, 1000.0),
        color: Color::rgb(0, 0, 0),
        width: 4.0,
        round: false,
    });
    let prepared = PreparedList::new(&list).unwrap();
    let middle = Affine::translate((-400.0, -400.0));
    assert_eq!(prepared.visible_count(middle, 200, 200), 0);
    let (region, drawn) = render_region(&prepared, middle, 200, 200).unwrap();
    assert_eq!(drawn, 0);
    assert!(region.data.iter().all(|&c| c == 0));
    // A region on the edge draws the stroke.
    let edge = Affine::translate((-400.0, 100.0));
    assert_eq!(prepared.visible_count(edge, 200, 200), 1);
}

#[test]
fn a_culled_clip_skips_its_items() {
    let mut list = DisplayList::new(1000.0, 1000.0);
    list.push(Item::PushClip {
        path: rect_path(900.0, 900.0, 1000.0, 1000.0),
        rule: FillRule::NonZero,
    });
    for _ in 0..10 {
        list.push(fill(
            rect_path(0.0, 0.0, 1000.0, 1000.0),
            Color::rgb(0, 0, 255),
        ));
    }
    list.push(Item::PopClip);
    let prepared = PreparedList::new(&list).unwrap();
    assert_eq!(prepared.visible_count(Affine::IDENTITY, 100, 100), 0);
    let corner = Affine::translate((-900.0, -900.0));
    assert_eq!(prepared.visible_count(corner, 100, 100), 10);
}
