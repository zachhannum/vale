use kurbo::Shape;
use vale_render::{
    Affine, BUILTIN_SYMBOLS, Color, FillRule, Item, Rect, Symbol, SymbolError, Symbols,
    symbol_items,
};

fn bounds(symbol: &Symbol) -> Rect {
    symbol
        .paths()
        .iter()
        .map(|p| p.path.bounding_box())
        .reduce(|a, b| a.union(b))
        .unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn builtins_parse_and_are_normalized() {
    let lib = Symbols::builtin();
    assert_eq!(lib.names(), BUILTIN_SYMBOLS.to_vec());
    for name in BUILTIN_SYMBOLS {
        assert!(Symbols::is_builtin(name));
        let s = lib.get(name).unwrap();
        let b = bounds(s);
        assert!(close(b.width().max(b.height()), 1.0), "{name}: {b:?}");
        assert!(
            close(b.center().x, 0.0) && close(b.center().y, 0.0),
            "{name}: {b:?}"
        );
    }
    assert!(!Symbols::is_builtin("mine"));
}

#[test]
fn user_svg_with_style_transform_rect_and_circle() {
    // In SVG units the rect spans 10..30 by 10..20, and the circle 0..20 by 10..30.
    // Together they span 0..30 by 10..30: 30 wide and 20 high.
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 40">
        <g transform="translate(10 10)">
            <rect x="0" y="0" width="20" height="10" style="fill:#336699"/>
        </g>
        <circle cx="10" cy="20" r="10" fill="#336699" fill-opacity="0.5"/>
    </svg>"##;
    let s = Symbol::from_svg(svg).unwrap();
    assert_eq!(s.paths().len(), 2);
    let b = bounds(&s);
    assert!(close(b.x0, -0.5) && close(b.x1, 0.5), "{b:?}");
    assert!(close(b.y0, -1.0 / 3.0) && close(b.y1, 1.0 / 3.0), "{b:?}");
    let rect = s.paths()[0].path.bounding_box();
    assert!(
        close(rect.x0, -1.0 / 6.0) && close(rect.y0, -1.0 / 3.0),
        "{rect:?}"
    );
    assert_eq!(
        s.paths()[0].fill,
        Some((Color::rgb(0x33, 0x66, 0x99), FillRule::NonZero))
    );
    assert_eq!(
        s.paths()[1].fill,
        Some((Color::rgba(0x33, 0x66, 0x99, 128), FillRule::NonZero))
    );
    assert_eq!(s.paths()[0].stroke, None);
}

#[test]
fn group_opacity_gradient_and_stroke_scale() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
        <defs><linearGradient id="g"><stop offset="0" stop-color="#ff0000"/>
            <stop offset="1" stop-color="#0000ff"/></linearGradient></defs>
        <g opacity="0.5" transform="scale(2)">
            <path d="M0 0H5V5H0Z" fill="url(#g)" fill-rule="evenodd" stroke="#00ff00" stroke-width="1"/>
        </g>
    </svg>"##;
    let s = Symbol::from_svg(svg).unwrap();
    let p = &s.paths()[0];
    assert_eq!(
        p.fill,
        Some((Color::rgba(255, 0, 0, 128), FillRule::EvenOdd))
    );
    // A width of 1 times the scale 2, over the side 10.
    let (color, width) = p.stroke.unwrap();
    assert_eq!(color, Color::rgba(0, 255, 0, 128));
    assert!(close(width, 0.2), "{width}");
}

#[test]
fn a_tint_replaces_colors() {
    let s = Symbols::builtin().get("mountain").unwrap();
    let tint = Color::rgba(10, 120, 30, 128);
    let items = symbol_items(s, &[Affine::scale(20.0)], Some(tint));
    assert_eq!(items.len(), s.paths().len());
    for (item, path) in items.iter().zip(s.paths()) {
        let Item::Fill { color, .. } = item else {
            panic!("not a fill: {item:?}");
        };
        let a = path.fill.unwrap().0.a;
        let expect = ((128 * u16::from(a) + 127) / 255) as u8;
        assert_eq!(*color, Color::rgba(10, 120, 30, expect));
    }
}

#[test]
fn stroke_width_scales_with_the_instance() {
    let s = Symbols::builtin().get("marsh").unwrap();
    let width = s.paths()[0].stroke.unwrap().1;
    let a = Affine::translate((50.0, 50.0)) * Affine::rotate(0.7) * Affine::scale(30.0);
    let items = symbol_items(s, &[a], None);
    let Item::Stroke {
        width: w, round, ..
    } = &items[0]
    else {
        panic!("not a stroke: {:?}", items[0]);
    };
    assert!(close(*w, width * 30.0), "{w}");
    assert!(*round);
}

#[test]
fn many_instances_merge_into_one_item_per_path() {
    let lib = Symbols::builtin();
    let instances: Vec<Affine> = (0..10_000)
        .map(|i| Affine::translate(((i % 100) as f64 * 10.0, (i / 100) as f64 * 10.0)))
        .map(|t| t * Affine::scale(8.0))
        .collect();
    for name in ["tree", "marsh", "city"] {
        let s = lib.get(name).unwrap();
        let per_path: usize = s
            .paths()
            .iter()
            .map(|p| usize::from(p.fill.is_some()) + usize::from(p.stroke.is_some()))
            .sum();
        let items = symbol_items(s, &instances, None);
        assert_eq!(items.len(), per_path, "{name}");
    }
    assert!(symbol_items(lib.get("tree").unwrap(), &[], None).is_empty());
}

#[test]
fn many_instances_cover_all_of_them() {
    let s = Symbols::builtin().get("tree").unwrap();
    let instances = [
        Affine::translate((10.0, 10.0)) * Affine::scale(8.0),
        Affine::translate((90.0, 50.0)) * Affine::scale(8.0),
    ];
    let items = symbol_items(s, &instances, None);
    let Item::Fill { path, .. } = &items[0] else {
        panic!("not a fill");
    };
    let b = path.bounding_box();
    assert!(b.x0 < 10.0 && b.x1 > 90.0, "{b:?}");
}

#[test]
fn library_insert_get_and_names() {
    let mut lib = Symbols::with_builtins();
    let dot = Symbol::from_svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4"><circle cx="2" cy="2" r="2"/></svg>"#,
    )
    .unwrap();
    lib.insert("dot", dot.clone());
    assert_eq!(lib.names().len(), 10);
    assert_eq!(lib.names()[9], "dot");
    assert_eq!(**lib.get("dot").unwrap(), dot);
    assert!(lib.get("nothing").is_none());
    assert!(Symbols::builtin().get("dot").is_none());
}

#[test]
fn text_only_svg_is_an_error() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><text x="1" y="8">A</text></svg>"#;
    assert_eq!(Symbol::from_svg(svg), Err(SymbolError::Empty));
    assert!(matches!(
        Symbol::from_svg("not svg"),
        Err(SymbolError::Parse(_))
    ));
}
