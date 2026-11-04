//! The GPU Vello backend against the `vello_cpu` backend.

use std::sync::{Mutex, OnceLock};

use vale_labeler::{
    Feature, Fonts, Geometry, LabelClass, LabelInput, Options, Placement, PointPlacement,
    TextStyle, place_labels,
};
use vale_render::gpu::{GpuRenderer, Limits, read_rgba};
use vale_render::{
    BezPath, Color, DisplayList, FillRule, GlyphRun, Item, Point, Rect, RenderError, Rgba,
    render_png, render_rgba,
};

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Mutex<GpuRenderer>,
}

/// The device for all tests. A software adapter is good enough.
fn gpu() -> &'static Gpu {
    static GPU: OnceLock<Gpu> = OnceLock::new();
    GPU.get_or_init(|| {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let request = |force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                force_fallback_adapter,
                ..Default::default()
            }))
        };
        let adapter = request(false)
            .or_else(|_| request(true))
            .expect("the GPU tests need a wgpu adapter, and this machine has none");
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("the adapter gives a device");
        let renderer = Mutex::new(GpuRenderer::new(&device).expect("Vello starts"));
        Gpu {
            device,
            queue,
            renderer,
        }
    })
}

/// Draws the list on the GPU, and returns the pixels and the number of batches.
fn render(list: &DisplayList, scale: f64, limits: Limits) -> (Rgba, usize) {
    let gpu = gpu();
    let mut renderer = gpu.renderer.lock().unwrap();
    renderer.set_limits(limits);
    let target = GpuRenderer::target(&gpu.device, list, scale).unwrap();
    let batches = renderer
        .render(&gpu.device, &gpu.queue, list, scale, &target)
        .unwrap();
    renderer.set_limits(Limits::default());
    (
        read_rgba(&gpu.device, &gpu.queue, &target).unwrap(),
        batches,
    )
}

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

/// A glyph run for the text `Vale`.
fn glyphs() -> GlyphRun {
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
                position: Point::new(100.0, 70.0),
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

/// A page with each kind of item.
fn sampler() -> DisplayList {
    let mut list = DisplayList::new(200.0, 100.0);
    list.push(fill(
        rect_path(0.0, 0.0, 200.0, 100.0),
        Color::rgb(244, 241, 232),
    ));
    let mut ring = rect_path(10.0, 10.0, 90.0, 90.0);
    ring.extend(rect_path(30.0, 30.0, 70.0, 70.0));
    list.push(Item::PushClip {
        path: ring,
        rule: FillRule::EvenOdd,
    });
    list.push(fill(
        rect_path(0.0, 0.0, 200.0, 100.0),
        Color::rgb(60, 120, 90),
    ));
    list.push(Item::PushClip {
        path: rect_path(0.0, 0.0, 50.0, 100.0),
        rule: FillRule::NonZero,
    });
    list.push(fill(
        rect_path(0.0, 40.0, 200.0, 60.0),
        Color::rgba(200, 40, 40, 160),
    ));
    list.push(Item::PopClip);
    list.push(Item::PopClip);
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
    list.push(Item::Glyphs(glyphs()));
    list
}

/// The largest difference of a channel, and the part of the channels that differ by more than
/// `tolerance`.
fn compare(a: &Rgba, b: &Rgba, tolerance: u8) -> (u8, f64) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let mut most = 0;
    let mut over = 0;
    for (x, y) in a.data.iter().zip(&b.data) {
        let d = x.abs_diff(*y);
        most = most.max(d);
        if d > tolerance {
            over += 1;
        }
    }
    (most, over as f64 / a.data.len() as f64)
}

#[test]
#[ignore = "gpu"]
fn gpu_pixels_match_cpu_pixels() {
    let list = sampler();
    for scale in [1.0, 2.5] {
        let cpu = render_rgba(&list, scale).unwrap();
        let (gpu, batches) = render(&list, scale, Limits::default());
        assert_eq!(batches, 1);
        let (most, over) = compare(&cpu, &gpu, 8);
        eprintln!("scale {scale}: largest difference {most}, {over:.5} over 8");
        // Only the edges differ, by the antialiasing.
        assert!(
            over < 0.01,
            "{over} of the channels differ at scale {scale}"
        );
    }
}

#[test]
#[ignore = "gpu"]
fn batches_give_the_same_pixels() {
    let list = sampler();
    let (one, batches) = render(&list, 2.0, Limits::default());
    assert_eq!(batches, 1);
    let small = Limits {
        tiles: 64,
        ..Limits::default()
    };
    let (many, batches) = render(&list, 2.0, small);
    assert!(batches > 3, "{batches} batches");
    let (most, _) = compare(&one, &many, 0);
    // The scratch texture has straight alpha in 8 bits.
    assert!(most <= 2, "a channel differs by {most}");
}

#[test]
#[ignore = "gpu"]
fn the_pixels_are_premultiplied() {
    let mut list = DisplayList::new(4.0, 4.0);
    list.push(fill(
        rect_path(0.0, 0.0, 4.0, 4.0),
        Color::rgba(255, 0, 0, 128),
    ));
    let (gpu, _) = render(&list, 1.0, Limits::default());
    let px = &gpu.data[..4];
    assert!(
        px[0].abs_diff(128) <= 1 && px[1] == 0 && px[3] == 128,
        "{px:?}"
    );
    assert_eq!(gpu, render_rgba(&list, 1.0).unwrap());
}

#[test]
#[ignore = "gpu"]
fn a_target_of_another_size_is_an_error() {
    let gpu = gpu();
    let list = DisplayList::new(4.0, 4.0);
    let target = GpuRenderer::target(&gpu.device, &list, 2.0).unwrap();
    let result = gpu
        .renderer
        .lock()
        .unwrap()
        .render(&gpu.device, &gpu.queue, &list, 1.0, &target);
    assert!(matches!(result, Err(RenderError::Gpu(_))));
}

#[test]
#[ignore = "gpu"]
fn an_image_draws_again_after_batches() {
    let list = sampler();
    let small = Limits {
        tiles: 64,
        ..Limits::default()
    };
    let cpu = render_rgba(&list, 1.0).unwrap();
    for _ in 0..2 {
        render(&list, 2.0, Limits::default());
        render(&list, 2.0, small);
        let (gpu, _) = render(&list, 1.0, Limits::default());
        // The red pixel of the image.
        let at = (70 * 200 + 160) * 4;
        assert_eq!(gpu.data[at..at + 4], cpu.data[at..at + 4]);
    }
}
