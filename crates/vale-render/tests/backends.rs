use std::path::Path;

use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, Options, Placement, PointPlacement,
    TextStyle, place_labels,
};
use vale_render::{
    Affine, BezPath, Color, DisplayList, FillRule, GlyphRun, Item, PdfDocument, Point, Rect,
    RenderError, Symbol, glyph_run_outline, render_pdf, render_pixmap, render_png, render_rgba,
    symbol_items,
};

const RED: Color = Color::rgb(255, 0, 0);
const WHITE: Color = Color::rgb(255, 255, 255);
const BLUE: Color = Color::rgb(0, 0, 255);

fn rect_path(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to((x0, y0));
    p.line_to((x1, y0));
    p.line_to((x1, y1));
    p.line_to((x0, y1));
    p.close_path();
    p
}

fn page_fill(list: &mut DisplayList, color: Color) {
    list.push(Item::Fill {
        path: rect_path(0.0, 0.0, list.width, list.height),
        color,
        rule: FillRule::NonZero,
    });
}

fn px(pm: &vello_cpu::Pixmap, x: u16, y: u16) -> [u8; 4] {
    let p = pm.sample(x, y);
    [p.r, p.g, p.b, p.a]
}

/// A real glyph run for the text `Tokyo`, and its label bounds.
fn tokyo() -> (GlyphRun, Rect) {
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
            text: "Tokyo".into(),
            priority: 0.0,
            geometry: Geometry::Point {
                position: Point::new(100.0, 50.0),
                symbol_radius: 2.0,
            },
        }],
        obstacles: vec![],
        options: Options::default(),
    };
    let out = place_labels(&mut fonts, &input);
    let label = out.placed.first().expect("Tokyo is placed");
    let run = &label.glyph_runs[0];
    (
        GlyphRun {
            font: run.font.clone(),
            font_size: run.font_size,
            normalized_coords: run.normalized_coords.clone(),
            glyphs: run.glyphs.iter().map(|g| (g.id, g.transform)).collect(),
            text: label.text.clone(),
            color: Color::rgb(0, 0, 0),
        },
        label.bounds,
    )
}

#[test]
fn page_fill_and_scale() {
    let mut list = DisplayList::new(100.0, 50.0);
    page_fill(&mut list, RED);
    let pm = render_pixmap(&list, 1.0).unwrap();
    assert_eq!((pm.width(), pm.height()), (100, 50));
    assert_eq!(px(&pm, 10, 10), [255, 0, 0, 255]);
    let pm = render_pixmap(&list, 2.0).unwrap();
    assert_eq!((pm.width(), pm.height()), (200, 100));
}

#[test]
fn square_at_scale_two() {
    let mut list = DisplayList::new(100.0, 50.0);
    page_fill(&mut list, WHITE);
    list.push(Item::Fill {
        path: rect_path(10.0, 10.0, 20.0, 20.0),
        color: BLUE,
        rule: FillRule::NonZero,
    });
    let pm = render_pixmap(&list, 2.0).unwrap();
    assert_eq!(px(&pm, 30, 30), [0, 0, 255, 255]);
    assert_eq!(px(&pm, 50, 50), [255, 255, 255, 255]);
}

#[test]
fn fill_rules() {
    let mut path = rect_path(10.0, 10.0, 50.0, 50.0);
    path.extend(rect_path(20.0, 20.0, 40.0, 40.0).elements().iter().copied());
    let draw = |rule| {
        let mut list = DisplayList::new(60.0, 60.0);
        page_fill(&mut list, WHITE);
        list.push(Item::Fill {
            path: path.clone(),
            color: RED,
            rule,
        });
        render_pixmap(&list, 1.0).unwrap()
    };
    assert_eq!(px(&draw(FillRule::EvenOdd), 30, 30), [255, 255, 255, 255]);
    assert_eq!(px(&draw(FillRule::NonZero), 30, 30), [255, 0, 0, 255]);
}

#[test]
fn round_stroke() {
    let mut list = DisplayList::new(100.0, 50.0);
    page_fill(&mut list, WHITE);
    let mut p = BezPath::new();
    p.move_to((10.0, 25.0));
    p.line_to((90.0, 25.0));
    list.push(Item::Stroke {
        path: p,
        color: BLUE,
        width: 4.0,
        round: true,
    });
    let pm = render_pixmap(&list, 1.0).unwrap();
    assert_eq!(px(&pm, 50, 25), [0, 0, 255, 255]);
    assert_eq!(px(&pm, 50, 35), [255, 255, 255, 255]);
}

#[test]
fn glyph_run_draws_dark_pixels() {
    let (run, bounds) = tokyo();
    assert!(!glyph_run_outline(&run).elements().is_empty());
    let mut list = DisplayList::new(200.0, 100.0);
    page_fill(&mut list, WHITE);
    list.push(Item::Glyphs(run));
    let pm = render_pixmap(&list, 1.0).unwrap();
    let (mut inside, mut outside) = (0, 0);
    for y in 0..100u16 {
        for x in 0..200u16 {
            let p = px(&pm, x, y);
            if p[0] < 128 {
                let (fx, fy) = (f64::from(x), f64::from(y));
                if bounds.inflate(1.0, 1.0).contains(Point::new(fx, fy)) {
                    inside += 1;
                } else if !bounds.inflate(10.0, 10.0).contains(Point::new(fx, fy)) {
                    outside += 1;
                }
            }
        }
    }
    assert!(inside > 30, "dark pixels inside: {inside}");
    assert_eq!(outside, 0);
}

#[test]
fn rgba_and_png() {
    let mut list = DisplayList::new(30.0, 20.0);
    page_fill(&mut list, RED);
    let a = render_rgba(&list, 1.5).unwrap();
    assert_eq!((a.width, a.height), (45, 30));
    assert_eq!(a.data.len(), a.width * a.height * 4);
    assert_eq!(a, render_rgba(&list, 1.5).unwrap());
    let png = render_png(&list, 1.0).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

fn red_blue_png() -> Vec<u8> {
    let mut list = DisplayList::new(2.0, 1.0);
    list.push(Item::Fill {
        path: rect_path(0.0, 0.0, 1.0, 1.0),
        color: RED,
        rule: FillRule::NonZero,
    });
    list.push(Item::Fill {
        path: rect_path(1.0, 0.0, 2.0, 1.0),
        color: BLUE,
        rule: FillRule::NonZero,
    });
    render_png(&list, 1.0).unwrap()
}

#[test]
fn png_image_draws_in_its_page_rectangle() {
    let mut list = DisplayList::new(40.0, 20.0);
    page_fill(&mut list, WHITE);
    list.push(Item::Image {
        png: red_blue_png(),
        rect: Rect::new(10.0, 5.0, 30.0, 15.0),
        interpolate: false,
    });
    let pm = render_pixmap(&list, 1.0).unwrap();
    assert_eq!(px(&pm, 5, 10), [255, 255, 255, 255]);
    assert_eq!(px(&pm, 12, 10), [255, 0, 0, 255]);
    assert_eq!(px(&pm, 28, 10), [0, 0, 255, 255]);
}

#[test]
fn pdf_embeds_a_png_image() {
    let mut list = DisplayList::new(40.0, 20.0);
    list.push(Item::Image {
        png: red_blue_png(),
        rect: Rect::new(10.0, 5.0, 30.0, 15.0),
        interpolate: true,
    });
    let bytes = render_pdf(&list).unwrap();
    let raw = String::from_utf8_lossy(&bytes);
    assert!(raw.contains("/Subtype/Image"), "PDF has no image object");
}

#[test]
fn errors() {
    let list = DisplayList::new(0.0, 10.0);
    assert!(matches!(
        render_pixmap(&list, 1.0),
        Err(RenderError::EmptyPage)
    ));
    let list = DisplayList::new(10.0, 10.0);
    assert!(matches!(
        render_pixmap(&list, 0.0),
        Err(RenderError::EmptyPage)
    ));
    assert!(matches!(
        render_pixmap(&list, f64::NAN),
        Err(RenderError::EmptyPage)
    ));
    let list = DisplayList::new(20000.0, 10.0);
    assert!(matches!(
        render_pixmap(&list, 1.0),
        Err(RenderError::TooLarge { .. })
    ));
}

fn pdf_list(run: GlyphRun) -> DisplayList {
    let mut list = DisplayList::new(200.0, 100.0);
    page_fill(&mut list, Color::rgb(244, 241, 232));
    let mut p = BezPath::new();
    p.move_to((10.0, 90.0));
    p.line_to((190.0, 80.0));
    list.push(Item::Stroke {
        path: p,
        color: Color::rgba(0, 0, 255, 128),
        width: 2.0,
        round: true,
    });
    list.push(Item::Glyphs(run));
    list
}

#[test]
fn pdf_keeps_text() {
    let (run, _) = tokyo();
    let bytes = render_pdf(&pdf_list(run)).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(bytes.len() > 1000);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/app");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test-backends.pdf");
    std::fs::write(&file, &bytes).unwrap();
    if let Ok(out) = std::process::Command::new("pdftotext")
        .arg(&file)
        .arg("-")
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("Tokyo"), "pdftotext printed {text:?}");
    }
}

#[test]
fn pdf_has_pages_of_different_sizes_and_keeps_their_text() {
    let (run, _) = tokyo();
    let first = pdf_list(run.clone());
    let mut second = pdf_list(run);
    second.width = 300.0;
    second.height = 150.0;

    let mut doc = PdfDocument::new();
    doc.add_page(&first).unwrap();
    doc.add_page(&second).unwrap();
    let bytes = doc.finish().unwrap();
    let raw = String::from_utf8_lossy(&bytes);
    assert!(raw.contains("/Count 2"), "{raw}");
    assert!(raw.contains("/MediaBox[0 0 200 100]"), "{raw}");
    assert!(raw.contains("/MediaBox[0 0 300 150]"), "{raw}");

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/app");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("test-multi-page.pdf");
    std::fs::write(&file, &bytes).unwrap();
    if let Ok(out) = std::process::Command::new("pdftotext")
        .arg(&file)
        .arg("-")
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            text.matches("Tokyo").count(),
            2,
            "pdftotext printed {text:?}"
        );
    }
}

#[test]
fn pdf_with_turned_glyphs() {
    let (mut run, _) = tokyo();
    for (_, t) in &mut run.glyphs {
        *t = Affine::rotate(0.3) * *t;
    }
    let bytes = render_pdf(&pdf_list(run)).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn pdf_with_mismatched_text_uses_outline() {
    let (mut run, _) = tokyo();
    run.text = "ab".into();
    run.glyphs.truncate(3);
    let bytes = render_pdf(&pdf_list(run)).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
}

/// A white page, then a red page fill inside an even-odd clip: a square with a square hole.
fn ring_clip_list() -> DisplayList {
    let mut clip = rect_path(10.0, 10.0, 50.0, 50.0);
    clip.extend(rect_path(20.0, 20.0, 40.0, 40.0).elements().iter().copied());
    let mut list = DisplayList::new(60.0, 60.0);
    page_fill(&mut list, WHITE);
    list.push(Item::PushClip {
        path: clip,
        rule: FillRule::EvenOdd,
    });
    page_fill(&mut list, RED);
    list.push(Item::PopClip);
    list
}

#[test]
fn an_even_odd_clip_leaves_the_hole_empty() {
    let pm = render_pixmap(&ring_clip_list(), 1.0).unwrap();
    assert_eq!(px(&pm, 30, 30), [255, 255, 255, 255]);
    assert_eq!(px(&pm, 5, 5), [255, 255, 255, 255]);
    assert_eq!(px(&pm, 15, 30), [255, 0, 0, 255]);
}

#[test]
fn a_fill_after_pop_clip_is_not_clipped() {
    let mut list = ring_clip_list();
    list.push(Item::PopClip);
    page_fill(&mut list, BLUE);
    let pm = render_pixmap(&list, 1.0).unwrap();
    assert_eq!(px(&pm, 5, 5), [0, 0, 255, 255]);
}

#[test]
fn pdf_accepts_clips() {
    let mut list = ring_clip_list();
    list.push(Item::PushClip {
        path: BezPath::new(),
        rule: FillRule::NonZero,
    });
    page_fill(&mut list, BLUE);
    list.push(Item::PushClip {
        path: rect_path(0.0, 0.0, 10.0, 10.0),
        rule: FillRule::NonZero,
    });
    let bytes = render_pdf(&list).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
}

/// Brush dabs inside a clip, as a textured stroke draws them.
fn textured_items() -> Vec<Item> {
    use kurbo::Shape;
    let line = vec![vec![Point::new(5.0, 10.0), Point::new(45.0, 30.0)]];
    let mut items = vec![Item::PushClip {
        path: rect_path(0.0, 0.0, 40.0, 40.0),
        rule: FillRule::NonZero,
    }];
    for dab in vale_render::stroke::brush_dabs(&line, 4.0, 3) {
        items.push(Item::Fill {
            path: kurbo::Circle::new(dab.center, dab.radius).to_path(0.05),
            color: Color::rgba(40, 60, 90, (200.0 * dab.alpha) as u8),
            rule: FillRule::NonZero,
        });
    }
    items.push(Item::PopClip);
    items
}

#[test]
fn rasterize_draws_like_its_items() {
    let mut direct = DisplayList::new(50.0, 40.0);
    page_fill(&mut direct, WHITE);
    let mut wrapped = direct.clone();
    direct.items.extend(textured_items());
    wrapped.push(Item::Rasterize {
        bounds: Rect::new(0.0, 0.0, 50.0, 40.0),
        items: textured_items(),
    });
    // An item after the group is not clipped by the clip inside it.
    let after = Item::Fill {
        path: rect_path(44.0, 0.0, 50.0, 6.0),
        color: RED,
        rule: FillRule::NonZero,
    };
    direct.push(after.clone());
    wrapped.push(after);
    let a = render_rgba(&direct, 2.0).unwrap();
    let b = render_rgba(&wrapped, 2.0).unwrap();
    assert_eq!(a, b);
    let pm = render_pixmap(&wrapped, 1.0).unwrap();
    assert_eq!(px(&pm, 47, 3), [255, 0, 0, 255]);
}

#[test]
fn pdf_turns_rasterize_into_an_image() {
    let mut list = DisplayList::new(50.0, 40.0);
    list.push(Item::Rasterize {
        bounds: Rect::new(-10.0, -10.0, 60.0, 60.0),
        items: textured_items(),
    });
    let bytes = render_pdf(&list).unwrap();
    let raw = String::from_utf8_lossy(&bytes);
    assert!(raw.contains("/Subtype/Image"), "PDF has no image object");
}

#[test]
fn pdf_keeps_strokes_vector() {
    let mut list = DisplayList::new(50.0, 40.0);
    let mut p = BezPath::new();
    p.move_to((5.0, 10.0));
    p.line_to((45.0, 30.0));
    list.push(Item::Stroke {
        path: p,
        color: BLUE,
        width: 2.0,
        round: true,
    });
    let bytes = render_pdf(&list).unwrap();
    let raw = String::from_utf8_lossy(&bytes);
    assert!(
        !raw.contains("/Subtype/Image"),
        "a plain stroke became an image"
    );
}

#[test]
fn pdf_keeps_symbols_vector() {
    let mut list = DisplayList::new(80.0, 40.0);
    for (i, name) in ["tree", "marsh", "city"].into_iter().enumerate() {
        let path = format!("{}/tests/data/{name}.svg", env!("CARGO_MANIFEST_DIR"));
        let symbol = Symbol::from_svg(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let at = Affine::translate((15.0 + 25.0 * i as f64, 20.0)) * Affine::scale(20.0);
        list.items.extend(symbol_items(&symbol, &[at], None));
    }
    let bytes = render_pdf(&list).unwrap();
    let raw = String::from_utf8_lossy(&bytes);
    assert!(!raw.contains("/Subtype/Image"), "a symbol became an image");
}
