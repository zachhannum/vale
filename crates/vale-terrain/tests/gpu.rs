//! The brush on the GPU against the brush on the CPU.

use std::sync::{Arc, OnceLock, mpsc};

use vale_terrain::math::{V3, dot, lonlat_to_dir};
use vale_terrain::{
    ChannelMap, ChannelWindow, CoarseHeights, FACES, FlowMap, GROUP_STAMPS, GpuHeightmap,
    Heightmap, Mode, STAMP_SLOTS, Stamp, StampPlan, TILE_SIZE, TexelRect, WINDOW_CELLS, Window,
    WindowHeights, channel_map, window_channels,
};

/// The largest difference between a GPU level and a CPU level after raise,
/// lower, and flatten stamps, in 16-bit steps. The shader works with 32-bit
/// numbers, so a result near a half step can round to the other side.
const TOLERANCE: u16 = 1;

/// The same for smooth stamps. Each smooth stamp reads the results of the
/// stamps before it, so a difference of one step can grow to two.
const SMOOTH_TOLERANCE: u16 = 2;

/// The same for a stroke of raise or lower stamps. Each stamp rounds its
/// result, and the next stamp adds to it, so the differences of the stamps
/// that touch one texel can add. In the strokes of this file, up to 90
/// stamps touch one texel, and the largest difference is 1.
const ADD_STROKE_TOLERANCE: u16 = 3;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
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
        let info = adapter.get_info();
        eprintln!(
            "adapter: {} ({:?}, {:?})",
            info.name, info.backend, info.device_type
        );
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("the adapter gives a device");
        Gpu { device, queue }
    })
}

fn full(n: usize) -> TexelRect {
    TexelRect {
        x0: 0,
        y0: 0,
        x1: n,
        y1: n,
    }
}

impl Gpu {
    fn encoder(&self) -> wgpu::CommandEncoder {
        self.device.create_command_encoder(&Default::default())
    }

    fn submit(&self, enc: wgpu::CommandEncoder) {
        self.queue.submit([enc.finish()]);
    }

    /// A GPU heightmap with the texels and the channel map of `cpu`.
    fn copy_of(&self, cpu: &Heightmap) -> GpuHeightmap {
        self.copy_with_tiles(cpu, TILE_SIZE)
    }

    /// The same as `copy_of`, with `tile` by `tile` texels in a GPU tile.
    fn copy_with_tiles(&self, cpu: &Heightmap, tile: usize) -> GpuHeightmap {
        let map = GpuHeightmap::with_tile_size(&self.device, cpu.face_size() as u32, tile);
        map.clear(&self.queue, cpu.base());
        for (face, rect) in cpu.allocated_rects() {
            let mut data = Vec::new();
            cpu.read_rect(face, rect, &mut data);
            map.upload(&self.queue, face as u32, rect, &data);
        }
        if let Some(channels) = cpu.channels() {
            map.upload_channels(&self.queue, channels);
        }
        if let Some(window) = cpu.window() {
            map.upload_window(&self.queue, Some(window));
        }
        map
    }

    /// Applies the stamps to the GPU heightmap and to the CPU heightmap.
    /// Returns the number of batches.
    fn stamp(&self, map: &GpuHeightmap, cpu: &mut Heightmap, stamps: &[Stamp]) -> usize {
        let mut batches = 1;
        let mut enc = self.encoder();
        for stamp in stamps {
            let plan = cpu.stamp_plan(stamp);
            if !map.stamp(&self.queue, &mut enc, &plan) {
                self.submit(std::mem::replace(&mut enc, self.encoder()));
                map.begin_batch();
                batches += 1;
                assert!(map.stamp(&self.queue, &mut enc, &plan));
            }
            cpu.stamp(stamp);
        }
        self.submit(enc);
        map.begin_batch();
        batches
    }

    /// The texels of one rectangle on each face.
    fn read(&self, map: &GpuHeightmap, rect: TexelRect) -> Vec<Vec<u16>> {
        let mut enc = self.encoder();
        let readbacks: Vec<_> = (0..FACES as u32)
            .map(|face| map.read_rect(&self.device, &mut enc, face, rect))
            .collect();
        self.submit(enc);
        let faces: Vec<_> = readbacks
            .into_iter()
            .map(|readback| {
                let (sender, receiver) = mpsc::channel();
                readback.map(move |levels| sender.send(levels).expect("the test waits"));
                receiver
            })
            .collect();
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device answers");
        // The poll of another test can run the callbacks, so wait for them.
        faces
            .into_iter()
            .map(|receiver| receiver.recv().expect("the readback runs"))
            .collect()
    }

    fn read_all(&self, map: &GpuHeightmap) -> Vec<Vec<u16>> {
        self.read(map, full(map.face_size() as usize))
    }
}

/// The largest difference between the GPU texels and the CPU texels.
fn max_difference(cpu: &Heightmap, faces: &[Vec<u16>]) -> u16 {
    let mut worst = 0;
    for (face, levels) in faces.iter().enumerate() {
        let mut expected = Vec::new();
        cpu.read_rect(face, full(cpu.face_size()), &mut expected);
        assert_eq!(levels.len(), expected.len());
        for (a, b) in levels.iter().zip(&expected) {
            worst = worst.max(a.abs_diff(*b));
        }
    }
    worst
}

/// The result of the same stamps on the GPU and on the CPU.
struct Run {
    /// The GPU texels of each face.
    faces: Vec<Vec<u16>>,
    /// The largest difference between a GPU texel and a CPU texel.
    difference: u16,
    batches: usize,
}

/// Applies the stamps to a copy of `cpu` on the GPU and to `cpu`.
fn run(cpu: &mut Heightmap, stamps: &[Stamp]) -> Run {
    let gpu = gpu();
    let map = gpu.copy_of(cpu);
    let batches = gpu.stamp(&map, cpu, stamps);
    let faces = gpu.read_all(&map);
    Run {
        difference: max_difference(cpu, &faces),
        faces,
        batches,
    }
}

/// Sets all texels of one face to one level.
fn set_face(map: &mut Heightmap, face: usize, level: u16) {
    let n = map.face_size();
    map.store_rect(face, full(n), &vec![level; n * n]);
}

/// A heightmap with hills on each face and a step at each face edge.
fn hills(n: usize) -> Heightmap {
    let mut map = Heightmap::new(n, 0);
    for face in 0..FACES {
        let mut data = Vec::with_capacity(n * n);
        for y in 0..n {
            for x in 0..n {
                let (fx, fy) = (x as f64 * 37.0 / n as f64, y as f64 * 29.0 / n as f64);
                let hill = 9000.0 * fx.sin() * fy.cos() + 2500.0 * (3.0 * fx + fy).sin();
                data.push((30000.0 + 1500.0 * face as f64 + hill) as u16);
            }
        }
        map.store_rect(face, full(n), &data);
    }
    map
}

const MODES: [Mode; 4] = [Mode::Raise, Mode::Lower, Mode::Smooth, Mode::Flatten];

fn tolerance(mode: Mode) -> u16 {
    match mode {
        Mode::Smooth => SMOOTH_TOLERANCE,
        _ => TOLERANCE,
    }
}

fn stamp(center: V3, radius: f64, hardness: f64, mode: Mode) -> Stamp {
    Stamp {
        center,
        radius,
        hardness,
        flow: 0.8,
        mode,
        level: 52000,
        strength: 3000.0,
    }
}

#[test]
fn upload_and_read_rect_round_trip() {
    let gpu = gpu();
    let n = 256;
    let map = GpuHeightmap::new(&gpu.device, n as u32);
    assert_eq!(map.face_size(), 256);
    map.clear(&gpu.queue, 1234);
    // The width is odd, so the rows of the readback have padding.
    let rect = TexelRect {
        x0: 17,
        y0: 40,
        x1: 150,
        y1: 99,
    };
    let code = |x: usize, y: usize| (y * 257 + x * 3) as u16;
    let data: Vec<u16> = (rect.y0..rect.y1)
        .flat_map(|y| (rect.x0..rect.x1).map(move |x| code(x, y)))
        .collect();
    map.upload(&gpu.queue, 3, rect, &data);
    assert_eq!(gpu.read(&map, rect)[3], data);
    for (face, levels) in gpu.read_all(&map).iter().enumerate() {
        for (i, &level) in levels.iter().enumerate() {
            let (x, y) = (i % n, i / n);
            let inside = (rect.x0..rect.x1).contains(&x) && (rect.y0..rect.y1).contains(&y);
            let expected = if face == 3 && inside {
                code(x, y)
            } else {
                1234
            };
            assert_eq!(level, expected, "face {face} x {x} y {y}");
        }
    }
}

#[test]
fn gpu_matches_cpu_for_one_stamp() {
    let places = [
        ("face center", lonlat_to_dir(1.3, 0.7)),
        ("face edge", lonlat_to_dir(45.0, 0.0)),
        ("cube corner", lonlat_to_dir(45.0, 35.264)),
        ("pole", lonlat_to_dir(0.0, 90.0)),
    ];
    let mut worst = [0; 4];
    for (m, mode) in MODES.into_iter().enumerate() {
        for (place, center) in places {
            for radius in [0.01, 0.06, 0.3] {
                for hardness in [0.0, 0.5, 0.95] {
                    let mut cpu = hills(256);
                    let stamp = stamp(center, radius, hardness, mode);
                    let difference = run(&mut cpu, &[stamp]).difference;
                    assert!(
                        difference <= tolerance(mode),
                        "{mode:?} at the {place}, radius {radius}, hardness {hardness}: \
                         the difference is {difference}"
                    );
                    worst[m] = worst[m].max(difference);
                }
            }
        }
    }
    eprintln!("one stamp, largest difference for {MODES:?}: {worst:?}");
}

/// A stroke of stamps that overlap, across a face edge and near a cube
/// corner. The stroke needs more than one batch.
#[test]
fn gpu_matches_cpu_for_a_stroke() {
    let mut worst = [0; 4];
    let mut most_batches = 0;
    for (m, mode) in MODES.into_iter().enumerate() {
        for (lat, radius, hardness) in [(4.0, 0.07, 0.5), (35.0, 0.12, 0.0), (20.0, 0.03, 0.95)] {
            let count = STAMP_SLOTS as usize / 2;
            let stamps: Vec<Stamp> = (0..count)
                .map(|i| {
                    let lon = 33.0 + 24.0 * i as f64 / count as f64;
                    stamp(lonlat_to_dir(lon, lat), radius, hardness, mode)
                })
                .collect();
            let mut cpu = hills(256);
            let Run {
                difference,
                batches,
                ..
            } = run(&mut cpu, &stamps);
            most_batches = most_batches.max(batches);
            let limit = match mode {
                Mode::Raise | Mode::Lower => ADD_STROKE_TOLERANCE,
                _ => tolerance(mode),
            };
            assert!(
                difference <= limit,
                "{mode:?} stroke at latitude {lat}: the difference is {difference}"
            );
            worst[m] = worst[m].max(difference);
        }
    }
    assert!(most_batches > 1);
    eprintln!("stroke, largest difference for {MODES:?}: {worst:?}");
}

/// On a large face, a small brush is a few texels wide. The angles are then
/// too small for a 32-bit cosine.
#[test]
fn gpu_matches_cpu_for_a_small_brush_on_a_large_face() {
    let n = 2048;
    let mut cpu = Heightmap::new(n, 30000);
    set_face(&mut cpu, 2, 31000);
    let mut stamps = Vec::new();
    for mode in [Mode::Raise, Mode::Smooth, Mode::Flatten, Mode::Lower] {
        for (i, radius) in [0.002, 0.0035, 0.011].into_iter().enumerate() {
            for step in 0..8 {
                let lon = 44.9 + 0.03 * step as f64;
                let center = lonlat_to_dir(lon, 0.3 * i as f64 + 0.01 * step as f64);
                stamps.push(stamp(center, radius, 0.5, mode));
            }
        }
    }
    let difference = run(&mut cpu, &stamps).difference;
    eprintln!("small brush on a large face, largest difference: {difference}");
    assert!(difference <= SMOOTH_TOLERANCE, "{difference}");
}

/// A line of texels that crosses the +u edge of an even face at a right
/// angle. `m` is the place of the line along the edge. The line has `k`
/// texels on each face, and each texel touches the next one.
fn line_across_edge(n: usize, face: usize, m: usize, k: usize) -> Vec<(usize, usize, usize)> {
    assert!(face.is_multiple_of(2));
    let next = 2 * ((face / 2 + 1) % 3);
    let near = (n - k..n).map(|x| (face, x, m));
    // `u` of this face is the depth axis of the next face, and `v` of the
    // next face is the depth axis of this face.
    let far = (0..k).map(|i| (next, m, n - 1 - i));
    near.chain(far).collect()
}

/// Checks the levels along a line of `2 * k` texels with the face edge in
/// the middle.
fn assert_no_seam(name: &str, levels: &[i32], monotone: bool) {
    let k = levels.len() / 2;
    let jumps: Vec<i32> = levels.windows(2).map(|w| w[1] - w[0]).collect();
    let at_edge = jumps[k - 1].abs();
    let inside = jumps
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != k - 1)
        .map(|(_, jump)| jump.abs())
        .max()
        .unwrap();
    assert!(
        2 * at_edge <= 3 * inside + 4,
        "{name}: the jump at the edge is {at_edge}, and the largest jump inside is {inside}: \
         {levels:?}"
    );
    if monotone {
        let rising = levels[levels.len() - 1] > levels[0];
        assert!(
            jumps.iter().all(|&jump| jump == 0 || (jump > 0) == rising),
            "{name}: the levels go up and down: {levels:?}"
        );
    }
}

/// Smooths a map with one level on each face, then checks lines across the
/// edges of the even faces in `edges`.
fn smooth_and_check(levels: [u16; 3], center: V3, m: usize, edges: &[usize], monotone: bool) {
    let n = 256;
    let k = 12;
    let mut cpu = Heightmap::new(n, levels[0]);
    set_face(&mut cpu, 2, levels[1]);
    set_face(&mut cpu, 4, levels[2]);
    let stamps = vec![stamp(center, 0.15, 0.5, Mode::Smooth); 20];
    let Run {
        faces, difference, ..
    } = run(&mut cpu, &stamps);
    for &face in edges {
        let line = line_across_edge(n, face, m, k);
        let on_gpu: Vec<i32> = line
            .iter()
            .map(|&(face, x, y)| i32::from(faces[face][y * n + x]))
            .collect();
        let on_cpu: Vec<i32> = line
            .iter()
            .map(|&(face, x, y)| i32::from(cpu.get(face, x as i64, y as i64)))
            .collect();
        // The stamps changed each texel of the line.
        let old = |face: usize| i32::from(levels[face / 2]);
        for (i, &(face, ..)) in line.iter().enumerate() {
            assert_ne!(
                on_cpu[i],
                old(face),
                "texel {i} of the line is not in the brush"
            );
        }
        assert_no_seam(&format!("CPU, edge of face {face}"), &on_cpu, monotone);
        assert_no_seam(&format!("GPU, edge of face {face}"), &on_gpu, monotone);
    }
    assert!(
        difference <= SMOOTH_TOLERANCE,
        "the difference is {difference}"
    );
}

#[test]
fn smooth_leaves_no_seam_at_a_face_edge() {
    // Face 0 and face 2 meet at longitude 45.
    smooth_and_check(
        [20000, 40000, 20000],
        lonlat_to_dir(45.0, 0.0),
        128,
        &[0],
        true,
    );
}

#[test]
fn smooth_leaves_no_seam_at_a_cube_corner() {
    // Faces 0, 2, and 4 meet at this corner. Each line is 6 texels from it.
    let corner = lonlat_to_dir(45.0, 35.264);
    smooth_and_check([20000, 40000, 60000], corner, 249, &[0, 2, 4], false);
}

/// A heightmap with one large slope down to a sea, low hills on the slope,
/// and the channel map of its rivers.
fn river_land(n: usize) -> Heightmap {
    static CHANNELS: OnceLock<Arc<ChannelMap>> = OnceLock::new();
    let low = lonlat_to_dir(160.0, -20.0);
    let mut map = Heightmap::new(n, 0);
    for face in 0..FACES {
        let mut data = Vec::with_capacity(n * n);
        for y in 0..n {
            for x in 0..n {
                let d = map.texel_dir(face, x, y);
                let slope = 31000.0 + 7000.0 * (1.0 - dot(d, low));
                let hill = 300.0 * (40.0 * d[0]).sin() * (40.0 * d[1]).cos()
                    + 200.0 * (55.0 * d[2] + 20.0 * d[0]).sin();
                data.push((slope + hill) as u16);
            }
        }
        map.store_rect(face, full(n), &data);
    }
    assert_eq!(n, 256, "the channel map is for one size");
    let channels = CHANNELS.get_or_init(|| {
        let channels = channel_map(&CoarseHeights::new(&map));
        assert!(channels.has_rivers());
        Arc::new(channels)
    });
    map.set_channels(Some(channels.clone()));
    map
}

/// The number of levels that are not the same.
fn changes(before: &[u16], after: &[u16]) -> usize {
    let pairs = before.iter().zip(after);
    pairs.filter(|(old, new)| old != new).count()
}

/// The levels of all faces.
fn levels(map: &Heightmap) -> Vec<u16> {
    let mut all = Vec::new();
    for face in 0..FACES {
        map.read_rect(face, full(map.face_size()), &mut all);
    }
    all
}

#[test]
fn gpu_carve_matches_cpu_for_one_stamp() {
    let places = [
        ("face center", lonlat_to_dir(1.3, 0.7)),
        ("face edge", lonlat_to_dir(45.0, 0.0)),
        ("cube corner", lonlat_to_dir(45.0, 35.264)),
        ("pole", lonlat_to_dir(0.0, 90.0)),
    ];
    let mut worst = 0;
    for (place, center) in places {
        for radius in [0.06, 0.3] {
            for hardness in [0.0, 0.95] {
                let mut cpu = river_land(256);
                let before = levels(&cpu);
                let stamp = stamp(center, radius, hardness, Mode::Carve);
                let difference = run(&mut cpu, &[stamp]).difference;
                let name = format!("the {place}, radius {radius}, hardness {hardness}");
                let changed = changes(&before, &levels(&cpu));
                assert!(changed > 0, "{name}: no river is in the brush");
                assert!(
                    difference <= TOLERANCE,
                    "{name}: the difference is {difference}"
                );
                worst = worst.max(difference);
            }
        }
    }
    eprintln!("one carve stamp, largest difference: {worst}");
}

/// Strokes across a face edge and past a cube corner. Many stamps touch
/// each texel, so the ground at some rivers goes down to sea level.
#[test]
fn gpu_carve_matches_cpu_for_a_stroke() {
    let lines = [
        ("face edge", (31.0, 2.0), (59.0, 5.0)),
        ("cube corner", (35.0, 25.264), (55.0, 45.264)),
    ];
    let mut worst = 0;
    for (place, from, to) in lines {
        let stamps = stroke(from, to, 240, 0.05, Mode::Carve);
        let mut cpu = river_land(256);
        let before = levels(&cpu);
        let GroupRun {
            groups, difference, ..
        } = run_groups(&mut cpu, &stamps);
        assert!(
            groups.len() <= stamps.len() / GROUP_STAMPS + 2,
            "stroke at the {place}: the groups are {groups:?}"
        );
        let changed = changes(&before, &levels(&cpu));
        assert!(
            changed > 200,
            "stroke at the {place}: {changed} texels changed"
        );
        assert!(
            difference <= ADD_STROKE_TOLERANCE,
            "stroke at the {place}: the difference is {difference}"
        );
        worst = worst.max(difference);
    }
    eprintln!("carve stroke in groups, largest difference: {worst}");
}

#[test]
fn upload_channels_reaches_the_shader() {
    let gpu = gpu();
    let n = 256;
    let mut cpu = river_land(n);
    cpu.set_channels(None);
    let before = levels(&cpu);
    // The copy has the channel map of a new `GpuHeightmap`, with no rivers.
    let map = gpu.copy_of(&cpu);
    let plan = cpu.stamp_plan(&stamp(lonlat_to_dir(45.0, 0.0), 0.3, 0.5, Mode::Carve));
    let carve = || {
        let mut enc = gpu.encoder();
        assert!(map.stamp(&gpu.queue, &mut enc, &plan));
        gpu.submit(enc);
        map.begin_batch();
        gpu.read_all(&map).concat()
    };
    assert!(carve() == before);

    let channels = river_land(n)
        .channels()
        .expect("the map has rivers")
        .clone();
    assert_eq!(channels.size(), ChannelMap::size_for(n));
    map.upload_channels(&gpu.queue, &channels);
    cpu.set_channels(Some(channels));
    cpu.stamp(&plan.stamp);
    let carved = carve();
    assert!(carved != before);
    let cpu_levels = levels(&cpu);
    let difference = carved.iter().zip(&cpu_levels).map(|(a, b)| a.abs_diff(*b));
    assert!(difference.max().unwrap() <= TOLERANCE);

    map.upload_channels(&gpu.queue, &ChannelMap::empty(ChannelMap::size_for(n)));
    assert!(carve() == carved);
}

/// The face size of `window_land`. The window of a `GpuHeightmap` is half of
/// a face wide at this size.
const WINDOW_FACE: usize = 2 * WINDOW_CELLS;

/// A heightmap with a slope and hills on faces 0 and 2, the channel map of
/// its rivers, and a window with cells of one texel. The window is on face 0,
/// and it covers a part of face 2. The faces meet at longitude 45.
fn window_land() -> Heightmap {
    type Land = (Vec<Vec<u16>>, Arc<ChannelMap>, Arc<ChannelWindow>);
    static LAND: OnceLock<Land> = OnceLock::new();
    let n = WINDOW_FACE;
    let mut map = Heightmap::new(n, 0);
    let (faces, channels, window) = LAND.get_or_init(|| {
        let low = lonlat_to_dir(160.0, -20.0);
        let face = |face: usize| -> Vec<u16> {
            let texels = (0..n * n).map(|i| {
                let d = map.texel_dir(face, i % n, i / n);
                let slope = 31000.0 + 7000.0 * (1.0 - dot(d, low));
                let hill = 300.0 * (40.0 * d[0]).sin() * (40.0 * d[1]).cos()
                    + 200.0 * (55.0 * d[2] + 20.0 * d[0]).sin()
                    + 20.0 * (300.0 * d[1]).sin() * (300.0 * d[2]).cos();
                (slope + hill) as u16
            });
            texels.collect()
        };
        let faces = vec![face(0), face(2)];
        let mut land = Heightmap::new(n, 0);
        land.store_rect(0, full(n), &faces[0]);
        land.store_rect(2, full(n), &faces[1]);
        let flow = FlowMap::new(&CoarseHeights::new(&land));
        let window = Window::centered(lonlat_to_dir(40.0, 3.0), n, 1, WINDOW_CELLS);
        assert_eq!(window.face, 0);
        assert!(window.x0 + WINDOW_CELLS as i64 > n as i64 + 150);
        let window = window_channels(&WindowHeights::new(&land, window), &flow);
        assert!(window.has_rivers());
        (faces, Arc::new(flow.channels()), Arc::new(window))
    });
    map.store_rect(0, full(n), &faces[0]);
    map.store_rect(2, full(n), &faces[1]);
    map.set_channels(Some(channels.clone()));
    map.set_window(Some(window.clone()));
    map
}

/// Applies one carve stamp at each radius and hardness to `window_land`.
/// Returns the largest difference between the GPU and the CPU.
fn carve_in_window(lon: f64, lat: f64, face: usize) -> u16 {
    let mut worst = 0;
    for radius in [0.05, 0.08] {
        for hardness in [0.0, 0.95] {
            let mut cpu = window_land();
            let mut without = window_land();
            without.set_window(None);
            let before = levels(&cpu);
            let stamp = stamp(lonlat_to_dir(lon, lat), radius, hardness, Mode::Carve);
            let plan = cpu.stamp_plan(&stamp);
            let touched: Vec<usize> = (0..FACES).filter(|&f| plan.rects[f].is_some()).collect();
            assert_eq!(touched, [face], "the stamp is on one face");
            let difference = run(&mut cpu, &[stamp]).difference;
            let after = levels(&cpu);
            let changed = changes(&before, &after);
            assert!(changed > 300, "radius {radius}: {changed} texels changed");
            // The window gives the result, not the channel map.
            without.stamp(&stamp);
            assert!(changes(&after, &levels(&without)) > 300);
            worst = worst.max(difference);
        }
    }
    worst
}

#[test]
fn gpu_carve_in_a_window_matches_cpu_for_one_stamp() {
    let difference = carve_in_window(30.0, 3.0, 0);
    eprintln!("carve in a window, largest difference: {difference}");
    assert!(difference <= TOLERANCE, "{difference}");
}

#[test]
fn gpu_carve_in_a_window_matches_cpu_past_a_face_edge() {
    let difference = carve_in_window(53.0, 3.0, 2);
    eprintln!("carve in a window past a face edge, largest difference: {difference}");
    assert!(difference <= TOLERANCE, "{difference}");
}

/// A stroke from the face of the window to the next face.
///
/// The brush has no hard part, so each stamp has a different effect on a
/// texel. On the next face, the place of a texel in the window has an error
/// of about 0.0003 channel texels on the GPU. If many stamps have one effect
/// on a texel, and that effect is that near to a half step, each of them
/// rounds to the other side, and the differences add without a limit.
#[test]
fn gpu_carve_in_a_window_matches_cpu_for_a_stroke() {
    let mut stamps = stroke((36.0, 2.0), (54.0, 5.0), 240, 0.05, Mode::Carve);
    for stamp in &mut stamps {
        stamp.hardness = 0.0;
    }
    let mut cpu = window_land();
    let before = levels(&cpu);
    let GroupRun {
        groups, difference, ..
    } = run_groups(&mut cpu, &stamps);
    assert!(
        groups.len() <= stamps.len() / GROUP_STAMPS + 2,
        "{groups:?}"
    );
    let n = WINDOW_FACE * WINDOW_FACE;
    let after = levels(&cpu);
    for face in [0, 2] {
        let changed = changes(&before[face * n..][..n], &after[face * n..][..n]);
        assert!(changed > 500, "face {face}: {changed} texels changed");
    }
    eprintln!("carve stroke in a window, largest difference: {difference}");
    assert!(difference <= ADD_STROKE_TOLERANCE, "{difference}");
}

/// The result of the same stamps in groups and one at a time.
struct GroupRun {
    /// The GPU texels of each face after the stamps in groups.
    faces: Vec<Vec<u16>>,
    /// The number of stamps in each group.
    groups: Vec<usize>,
    /// The number of times that the batch was full.
    full: usize,
    /// The largest difference between a GPU texel and a CPU texel.
    difference: u16,
}

/// Applies the stamps to `cpu`, to a GPU copy in groups, and to a GPU copy
/// one at a time. The two GPU results must be the same.
fn run_groups(cpu: &mut Heightmap, stamps: &[Stamp]) -> GroupRun {
    let gpu = gpu();
    let grouped = gpu.copy_of(cpu);
    let single = gpu.copy_of(cpu);
    let plans: Vec<StampPlan> = stamps.iter().map(|stamp| cpu.stamp_plan(stamp)).collect();

    let (mut groups, mut full) = (Vec::new(), 0);
    let mut enc = gpu.encoder();
    let mut rest = &plans[..];
    while !rest.is_empty() {
        let mut count = grouped.stamp_group(&gpu.queue, &mut enc, rest);
        if count == 0 {
            gpu.submit(std::mem::replace(&mut enc, gpu.encoder()));
            grouped.begin_batch();
            full += 1;
            count = grouped.stamp_group(&gpu.queue, &mut enc, rest);
            assert!(count > 0, "an empty batch takes a group");
        }
        groups.push(count);
        rest = &rest[count..];
    }
    gpu.submit(enc);
    grouped.begin_batch();

    let mut enc = gpu.encoder();
    for plan in &plans {
        if !single.stamp(&gpu.queue, &mut enc, plan) {
            gpu.submit(std::mem::replace(&mut enc, gpu.encoder()));
            single.begin_batch();
            assert!(single.stamp(&gpu.queue, &mut enc, plan));
        }
    }
    gpu.submit(enc);
    single.begin_batch();

    for stamp in stamps {
        cpu.stamp(stamp);
    }
    let faces = gpu.read_all(&grouped);
    let one_at_a_time = gpu.read_all(&single);
    for face in 0..FACES {
        assert!(
            faces[face] == one_at_a_time[face],
            "face {face}: the groups and the single stamps give different texels"
        );
    }
    GroupRun {
        difference: max_difference(cpu, &faces),
        faces,
        groups,
        full,
    }
}

/// A stamp with a small effect, so that many of them on one texel stay
/// inside the range of the levels.
fn light(lon: f64, lat: f64, radius: f64, mode: Mode) -> Stamp {
    Stamp {
        flow: 0.1,
        strength: 1500.0,
        ..stamp(lonlat_to_dir(lon, lat), radius, 0.5, mode)
    }
}

/// A stroke of `count` stamps along a line of longitude and latitude.
fn stroke(from: (f64, f64), to: (f64, f64), count: usize, radius: f64, mode: Mode) -> Vec<Stamp> {
    (0..count)
        .map(|i| {
            let t = i as f64 / count as f64;
            let lon = from.0 + (to.0 - from.0) * t;
            let lat = from.1 + (to.1 - from.1) * t;
            light(lon, lat, radius, mode)
        })
        .collect()
}

/// Strokes of small stamps at about 0.1 of the radius apart, across a face
/// edge and past a cube corner.
#[test]
fn groups_match_single_stamps_and_the_cpu_for_a_stroke() {
    let lines = [
        ("face edge", (31.0, 2.0), (59.0, 5.0)),
        ("cube corner", (35.0, 25.264), (55.0, 45.264)),
    ];
    let modes = [Mode::Raise, Mode::Lower, Mode::Flatten];
    let mut worst = [0; 3];
    for (m, mode) in modes.into_iter().enumerate() {
        for (place, from, to) in lines {
            let stamps = stroke(from, to, 240, 0.02, mode);
            let mut cpu = hills(256);
            let before: Vec<Vec<u16>> = (0..FACES)
                .map(|face| {
                    let mut old = Vec::new();
                    cpu.read_rect(face, full(256), &mut old);
                    old
                })
                .collect();
            let GroupRun {
                faces,
                groups,
                difference,
                ..
            } = run_groups(&mut cpu, &stamps);
            assert!(
                groups.len() <= stamps.len() / GROUP_STAMPS + 2,
                "{mode:?} stroke at the {place}: the groups are {groups:?}"
            );
            let touched = (0..FACES)
                .filter(|&face| before[face] != faces[face])
                .count();
            let expected = if place == "cube corner" { 3 } else { 2 };
            assert_eq!(touched, expected, "{mode:?} stroke at the {place}");
            let limit = match mode {
                Mode::Flatten => TOLERANCE,
                _ => ADD_STROKE_TOLERANCE,
            };
            assert!(
                difference <= limit,
                "{mode:?} stroke at the {place}: the difference is {difference}"
            );
            worst[m] = worst[m].max(difference);
        }
    }
    eprintln!("stroke in groups, largest difference for {modes:?}: {worst:?}");
}

#[test]
fn a_group_has_one_mode_and_no_smooth_stamp() {
    use Mode::{Flatten, Lower, Raise, Smooth};
    let modes = [
        Raise, Raise, Smooth, Smooth, Flatten, Lower, Lower, Lower, Raise, Smooth, Flatten, Flatten,
    ];
    let stamps: Vec<Stamp> = modes
        .into_iter()
        .enumerate()
        .map(|(i, mode)| light(44.0 + 0.1 * i as f64, 34.0, 0.05, mode))
        .collect();
    let mut cpu = hills(256);
    let GroupRun {
        groups, difference, ..
    } = run_groups(&mut cpu, &stamps);
    assert_eq!(groups, [2, 1, 1, 1, 3, 1, 1, 2]);
    assert!(difference <= SMOOTH_TOLERANCE, "{difference}");
}

#[test]
fn a_group_has_a_largest_size() {
    let stamps = vec![light(10.0, 5.0, 0.03, Mode::Raise); GROUP_STAMPS + 8];
    let mut cpu = hills(256);
    let GroupRun {
        groups, difference, ..
    } = run_groups(&mut cpu, &stamps);
    assert_eq!(groups, [GROUP_STAMPS, 8]);
    assert!(difference <= ADD_STROKE_TOLERANCE, "{difference}");
}

/// Each stamp at the cube corner takes three passes, and a change of mode
/// ends a group. The batch is full before the end of the list.
#[test]
fn a_full_batch_takes_no_group() {
    let count = STAMP_SLOTS as usize;
    let stamps: Vec<Stamp> = (0..count)
        .map(|i| {
            let mode = [Mode::Raise, Mode::Lower][i / 2 % 2];
            light(45.0, 35.264, 0.04, mode)
        })
        .collect();
    let mut cpu = hills(256);
    let GroupRun {
        groups,
        full,
        difference,
        ..
    } = run_groups(&mut cpu, &stamps);
    assert_eq!(groups, vec![2; count / 2]);
    // Each group takes 3 of the slots.
    assert_eq!(full, (count / 2 * 3 - 1) / STAMP_SLOTS as usize);
    assert!(full > 0);
    assert!(difference <= ADD_STROKE_TOLERANCE, "{difference}");
}

/// Two places on one face. One pass for both would cover the texels between
/// them.
#[test]
fn stamps_far_apart_are_not_one_group() {
    let stamps: Vec<Stamp> = (0..12)
        .map(|i| {
            let lon = if i % 2 == 0 { -30.0 } else { 30.0 };
            light(lon, 0.0, 0.03, Mode::Raise)
        })
        .collect();
    let mut cpu = hills(256);
    let GroupRun {
        groups, difference, ..
    } = run_groups(&mut cpu, &stamps);
    assert_eq!(groups, [1; 12]);
    assert!(difference <= ADD_STROKE_TOLERANCE, "{difference}");

    // Stamps on two faces are one group, with one pass for each face.
    let stamps: Vec<Stamp> = (0..12)
        .map(|i| {
            let lon = if i % 2 == 0 { 0.0 } else { 90.0 };
            light(lon, 0.0, 0.03, Mode::Raise)
        })
        .collect();
    let mut cpu = hills(256);
    let GroupRun {
        groups, difference, ..
    } = run_groups(&mut cpu, &stamps);
    assert_eq!(groups, [12]);
    assert!(difference <= ADD_STROKE_TOLERANCE, "{difference}");
}

/// The memory of the table of the tiles, in bytes.
fn table_bytes(n: usize) -> usize {
    let per_side = n.div_ceil(TILE_SIZE);
    FACES * per_side * per_side * 4
}

#[test]
fn an_empty_world_takes_memory_for_the_table_only() {
    let gpu = gpu();
    for n in [4096, 8192] {
        let map = GpuHeightmap::new(&gpu.device, n as u32);
        map.clear(&gpu.queue, 30000);
        assert_eq!(map.allocated_tiles(), 0);
        assert_eq!(map.memory_bytes(), table_bytes(n), "face size {n}");
        // Each texel reads as the base level.
        let rect = TexelRect {
            x0: n - 300,
            y0: 100,
            x1: n,
            y1: 400,
        };
        for levels in gpu.read(&map, rect) {
            assert!(levels.iter().all(|level| *level == 30000));
        }
    }
}

/// The same brush in texels takes the same memory at each face size.
#[test]
fn memory_grows_with_the_painted_tiles_and_not_with_the_face_size() {
    let gpu = gpu();
    let memory = |n: usize, stamps: usize| {
        let mut cpu = Heightmap::new(n, 30000);
        let map = gpu.copy_of(&cpu);
        // A brush of 300 texels across, at a tile corner near the face center.
        let radius = 150.0 * std::f64::consts::FRAC_PI_2 / n as f64;
        let stamps: Vec<Stamp> = (0..stamps)
            .map(|i| {
                stamp(
                    lonlat_to_dir(i as f64 * 2.0 * radius.to_degrees(), 0.0),
                    radius,
                    0.5,
                    Mode::Raise,
                )
            })
            .collect();
        gpu.stamp(&map, &mut cpu, &stamps);
        assert!(max_difference(&cpu, &gpu.read_all(&map)) <= TOLERANCE);
        (map.allocated_tiles(), map.memory_bytes() - table_bytes(n))
    };
    let (tiles, small) = memory(4096, 1);
    assert_eq!(tiles, 4);
    assert_eq!(memory(8192, 1), (tiles, small));
    // One layer of the pool and the render target of the brush.
    let tile = TILE_SIZE * TILE_SIZE * 2;
    assert_eq!(small, 64 * tile + 4 * tile);
    // More stamps take more tiles, and the same number at each face size.
    let (more_tiles, more) = memory(4096, 40);
    assert!(more_tiles > 64 && more > small);
    assert_eq!(memory(8192, 40), (more_tiles, more));
    // The pool has at most two times the memory of its tiles, plus a layer.
    assert!(more <= (2 * more_tiles + 64 + 4) * tile);
}

#[test]
fn a_tile_at_the_base_level_gives_its_memory_back() {
    let gpu = gpu();
    let n = 1024;
    let map = GpuHeightmap::new(&gpu.device, n as u32);
    map.clear(&gpu.queue, 500);
    let tile = TexelRect {
        x0: 256,
        y0: 512,
        x1: 512,
        y1: 768,
    };
    let part = TexelRect {
        x0: 300,
        y0: 600,
        x1: 310,
        y1: 610,
    };
    // The base level takes no tile.
    map.upload(&gpu.queue, 2, part, &[500; 100]);
    assert_eq!(map.allocated_tiles(), 0);
    map.upload(&gpu.queue, 2, part, &[900; 100]);
    assert_eq!(map.allocated_tiles(), 1);
    let levels = &gpu.read(&map, tile)[2];
    assert_eq!(levels.iter().filter(|level| **level == 900).count(), 100);
    assert_eq!(
        levels.iter().filter(|level| **level == 500).count(),
        256 * 256 - 100
    );
    // A part of a tile at the base level keeps the tile.
    map.upload(&gpu.queue, 2, part, &[500; 100]);
    assert_eq!(map.allocated_tiles(), 1);
    map.upload(&gpu.queue, 2, tile, &vec![500; 256 * 256]);
    assert_eq!(map.allocated_tiles(), 0);
    assert!(gpu.read(&map, tile)[2].iter().all(|level| *level == 500));
    // The next tile takes the free slot.
    map.upload(&gpu.queue, 4, part, &[700; 100]);
    assert_eq!(map.allocated_tiles(), 1);
    assert_eq!(map.memory_bytes(), table_bytes(n) + 64 * 256 * 256 * 2);
    assert!(gpu.read(&map, part)[4].iter().all(|level| *level == 700));
    map.clear(&gpu.queue, 100);
    assert_eq!(map.memory_bytes(), table_bytes(n));
    assert!(gpu.read(&map, tile)[4].iter().all(|level| *level == 100));
}

/// The stamps of each mode on small tiles, across tile edges, a face edge,
/// and a cube corner. Some of the tiles under the stamps have no memory. The
/// levels do not depend on the tile size, so a tile edge is no seam.
#[test]
fn small_tiles_give_the_same_levels_as_one_tile_for_each_face() {
    let gpu = gpu();
    for mode in MODES {
        let stamps = [
            stamp(lonlat_to_dir(3.0, 2.0), 0.2, 0.3, mode),
            stamp(lonlat_to_dir(45.0, 1.0), 0.1, 0.5, mode),
            stamp(lonlat_to_dir(44.0, 35.0), 0.15, 0.0, mode),
            stamp(lonlat_to_dir(40.0, 20.0), 0.02, 0.9, mode),
        ];
        let mut faces = Vec::new();
        let mut tiles = Vec::new();
        for tile in [256, 32, 24] {
            let mut cpu = hills(256);
            // The base level of `hills` on a part of face 0 and on face 2.
            let part = TexelRect {
                x0: 0,
                y0: 0,
                x1: 100,
                y1: 256,
            };
            cpu.store_rect(0, part, &[0; 100 * 256]);
            set_face(&mut cpu, 2, 0);
            let map = gpu.copy_with_tiles(&cpu, tile);
            tiles.push(map.allocated_tiles());
            gpu.stamp(&map, &mut cpu, &stamps);
            let levels = gpu.read_all(&map);
            let difference = max_difference(&cpu, &levels);
            assert!(
                difference <= tolerance(mode),
                "{mode:?}, tile {tile}: {difference}"
            );
            faces.push(levels);
        }
        // 3 columns of 8 tiles on face 0 and all 64 tiles of face 2 start
        // with no memory.
        assert_eq!(tiles, [5, 6 * 64 - 24 - 64, 6 * 121 - 44 - 121]);
        assert!(faces[1] == faces[0], "{mode:?}, tiles of 32 texels");
        assert!(faces[2] == faces[0], "{mode:?}, tiles of 24 texels");
    }
}

/// A smooth stamp on a bowl that crosses tile edges. The step from texel to
/// texel changes by the same amount at a tile edge and inside a tile.
#[test]
fn smooth_leaves_no_seam_at_a_tile_edge() {
    let gpu = gpu();
    let n = 256;
    let mut cpu = Heightmap::new(n, 20000);
    let bowl: Vec<u16> = (0..n * n)
        .map(|i| 20000 + 2 * (i % n).abs_diff(128).pow(2) as u16)
        .collect();
    cpu.store_rect(0, full(n), &bowl);
    let map = gpu.copy_with_tiles(&cpu, 32);
    let smooth = stamp(lonlat_to_dir(0.0, 0.0), 0.3, 1.0, Mode::Smooth);
    gpu.stamp(&map, &mut cpu, &[smooth; 3]);
    let levels = gpu.read_all(&map);
    assert!(max_difference(&cpu, &levels) <= SMOOTH_TOLERANCE);
    let row: Vec<i32> = (100..157)
        .map(|x| i32::from(levels[0][128 * n + x]))
        .collect();
    assert!(
        row.iter()
            .zip(&bowl[128 * n + 100..])
            .any(|(a, b)| *a != i32::from(*b))
    );
    for (i, three) in row.windows(3).enumerate() {
        let bend = three[2] - 2 * three[1] + three[0];
        assert!(
            (bend - 4).abs() <= 3,
            "x {}: the bend is {bend}: {row:?}",
            101 + i
        );
    }
}
