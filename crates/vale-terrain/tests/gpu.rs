//! The brush on the GPU against the brush on the CPU.

use std::sync::{Arc, OnceLock, mpsc};

use vale_terrain::math::{V3, dot, lonlat_to_dir};
use vale_terrain::{
    ChannelMap, ChannelWindow, CoarseHeights, ErodeBrush, Erosion, FACES, FIELD_WGSL, FlowMap,
    GROUP_STAMPS, GpuHeightmap, GpuPyramid, GpuTiles, Heightmap, Limits, Mode, OuterFlow, Pyramid,
    Readback, STAMP_SLOTS, Stamp, StampPlan, TILE_SIZE, TILES_WGSL, TexelRect, WINDOW_CELLS,
    Window, WindowHeights, channel_map, pyramid_layout, tiles_layout, window_channels,
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

/// The same for weather stamps. Each pair of texels moves a whole number of
/// levels. The 32-bit weight of a pair can give one level more or less, and a
/// texel is in four pairs.
const WEATHER_TOLERANCE: u16 = 2;

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
        self.read_with(|enc, face| map.read_rect(&self.device, enc, face, rect))
    }

    /// The same as `read`, for a map with no brush.
    fn read_tiles(&self, map: &GpuTiles, rect: TexelRect) -> Vec<Vec<u16>> {
        self.read_with(|enc, face| map.read_rect(&self.device, enc, face, rect))
    }

    /// The texels of the readback that `read` makes for each face.
    fn read_with(
        &self,
        mut read: impl FnMut(&mut wgpu::CommandEncoder, u32) -> Readback,
    ) -> Vec<Vec<u16>> {
        let mut enc = self.encoder();
        let readbacks: Vec<_> = (0..FACES as u32).map(|face| read(&mut enc, face)).collect();
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
        talus: 0.0,
        scale: 0.1,
        seed: 1,
        limits: Limits::default(),
    }
}

#[test]
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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

fn weather(center: V3, radius: f64, hardness: f64) -> Stamp {
    Stamp {
        talus: 40.0,
        ..stamp(center, radius, hardness, Mode::Weather)
    }
}

#[test]
#[ignore = "gpu"]
fn gpu_weather_matches_cpu_for_one_stamp() {
    let places = [
        ("face center", lonlat_to_dir(1.3, 0.7)),
        ("face edge", lonlat_to_dir(45.0, 0.0)),
        ("cube corner", lonlat_to_dir(45.0, 35.264)),
        ("pole", lonlat_to_dir(0.0, 90.0)),
    ];
    let mut worst = 0;
    for (place, center) in places {
        for radius in [0.01, 0.06, 0.3] {
            for hardness in [0.0, 0.5, 0.95] {
                let mut cpu = hills(256);
                let before = levels(&cpu);
                let stamp = weather(center, radius, hardness);
                let difference = run(&mut cpu, &[stamp]).difference;
                assert!(levels(&cpu) != before, "{place}, radius {radius}");
                assert!(
                    difference <= WEATHER_TOLERANCE,
                    "weather at the {place}, radius {radius}, hardness {hardness}: \
                     the difference is {difference}"
                );
                worst = worst.max(difference);
            }
        }
    }
    eprintln!("one weather stamp, largest difference: {worst}");
}

/// A stroke of weather stamps that overlap, across a face edge and near a
/// cube corner.
fn weather_stroke(lat: f64, radius: f64, hardness: f64) -> Vec<Stamp> {
    let count = STAMP_SLOTS as usize / 2;
    (0..count)
        .map(|i| {
            let lon = 33.0 + 24.0 * i as f64 / count as f64;
            weather(lonlat_to_dir(lon, lat), radius, hardness)
        })
        .collect()
}

#[test]
#[ignore = "gpu"]
fn gpu_weather_matches_cpu_for_a_stroke() {
    let mut worst = 0;
    for (lat, radius, hardness) in [(4.0, 0.07, 0.5), (35.0, 0.12, 0.0), (20.0, 0.03, 0.95)] {
        let mut cpu = hills(256);
        let difference = run(&mut cpu, &weather_stroke(lat, radius, hardness)).difference;
        assert!(
            difference <= WEATHER_TOLERANCE,
            "weather stroke at latitude {lat}: the difference is {difference}"
        );
        worst = worst.max(difference);
    }
    eprintln!("weather stroke, largest difference: {worst}");
}

#[test]
#[ignore = "gpu"]
fn gpu_weather_keeps_the_sum_of_the_levels() {
    let sum = |faces: &[Vec<u16>]| -> u64 { faces.iter().flatten().map(|&l| u64::from(l)).sum() };
    let mut cpu = hills(256);
    let before: Vec<Vec<u16>> = (0..FACES)
        .map(|face| {
            let mut old = Vec::new();
            cpu.read_rect(face, full(256), &mut old);
            old
        })
        .collect();
    let faces = run(&mut cpu, &weather_stroke(35.0, 0.12, 0.0)).faces;
    assert!(faces != before);
    assert_eq!(sum(&faces), sum(&before));
}

/// On a large face, a small brush is a few texels wide. The angles are then
/// too small for a 32-bit cosine.
#[test]
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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

/// Applies one stamp of a mode at each radius and hardness to `window_land`.
/// Each stamp changes more than `least` texels. Returns the largest
/// difference between the GPU and the CPU.
fn stamp_in_window(mode: Mode, lon: f64, lat: f64, face: usize, least: usize) -> u16 {
    let mut worst = 0;
    for radius in [0.05, 0.08] {
        for hardness in [0.0, 0.95] {
            let mut cpu = window_land();
            let mut without = window_land();
            without.set_window(None);
            let before = levels(&cpu);
            let stamp = stamp(lonlat_to_dir(lon, lat), radius, hardness, mode);
            let plan = cpu.stamp_plan(&stamp);
            let touched: Vec<usize> = (0..FACES).filter(|&f| plan.rects[f].is_some()).collect();
            assert_eq!(touched, [face], "the stamp is on one face");
            let difference = run(&mut cpu, &[stamp]).difference;
            let after = levels(&cpu);
            let changed = changes(&before, &after);
            assert!(changed > least, "radius {radius}: {changed} texels changed");
            // The window gives the result, not the channel map.
            without.stamp(&stamp);
            assert!(changes(&after, &levels(&without)) > least);
            worst = worst.max(difference);
        }
    }
    worst
}

#[test]
#[ignore = "gpu"]
fn gpu_carve_in_a_window_matches_cpu_for_one_stamp() {
    let difference = stamp_in_window(Mode::Carve, 30.0, 3.0, 0, 300);
    eprintln!("carve in a window, largest difference: {difference}");
    assert!(difference <= TOLERANCE, "{difference}");
}

#[test]
#[ignore = "gpu"]
fn gpu_carve_in_a_window_matches_cpu_past_a_face_edge() {
    let difference = stamp_in_window(Mode::Carve, 53.0, 3.0, 2, 300);
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
#[ignore = "gpu"]
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

#[test]
#[ignore = "gpu"]
fn gpu_ridge_matches_cpu_for_one_stamp() {
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
                let stamp = stamp(center, radius, hardness, Mode::Ridge);
                let difference = run(&mut cpu, &[stamp]).difference;
                let name = format!("the {place}, radius {radius}, hardness {hardness}");
                let changed = changes(&before, &levels(&cpu));
                assert!(changed > 0, "{name}: no land is in the brush");
                assert!(
                    difference <= TOLERANCE,
                    "{name}: the difference is {difference}"
                );
                worst = worst.max(difference);
            }
        }
    }
    eprintln!("one ridge stamp, largest difference: {worst}");
}

#[test]
#[ignore = "gpu"]
fn gpu_ridge_matches_cpu_for_a_stroke() {
    let lines = [
        ("face edge", (31.0, 2.0), (59.0, 5.0)),
        ("cube corner", (35.0, 25.264), (55.0, 45.264)),
    ];
    let mut worst = 0;
    for (place, from, to) in lines {
        let stamps = stroke(from, to, 240, 0.05, Mode::Ridge);
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
    eprintln!("ridge stroke in groups, largest difference: {worst}");
}

#[test]
#[ignore = "gpu"]
fn gpu_ridge_in_a_window_matches_cpu_for_one_stamp() {
    let difference = stamp_in_window(Mode::Ridge, 30.0, 3.0, 0, 300);
    eprintln!("ridge in a window, largest difference: {difference}");
    assert!(difference <= TOLERANCE, "{difference}");
}

#[test]
#[ignore = "gpu"]
fn gpu_ridge_in_a_window_matches_cpu_past_a_face_edge() {
    let difference = stamp_in_window(Mode::Ridge, 53.0, 3.0, 2, 300);
    eprintln!("ridge in a window past a face edge, largest difference: {difference}");
    assert!(difference <= TOLERANCE, "{difference}");
}

#[test]
#[ignore = "gpu"]
fn gpu_deposit_matches_cpu_for_one_stamp() {
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
                let stamp = stamp(center, radius, hardness, Mode::Deposit);
                let difference = run(&mut cpu, &[stamp]).difference;
                let name = format!("the {place}, radius {radius}, hardness {hardness}");
                let changed = changes(&before, &levels(&cpu));
                assert!(changed > 0, "{name}: no pit or valley is in the brush");
                assert!(
                    difference <= TOLERANCE,
                    "{name}: the difference is {difference}"
                );
                worst = worst.max(difference);
            }
        }
    }
    eprintln!("one deposit stamp, largest difference: {worst}");
}

#[test]
#[ignore = "gpu"]
fn gpu_deposit_matches_cpu_for_a_stroke() {
    let lines = [
        ("face edge", (31.0, 2.0), (59.0, 5.0)),
        ("cube corner", (35.0, 25.264), (55.0, 45.264)),
    ];
    let mut worst = 0;
    for (place, from, to) in lines {
        let stamps = stroke(from, to, 240, 0.05, Mode::Deposit);
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
            changed > 100,
            "stroke at the {place}: {changed} texels changed"
        );
        assert!(
            difference <= ADD_STROKE_TOLERANCE,
            "stroke at the {place}: the difference is {difference}"
        );
        worst = worst.max(difference);
    }
    eprintln!("deposit stroke in groups, largest difference: {worst}");
}

#[test]
#[ignore = "gpu"]
fn gpu_deposit_in_a_window_matches_cpu_for_one_stamp() {
    let difference = stamp_in_window(Mode::Deposit, 30.0, 3.0, 0, 10);
    eprintln!("deposit in a window, largest difference: {difference}");
    assert!(difference <= TOLERANCE, "the difference is {difference}");
}

#[test]
#[ignore = "gpu"]
fn gpu_deposit_in_a_window_matches_cpu_past_a_face_edge() {
    let difference = stamp_in_window(Mode::Deposit, 53.0, 3.0, 2, 10);
    eprintln!("deposit in a window past a face edge, largest difference: {difference}");
    assert!(difference <= TOLERANCE, "the difference is {difference}");
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
fn a_group_has_one_mode_and_no_smooth_stamp() {
    use Mode::{Flatten, Lower, Raise, Smooth, Weather};
    let modes = [
        Raise, Raise, Smooth, Smooth, Flatten, Lower, Lower, Lower, Raise, Smooth, Flatten,
        Flatten, Weather, Weather,
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
    assert_eq!(groups, [2, 1, 1, 1, 3, 1, 1, 2, 1, 1]);
    assert!(difference <= SMOOTH_TOLERANCE, "{difference}");
}

#[test]
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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

/// A code that is different for each texel of a face of 64.
fn code(face: usize, x: usize, y: usize) -> u16 {
    (face * 5000 + y * 67 + x * 3) as u16
}

/// A map with tiles of 16, and codes on a part of four faces.
fn coded() -> Heightmap {
    let mut cpu = Heightmap::with_tile_size(64, 16, 321);
    for face in [0, 2, 3, 5] {
        for y in 5 + face..40 {
            for x in 20..64 - face {
                cpu.set(face, x, y, code(face, x, y));
            }
        }
    }
    cpu
}

/// A display map with the texels of `cpu`, reduced by `factor`.
fn display_of(gpu: &Gpu, cpu: &Heightmap, factor: usize) -> GpuTiles {
    let size = cpu.face_size().div_ceil(factor);
    let map = GpuTiles::with_tile_size(&gpu.device, size as u32, 16, cpu.base());
    for face in 0..FACES {
        let mut data = Vec::new();
        cpu.read_reduced(face, full(size), factor, &mut data);
        map.upload(&gpu.queue, face as u32, full(size), &data);
    }
    map
}

#[test]
#[ignore = "gpu"]
fn a_display_map_reads_the_same_levels_as_the_heightmap() {
    let gpu = gpu();
    let cpu = coded();
    let n = cpu.face_size();
    let heights = gpu.copy_with_tiles(&cpu, 16);
    let display = display_of(gpu, &cpu, 1);
    assert_eq!(display.face_size(), heights.face_size());
    let levels = gpu.read_tiles(&display, full(n));
    assert_eq!(max_difference(&cpu, &levels), 0);
    assert_eq!(levels, gpu.read_all(&heights));
    // A tile with no code takes no memory, as in the heightmap.
    assert_eq!(display.allocated_tiles(), cpu.allocated_tiles());
    assert_eq!(display.allocated_tiles(), heights.allocated_tiles());
    // The display map has no render target of a brush.
    let table = FACES * 4 * 4 * 4;
    assert_eq!(display.memory_bytes(), table + 64 * 16 * 16 * 2);

    // A part of a face goes to the same texels.
    let rect = TexelRect {
        x0: 10,
        y0: 30,
        x1: 37,
        y1: 50,
    };
    let data = vec![4000; 27 * 20];
    heights.upload(&gpu.queue, 1, rect, &data);
    display.upload(&gpu.queue, 1, rect, &data);
    let levels = gpu.read_tiles(&display, full(n));
    assert_eq!(levels, gpu.read_all(&heights));
    assert_eq!(
        levels[1].iter().filter(|level| **level == 4000).count(),
        540
    );
}

#[test]
#[ignore = "gpu"]
fn a_display_map_holds_the_reduced_levels() {
    let gpu = gpu();
    let cpu = coded();
    let display = display_of(gpu, &cpu, 4);
    assert_eq!(display.face_size(), 16);
    for (face, levels) in gpu.read_tiles(&display, full(16)).iter().enumerate() {
        let mut expected = Vec::new();
        cpu.read_reduced(face, full(16), 4, &mut expected);
        assert_eq!(*levels, expected, "face {face}");
    }
    // Faces 1 and 4 have the base level, so their tiles take no memory.
    assert_eq!(display.allocated_tiles(), 4);
}

#[test]
#[ignore = "gpu"]
fn clear_puts_each_tile_of_a_display_map_back_to_the_base() {
    let gpu = gpu();
    let cpu = coded();
    let display = display_of(gpu, &cpu, 1);
    assert!(display.allocated_tiles() > 0);
    display.clear(&gpu.queue, 9000);
    assert_eq!(display.allocated_tiles(), 0);
    assert_eq!(display.memory_bytes(), FACES * 4 * 4 * 4);
    for levels in gpu.read_tiles(&display, full(64)) {
        assert!(levels.iter().all(|level| *level == 9000));
    }
    // A tile that gets the new base level in each texel takes no memory.
    let tile = TexelRect {
        x0: 16,
        y0: 32,
        x1: 32,
        y1: 48,
    };
    display.upload(&gpu.queue, 2, tile, &[9000; 256]);
    assert_eq!(display.allocated_tiles(), 0);
    display.upload(&gpu.queue, 2, tile, &[1; 256]);
    assert_eq!(display.allocated_tiles(), 1);
    display.upload(&gpu.queue, 2, tile, &[9000; 256]);
    assert_eq!(display.allocated_tiles(), 0);
}

/// A shader that draws the levels of one face, texel by texel.
const SHOW_WGSL: &str = "
@group(0) @binding(0) var<uniform> face: u32;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<u32> {
    return vec4<u32>(tile_level(i32(face), vec2<i32>(at.xy)), 0u, 0u, 0u);
}
";

/// A pipeline that draws from each map with the layout of `tiles_layout`.
struct Show {
    pipeline: wgpu::RenderPipeline,
    faces: Vec<wgpu::BindGroup>,
}

impl Show {
    fn new(gpu: &Gpu) -> Show {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let faces = (0..FACES as u32).map(|face| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 4,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.queue.write_buffer(&buffer, 0, &face.to_le_bytes());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        });
        let faces = faces.collect();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(format!("{TILES_WGSL}\n{SHOW_WGSL}").into()),
        });
        let tiles = tiles_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout), Some(&tiles)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::TextureFormat::R16Uint.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Show { pipeline, faces }
    }

    /// The levels that the shader reads from the bind group of a map, for
    /// each face of `n` by `n` texels.
    fn draw(&self, gpu: &Gpu, tiles: &wgpu::BindGroup, n: usize) -> Vec<Vec<u16>> {
        let row_bytes = (n * 2).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let size = wgpu::Extent3d {
            width: n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        };
        let mut enc = gpu.encoder();
        let buffers: Vec<_> = (0..FACES)
            .map(|face| {
                let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R16Uint,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = target.create_view(&wgpu::TextureViewDescriptor::default());
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations::default(),
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.faces[face], &[]);
                pass.set_bind_group(1, tiles, &[]);
                pass.draw(0..3, 0..1);
                drop(pass);
                let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: (row_bytes * n) as u64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                enc.copy_texture_to_buffer(
                    target.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(row_bytes as u32),
                            rows_per_image: None,
                        },
                    },
                    size,
                );
                buffer
            })
            .collect();
        gpu.submit(enc);
        let mapped: Vec<_> = buffers
            .iter()
            .map(|buffer| {
                let (sender, receiver) = mpsc::channel();
                buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                    sender.send(result).expect("the test waits");
                });
                receiver
            })
            .collect();
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the device answers");
        let levels = buffers.iter().zip(mapped).map(|(buffer, receiver)| {
            receiver
                .recv()
                .expect("the readback runs")
                .expect("the buffer maps");
            let bytes = buffer.get_mapped_range(..).expect("the buffer is mapped");
            let rows = bytes.chunks(row_bytes);
            let pairs = rows.flat_map(|row| row[..n * 2].as_chunks::<2>().0);
            pairs.map(|pair| u16::from_le_bytes(*pair)).collect()
        });
        levels.collect()
    }
}

/// The heightmap, a display map of the same size, and a display map of
/// another size and another base level go through one pipeline.
#[test]
#[ignore = "gpu"]
fn one_pipeline_draws_from_the_heightmap_and_from_each_display_map() {
    let gpu = gpu();
    let cpu = coded();
    let n = cpu.face_size();
    let heights = gpu.copy_with_tiles(&cpu, 16);
    let display = display_of(gpu, &cpu, 1);
    let mut other = Heightmap::with_tile_size(128, 16, 40_000);
    other.set(4, 100, 3, 77);
    other.set(1, 0, 127, 78);
    let other_display = display_of(gpu, &other, 1);

    let show = Show::new(gpu);
    assert_eq!(
        max_difference(&cpu, &show.draw(gpu, &heights.tiles(), n)),
        0
    );
    assert_eq!(
        max_difference(&cpu, &show.draw(gpu, &display.tiles(), n)),
        0
    );
    let levels = show.draw(gpu, &other_display.tiles(), 128);
    assert_eq!(max_difference(&other, &levels), 0);
    // A draw after an upload reads the new texels.
    display.upload(&gpu.queue, 1, full(n), &vec![555; n * n]);
    let levels = show.draw(gpu, &display.tiles(), n);
    assert!(levels[1].iter().all(|level| *level == 555));
}

/// The reset of a reader: the base level, then each tile that holds memory.
fn show(gpu: &Gpu, map: &GpuHeightmap, cpu: &mut Heightmap) {
    assert!(cpu.take_reset());
    map.clear(&gpu.queue, cpu.base());
    for (face, rect) in cpu.allocated_rects() {
        let mut data = Vec::new();
        cpu.read_rect(face, rect, &mut data);
        map.upload(&gpu.queue, face as u32, rect, &data);
    }
}

#[test]
#[ignore = "gpu"]
fn a_reset_shows_another_map_with_only_its_tiles() {
    let gpu = gpu();
    let n = 64;
    let mut first = Heightmap::with_tile_size(n, 16, 100);
    first.set(0, 3, 3, 900);
    first.set(2, 40, 20, 800);
    first.set(5, 63, 63, 700);
    let mut second = Heightmap::with_tile_size(n, 16, 300);
    second.set(2, 41, 21, 600);
    second.set(3, 10, 50, 500);
    let map = GpuHeightmap::with_tile_size(&gpu.device, n as u32, 16);

    show(gpu, &map, &mut first);
    assert_eq!(map.allocated_tiles(), 3);
    assert_eq!(max_difference(&first, &gpu.read_all(&map)), 0);

    // The maps were readers of their own state before, so they have no reset.
    second.take_reset();
    for (cpu, tiles) in [(&mut second, 2), (&mut first, 3)] {
        assert!(!cpu.take_reset());
        cpu.mark_reset();
        show(gpu, &map, cpu);
        assert_eq!(map.allocated_tiles(), tiles);
        assert_eq!(max_difference(cpu, &gpu.read_all(&map)), 0);
    }
}

/// The stamps of each mode on small tiles, across tile edges, a face edge,
/// and a cube corner. Some of the tiles under the stamps have no memory. The
/// levels do not depend on the tile size, so a tile edge is no seam.
#[test]
#[ignore = "gpu"]
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
#[ignore = "gpu"]
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

fn erode_brush(lon: f64, lat: f64, radius: f64) -> ErodeBrush {
    ErodeBrush {
        center: lonlat_to_dir(lon, lat),
        radius,
        hardness: 0.3,
        flow: 0.8,
    }
}

/// Applies steps of the erode brush to a copy of `cpu` on the GPU and to
/// `cpu`, with one submit for each step. Returns the largest difference
/// between a GPU texel and a CPU texel, and the number of texels that changed
/// on each face.
fn erode_run(
    cpu: &mut Heightmap,
    erosion: &mut Erosion,
    brushes: &[ErodeBrush],
    steps: usize,
) -> (u16, [usize; FACES]) {
    let gpu = gpu();
    let map = gpu.copy_of(cpu);
    let before = levels(cpu);
    for _ in 0..steps {
        let step = erosion.step(brushes, 1.0);
        assert!(!step.is_empty());
        let mut enc = gpu.encoder();
        assert!(map.erode(&gpu.queue, &mut enc, &step));
        gpu.submit(enc);
        map.begin_batch();
        assert!(cpu.erode(&step) > 0);
    }
    let difference = max_difference(cpu, &gpu.read_all(&map));
    let (after, n) = (levels(cpu), cpu.face_size());
    let changed = std::array::from_fn(|face| {
        let texels = face * n * n..(face + 1) * n * n;
        changes(&before[texels.clone()], &after[texels])
    });
    (difference, changed)
}

/// An erosion of a window on `window_land` with cells of 2 texels. The window
/// is on face 0, and it covers a part of face 2.
fn window_erosion(cpu: &Heightmap) -> Erosion {
    let window = Window::centered(lonlat_to_dir(40.0, 3.0), WINDOW_FACE, 2, WINDOW_CELLS);
    assert_eq!(window.face, 0);
    let global = FlowMap::new(&CoarseHeights::new(cpu));
    Erosion::window(&WindowHeights::new(cpu, window), &global)
}

/// A cell of the global grid is 2 texels wide, and the brushes are on both
/// sides of a face edge. Thus the test covers the mix of the cells and the
/// cells past the edge.
#[test]
#[ignore = "gpu"]
fn gpu_erode_matches_cpu_for_a_global_step() {
    let mut cpu = window_land();
    let coarse = CoarseHeights::new(&cpu);
    assert_eq!(2 * coarse.size(), cpu.face_size());
    let mut erosion = Erosion::global(&coarse, cpu.face_size());
    let brushes = [erode_brush(41.0, 3.0, 0.1), erode_brush(49.0, 6.0, 0.1)];
    let (difference, changed) = erode_run(&mut cpu, &mut erosion, &brushes, 3);
    eprintln!("erode on the global grid, largest difference: {difference}, {changed:?}");
    assert!(changed[0] > 1000 && changed[2] > 1000, "{changed:?}");
    assert!(difference <= TOLERANCE, "{difference}");

    // A step with no free slot adds nothing.
    let gpu = gpu();
    let map = gpu.copy_of(&cpu);
    let step = erosion.step(&brushes, 1.0);
    let passes = step.rects().iter().flatten().count();
    assert!(passes >= 2);
    let mut enc = gpu.encoder();
    let mut steps = 0;
    while map.erode(&gpu.queue, &mut enc, &step) {
        steps += 1;
        assert!(steps * passes <= STAMP_SLOTS as usize);
    }
    assert_eq!(steps, STAMP_SLOTS as usize / passes);
    map.begin_batch();
    assert!(map.erode(&gpu.queue, &mut gpu.encoder(), &step));
}

/// The cells of the window of the brush are 1 texel wide, and the water of a
/// window with cells of 2 texels comes in.
#[test]
#[ignore = "gpu"]
fn gpu_erode_matches_cpu_for_a_step_on_the_window_of_a_brush() {
    let mut cpu = window_land();
    let view = Window::centered(lonlat_to_dir(40.0, 3.0), WINDOW_FACE, 2, WINDOW_CELLS);
    let global = FlowMap::new(&CoarseHeights::new(&cpu));
    let outer = OuterFlow::window(&WindowHeights::new(&cpu, view), &global);
    let brush = erode_brush(41.0, 3.0, 0.05);
    let window = Window::centered(brush.center, WINDOW_FACE, 1, WINDOW_CELLS);
    let mut erosion = Erosion::window_in(&WindowHeights::new(&cpu, window), &outer);
    let (difference, changed) = erode_run(&mut cpu, &mut erosion, &[brush], 3);
    eprintln!("erode on the window of a brush, largest difference: {difference}, {changed:?}");
    assert!(changed.iter().sum::<usize>() > 100, "{changed:?}");
    assert!(difference <= TOLERANCE, "{difference}");
}

/// The ground under the brush is below sea level, and it goes down as the
/// land does.
#[test]
#[ignore = "gpu"]
fn gpu_erode_matches_cpu_below_sea_level() {
    let mut cpu = window_land();
    let sea = vale_terrain::meters_to_level(vale_terrain::SEA_LEVEL);
    let before = levels(&cpu);
    let mut erosion = Erosion::global(&CoarseHeights::new(&cpu), cpu.face_size());
    let brush = erode_brush(125.0, -15.0, 0.1);
    assert!(cpu.sample(brush.center) < sea);
    let (difference, changed) = erode_run(&mut cpu, &mut erosion, &[brush], 3);
    eprintln!("erode below sea level, largest difference: {difference}, {changed:?}");
    let lowered = before.iter().zip(levels(&cpu));
    let lowered = lowered
        .filter(|&(&old, new)| old < sea && new < old)
        .count();
    assert!(lowered > 1000, "{lowered}");
    assert!(difference <= TOLERANCE, "{difference}");
}

#[test]
#[ignore = "gpu"]
fn gpu_erode_in_a_window_matches_cpu() {
    let mut cpu = window_land();
    let mut erosion = window_erosion(&cpu);
    let (difference, changed) =
        erode_run(&mut cpu, &mut erosion, &[erode_brush(30.0, 3.0, 0.05)], 3);
    eprintln!("erode in a window, largest difference: {difference}, {changed:?}");
    assert!(changed[0] > 1000, "{changed:?}");
    assert_eq!(changed.iter().sum::<usize>(), changed[0]);
    assert!(difference <= TOLERANCE, "{difference}");
}

/// On the next face, the place of a texel in the window has a small error on
/// the GPU. The differences of the steps can add, so the test has one step.
#[test]
#[ignore = "gpu"]
fn gpu_erode_in_a_window_matches_cpu_past_a_face_edge() {
    let mut cpu = window_land();
    let mut erosion = window_erosion(&cpu);
    let (difference, changed) =
        erode_run(&mut cpu, &mut erosion, &[erode_brush(53.0, 3.0, 0.05)], 1);
    eprintln!("erode in a window past a face edge, largest difference: {difference}, {changed:?}");
    assert!(changed[2] > 1000, "{changed:?}");
    assert_eq!(changed.iter().sum::<usize>(), changed[2]);
    assert!(difference <= TOLERANCE, "{difference}");
}

/// The same for one warp stamp. The shader finds the offset place with
/// 32-bit numbers, so a level near a half step can round to the other side.
/// The largest difference in the tests of this file is 1.
const WARP_TOLERANCE: u16 = 1;

/// The same for a stroke of warp stamps. Each stamp reads the results of the
/// stamps before it, so a difference of one step can grow to two. The
/// largest difference in the strokes of this file is 1.
const WARP_STROKE_TOLERANCE: u16 = 2;

fn warp(center: V3, radius: f64, hardness: f64) -> Stamp {
    Stamp {
        strength: 0.5 * radius,
        scale: 0.5 * radius,
        seed: 7,
        ..stamp(center, radius, hardness, Mode::Warp)
    }
}

/// A heightmap with a level for each texel from `level`.
fn ground(n: usize, mut level: impl FnMut(V3) -> u16) -> Heightmap {
    let mut map = Heightmap::new(n, 0);
    for face in 0..FACES {
        let mut data = Vec::with_capacity(n * n);
        for y in 0..n {
            data.extend((0..n).map(|x| level(map.texel_dir(face, x, y))));
        }
        map.store_rect(face, full(n), &data);
    }
    map
}

/// Ground that is a smooth function of the direction, so it has no step at
/// a face edge. The steepest slope is 55000 levels for each radian.
fn smooth_ground(n: usize) -> Heightmap {
    ground(n, |d| {
        let hill = 9000.0 * (3.0 * d[0] + 1.0).sin() * (2.0 * d[1]).cos();
        (30000.0 + hill + 6000.0 * (4.0 * d[2] + d[0]).sin()).round() as u16
    })
}

/// Ground with a level for each texel that has no relation to the levels of
/// the texels around it.
fn rough_ground(n: usize) -> Heightmap {
    let mut state = 12345u32;
    ground(n, |_| {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state >> 16) as u16
    })
}

/// A face center, a face edge, a cube corner, and a pole.
const WARP_PLACES: [(&str, f64, f64); 4] = [
    ("face center", 1.3, 0.7),
    ("face edge", 45.0, 0.0),
    ("cube corner", 45.0, 35.264),
    ("pole", 0.0, 90.0),
];

#[test]
#[ignore = "gpu"]
fn gpu_warp_matches_cpu_for_one_stamp() {
    let mut worst = 0;
    for (place, lon, lat) in WARP_PLACES {
        // A brush of 3 texel steps has no offset, so the small brush gets a
        // large face.
        for (n, radius) in [(1024, 0.01), (256, 0.06), (256, 0.3)] {
            for hardness in [0.0, 0.5, 0.95] {
                let mut cpu = smooth_ground(n);
                let before = levels(&cpu);
                let stamp = warp(lonlat_to_dir(lon, lat), radius, hardness);
                let difference = run(&mut cpu, &[stamp]).difference;
                assert!(levels(&cpu) != before, "{place}, radius {radius}");
                assert!(
                    difference <= WARP_TOLERANCE,
                    "warp at the {place}, radius {radius}, hardness {hardness}: \
                     the difference is {difference}"
                );
                worst = worst.max(difference);
            }
        }
    }
    eprintln!("one warp stamp, largest difference: {worst}");
}

/// A stroke of warp stamps that overlap, across a face edge and near a cube
/// corner. Each stamp has its own seed.
fn warp_stroke(lat: f64, radius: f64, hardness: f64) -> Vec<Stamp> {
    let count = 64;
    (0..count)
        .map(|i| {
            let lon = 33.0 + 24.0 * f64::from(i) / f64::from(count);
            Stamp {
                seed: i,
                ..warp(lonlat_to_dir(lon, lat), radius, hardness)
            }
        })
        .collect()
}

#[test]
#[ignore = "gpu"]
fn gpu_warp_matches_cpu_for_a_stroke() {
    let mut worst = 0;
    for (lat, radius, hardness) in [(4.0, 0.07, 0.5), (35.0, 0.12, 0.0), (20.0, 0.05, 0.95)] {
        let mut cpu = smooth_ground(256);
        let before = levels(&cpu);
        let difference = run(&mut cpu, &warp_stroke(lat, radius, hardness)).difference;
        assert!(changes(&before, &levels(&cpu)) > 300, "latitude {lat}");
        assert!(
            difference <= WARP_STROKE_TOLERANCE,
            "warp stroke at latitude {lat}: the difference is {difference}"
        );
        worst = worst.max(difference);
    }
    eprintln!("warp stroke, largest difference: {worst}");
}

/// On a large face, the place of a texel needs most of the digits of a
/// 32-bit number.
#[test]
#[ignore = "gpu"]
fn gpu_warp_matches_cpu_on_a_large_face() {
    let mut cpu = smooth_ground(2048);
    let mut stamps = Vec::new();
    for (i, radius) in [0.004, 0.011, 0.05].into_iter().enumerate() {
        for step in 0..6 {
            let lon = 44.9 + 0.03 * f64::from(step);
            let center = lonlat_to_dir(lon, 0.3 * i as f64 + 0.01 * f64::from(step));
            stamps.push(warp(center, radius, 0.5));
        }
    }
    // A large brush at a cube corner.
    stamps.push(warp(lonlat_to_dir(44.0, 34.0), 0.3, 0.5));
    let before = levels(&cpu);
    let difference = run(&mut cpu, &stamps).difference;
    assert!(changes(&before, &levels(&cpu)) > 100_000);
    eprintln!("warp on a large face, largest difference: {difference}");
    assert!(difference <= WARP_STROKE_TOLERANCE, "{difference}");
}

#[test]
#[ignore = "gpu"]
fn gpu_warp_leaves_flat_ground_flat() {
    let n = 256;
    let mut cpu = Heightmap::new(n, 30000);
    let mut stamps = warp_stroke(35.0, 0.12, 0.5);
    for (_, lon, lat) in WARP_PLACES {
        stamps.push(Stamp {
            strength: 0.3,
            ..warp(lonlat_to_dir(lon, lat), 0.3, 0.5)
        });
    }
    let faces = run(&mut cpu, &stamps).faces;
    assert_eq!(faces.len(), FACES);
    for levels in faces {
        assert!(levels == vec![30000; n * n]);
    }
}

#[test]
#[ignore = "gpu"]
fn gpu_warped_level_is_between_the_levels_under_the_brush() {
    let n = 256;
    for (place, lon, lat) in WARP_PLACES {
        for radius in [0.06, 0.3] {
            for hardness in [0.0, 0.5, 0.95] {
                let name = format!("{place}, radius {radius}, hardness {hardness}");
                let mut cpu = rough_ground(n);
                let before = levels(&cpu);
                let center = lonlat_to_dir(lon, lat);
                let stamp = Stamp {
                    strength: radius,
                    ..warp(center, radius, hardness)
                };
                // The texels under the brush.
                let mut under = Vec::with_capacity(before.len());
                for face in 0..FACES {
                    for i in 0..n * n {
                        let dir = cpu.texel_dir(face, i % n, i / n);
                        under.push(dot(center, dir) > radius.cos());
                    }
                }
                let old = || before.iter().zip(&under).filter(|(_, under)| **under);
                let lo = old().map(|(level, _)| *level).min().unwrap();
                let hi = old().map(|(level, _)| *level).max().unwrap();
                let after = run(&mut cpu, &[stamp]).faces.concat();
                assert!(changes(&before, &after) > 100, "{name}");
                for (i, &level) in after.iter().enumerate() {
                    if under[i] {
                        assert!(lo <= level && level <= hi, "{name}");
                    } else {
                        assert_eq!(level, before[i], "{name}");
                    }
                }
            }
        }
    }
}

/// Warps smooth ground, then checks lines across the edges of the even faces
/// in `edges`, `m` texels along each edge.
fn warp_and_check(stamps: &[Stamp], edges: &[usize], lines: &[usize]) {
    let n = 256;
    let mut cpu = smooth_ground(n);
    let before = levels(&cpu);
    let Run {
        faces, difference, ..
    } = run(&mut cpu, stamps);
    for &face in edges {
        for &m in lines {
            let line = line_across_edge(n, face, m, 12);
            let on_gpu: Vec<i32> = line
                .iter()
                .map(|&(face, x, y)| i32::from(faces[face][y * n + x]))
                .collect();
            let on_cpu: Vec<i32> = line
                .iter()
                .map(|&(face, x, y)| i32::from(cpu.get(face, x as i64, y as i64)))
                .collect();
            let old = line
                .iter()
                .map(|&(face, x, y)| i32::from(before[(face * n + y) * n + x]));
            // The stamps changed most texels of the line.
            let changed = old.zip(&on_cpu).filter(|(old, new)| old != *new).count();
            assert!(changed > 12, "edge of face {face}, line {m}: {changed}");
            assert_no_seam(&format!("CPU, edge of face {face}"), &on_cpu, false);
            assert_no_seam(&format!("GPU, edge of face {face}"), &on_gpu, false);
        }
    }
    assert!(
        difference <= WARP_STROKE_TOLERANCE,
        "the difference is {difference}"
    );
}

#[test]
#[ignore = "gpu"]
fn warp_leaves_no_seam_at_a_face_edge() {
    // Face 0 and face 2 meet at longitude 45.
    let stamps: Vec<Stamp> = (0..16)
        .map(|i| Stamp {
            seed: i,
            ..warp(lonlat_to_dir(41.0 + 0.5 * f64::from(i), 2.0), 0.15, 0.5)
        })
        .collect();
    warp_and_check(&stamps, &[0], &[118, 126, 134, 142]);
}

#[test]
#[ignore = "gpu"]
fn warp_leaves_no_seam_at_a_cube_corner() {
    // Faces 0, 2, and 4 meet at this corner.
    let corner = lonlat_to_dir(45.0, 35.264);
    let stamps: Vec<Stamp> = (0..4)
        .map(|seed| Stamp {
            seed,
            ..warp(corner, 0.2, 0.5)
        })
        .collect();
    warp_and_check(&stamps, &[0, 2, 4], &[236, 243, 249]);
}

/// At a cube corner, each of three faces takes levels from the other two.
/// The CPU reads the levels from before the stamp on each face.
#[test]
#[ignore = "gpu"]
fn gpu_warp_reads_no_level_that_it_wrote() {
    let n = 256;
    let mut cpu = smooth_ground(n);
    let before = levels(&cpu);
    let stamp = Stamp {
        strength: 0.3,
        ..warp(lonlat_to_dir(45.0, 35.264), 0.3, 0.5)
    };
    let difference = run(&mut cpu, &[stamp]).difference;
    let after = levels(&cpu);
    for face in [0, 2, 4] {
        let texels = face * n * n..(face + 1) * n * n;
        let changed = changes(&before[texels.clone()], &after[texels]);
        assert!(changed > 1000, "face {face}: {changed}");
    }
    assert!(
        difference <= WARP_TOLERANCE,
        "the difference is {difference}"
    );
}

/// Asserts that no level went from above the floor to below it, and that no
/// level at or below the floor went down. The ceiling has the same rule for
/// the other direction.
fn assert_inside(limits: Limits, before: &[u16], after: &[u16], name: &str) {
    for (new, old) in after.iter().zip(before) {
        assert!(*new >= (*old).min(limits.floor), "{name}: {old} {new}");
        assert!(*new <= (*old).max(limits.ceiling), "{name}: {old} {new}");
    }
}

/// Applies the stamps with the limits to the ground of `ground`, on the GPU
/// and on the CPU. The limits must hold on both, and they must change the
/// result of the CPU.
fn run_with_limits(
    ground: impl Fn() -> Heightmap,
    stamps: &[Stamp],
    limits: Limits,
    name: &str,
) -> Run {
    let mut free = ground();
    for stamp in stamps {
        free.stamp(stamp);
    }
    let limited: Vec<Stamp> = stamps
        .iter()
        .map(|&stamp| Stamp { limits, ..stamp })
        .collect();
    let mut cpu = ground();
    let before = levels(&cpu);
    let run = run(&mut cpu, &limited);
    let after = levels(&cpu);
    assert!(after != levels(&free), "{name}: the limits change nothing");
    assert_inside(limits, &before, &after, name);
    assert_inside(limits, &before, &run.faces.concat(), name);
    run
}

/// Limits inside the range of the levels of `hills`.
const HILL_LIMITS: Limits = Limits {
    floor: 28_000,
    ceiling: 36_000,
};

#[test]
#[ignore = "gpu"]
fn gpu_matches_cpu_with_limits_for_a_stroke() {
    let mut worst = [0; 4];
    for (m, mode) in MODES.into_iter().enumerate() {
        for (lat, radius, hardness) in [(4.0, 0.07, 0.5), (35.0, 0.12, 0.0)] {
            let count = STAMP_SLOTS as usize / 2;
            let stamps: Vec<Stamp> = (0..count)
                .map(|i| {
                    let lon = 33.0 + 24.0 * i as f64 / count as f64;
                    stamp(lonlat_to_dir(lon, lat), radius, hardness, mode)
                })
                .collect();
            let name = format!("{mode:?} stroke at latitude {lat}");
            let difference = run_with_limits(|| hills(256), &stamps, HILL_LIMITS, &name).difference;
            assert!(
                difference <= tolerance(mode),
                "{name}: the difference is {difference}"
            );
            worst[m] = worst[m].max(difference);
        }
    }
    eprintln!("stroke with limits, largest difference for {MODES:?}: {worst:?}");
}

#[test]
#[ignore = "gpu"]
fn gpu_weather_with_limits_matches_cpu_and_keeps_the_sum() {
    let sum = |levels: &[u16]| -> u64 { levels.iter().map(|&l| u64::from(l)).sum() };
    let before = levels(&hills(256));
    let mut worst = 0;
    for (lat, radius, hardness) in [(4.0, 0.07, 0.5), (35.0, 0.12, 0.0)] {
        let stamps = weather_stroke(lat, radius, hardness);
        let name = format!("weather stroke at latitude {lat}");
        let Run {
            faces, difference, ..
        } = run_with_limits(|| hills(256), &stamps, HILL_LIMITS, &name);
        assert!(
            difference <= WEATHER_TOLERANCE,
            "{name}: the difference is {difference}"
        );
        assert_eq!(sum(&faces.concat()), sum(&before), "{name}");
        worst = worst.max(difference);
    }
    eprintln!("weather stroke with limits, largest difference: {worst}");
}

/// The floor is in the middle of the largest drop of the stamps with no
/// limits, so it stops the texel with that drop.
fn floor_in_the_largest_drop(before: &[u16], after: &[u16]) -> Limits {
    let drops = before.iter().zip(after);
    let (old, new) = drops
        .max_by_key(|&(old, new)| old.saturating_sub(*new))
        .expect("the map has texels");
    assert!(old - new >= 2, "the largest drop is {}", old - new);
    Limits {
        floor: old - (old - new) / 2,
        ..Limits::default()
    }
}

#[test]
#[ignore = "gpu"]
fn gpu_ridge_with_a_ceiling_matches_cpu() {
    let before = levels(&river_land(256));
    let stamps = stroke((31.0, 2.0), (59.0, 5.0), 240, 0.05, Mode::Ridge);
    let mut free = river_land(256);
    for stamp in &stamps {
        free.stamp(stamp);
    }
    // The ceiling is in the middle of the largest rise of the stamps with no
    // limits, so it stops the texel with that rise.
    let rises = before.iter().zip(levels(&free));
    let (old, new) = rises
        .max_by_key(|&(old, new)| new - old)
        .expect("the map has texels");
    assert!(new - old >= 2, "the largest rise is {}", new - old);
    let limits = Limits {
        ceiling: old + (new - old) / 2,
        ..Limits::default()
    };
    let stroke = run_with_limits(|| river_land(256), &stamps, limits, "stroke");
    assert!(
        stroke.difference <= ADD_STROKE_TOLERANCE,
        "{}",
        stroke.difference
    );
    let after = stroke.faces.concat();
    let at_ceiling = after.iter().zip(&before);
    let at_ceiling =
        at_ceiling.filter(|&(new, old)| *old < limits.ceiling && *new == limits.ceiling);
    assert!(at_ceiling.count() > 0);
}

#[test]
#[ignore = "gpu"]
fn gpu_carve_with_limits_matches_cpu() {
    let before = levels(&river_land(256));
    let carved = |stamps: &[Stamp]| {
        let mut free = river_land(256);
        for stamp in stamps {
            free.stamp(stamp);
        }
        floor_in_the_largest_drop(&before, &levels(&free))
    };

    let stamps = [stamp(lonlat_to_dir(45.0, 35.264), 0.3, 0.0, Mode::Carve)];
    let one = run_with_limits(|| river_land(256), &stamps, carved(&stamps), "one stamp");
    assert!(one.difference <= TOLERANCE, "{}", one.difference);

    // Many stamps touch each texel, so the ground at some rivers goes down
    // to the floor.
    let stamps = stroke((31.0, 2.0), (59.0, 5.0), 240, 0.05, Mode::Carve);
    let limits = carved(&stamps);
    let stroke = run_with_limits(|| river_land(256), &stamps, limits, "stroke");
    assert!(
        stroke.difference <= ADD_STROKE_TOLERANCE,
        "{}",
        stroke.difference
    );
    let after = stroke.faces.concat();
    let at_floor = after.iter().zip(&before);
    let at_floor = at_floor.filter(|&(new, old)| *old > limits.floor && *new == limits.floor);
    assert!(at_floor.count() > 20);
    eprintln!(
        "carve with a floor, largest differences: {} {}",
        one.difference, stroke.difference
    );
}

#[test]
#[ignore = "gpu"]
fn gpu_warp_with_limits_matches_cpu() {
    // The limits are on each side of the level at the brush center.
    let around = |lon: f64, lat: f64| {
        let middle = smooth_ground(256).sample(lonlat_to_dir(lon, lat));
        Limits {
            floor: middle - 800,
            ceiling: middle + 800,
        }
    };
    let mut worst = 0;
    for (place, lon, lat) in WARP_PLACES {
        let stamps = [warp(lonlat_to_dir(lon, lat), 0.3, 0.5)];
        let name = format!("warp at the {place}");
        let run = run_with_limits(|| smooth_ground(256), &stamps, around(lon, lat), &name);
        assert!(
            run.difference <= WARP_TOLERANCE,
            "{name}: the difference is {}",
            run.difference
        );
        worst = worst.max(run.difference);
    }
    let stamps = warp_stroke(35.0, 0.12, 0.0);
    let limits = around(45.0, 35.0);
    let stroke = run_with_limits(|| smooth_ground(256), &stamps, limits, "warp stroke");
    assert!(
        stroke.difference <= WARP_STROKE_TOLERANCE,
        "warp stroke: the difference is {}",
        stroke.difference
    );
    eprintln!(
        "warp with limits, largest differences: {worst} {}",
        stroke.difference
    );
}

#[test]
#[ignore = "gpu"]
fn gpu_erode_with_a_floor_matches_cpu() {
    let brushes = [erode_brush(41.0, 3.0, 0.1), erode_brush(49.0, 6.0, 0.1)];
    let erosion = |cpu: &Heightmap| Erosion::global(&CoarseHeights::new(cpu), cpu.face_size());
    let before = levels(&window_land());
    let mut free = window_land();
    let mut free_erosion = erosion(&free);
    for _ in 0..3 {
        free.erode(&free_erosion.step(&brushes, 1.0));
    }
    let free = levels(&free);
    let limits = floor_in_the_largest_drop(&before, &free);

    let mut cpu = window_land();
    let mut erosion = erosion(&cpu);
    erosion.set_limits(limits);
    let (difference, changed) = erode_run(&mut cpu, &mut erosion, &brushes, 3);
    eprintln!("erode with a floor, largest difference: {difference}, {changed:?}");
    assert!(changed[0] > 1000 && changed[2] > 1000, "{changed:?}");
    assert!(difference <= TOLERANCE, "{difference}");
    let after = levels(&cpu);
    assert_inside(limits, &before, &after, "erode");
    let stopped = before.iter().zip(&after).zip(&free);
    let stopped = stopped.filter(|&((old, new), free)| {
        *old > limits.floor && *new == limits.floor && *free < limits.floor
    });
    assert!(stopped.count() > 20);
}

/// A change of the limits ends a group. With one pair of limits for all
/// stamps, the GPU result is far from the CPU result.
#[test]
#[ignore = "gpu"]
fn a_group_has_one_pair_of_limits() {
    let center = lonlat_to_dir(44.0, 34.0);
    let low = Limits {
        ceiling: hills(256).sample(center) + 1000,
        ..Limits::default()
    };
    let free = Limits::default();
    let stamps_of = |limits: [Limits; 6]| -> Vec<Stamp> {
        let stamps = limits.into_iter().enumerate().map(|(i, limits)| Stamp {
            limits,
            ..stamp(
                lonlat_to_dir(44.0 + 0.1 * i as f64, 34.0),
                0.05,
                0.5,
                Mode::Raise,
            )
        });
        stamps.collect()
    };
    let stamped = |limits: [Limits; 6]| {
        let mut cpu = hills(256);
        for stamp in stamps_of(limits) {
            cpu.stamp(&stamp);
        }
        levels(&cpu)
    };
    let mut cpu = hills(256);
    let GroupRun {
        groups, difference, ..
    } = run_groups(&mut cpu, &stamps_of([low, low, free, free, free, low]));
    assert_eq!(groups, [2, 3, 1]);
    assert!(difference <= ADD_STROKE_TOLERANCE, "{difference}");
    let after = levels(&cpu);
    assert!(after != stamped([low; 6]) && after != stamped([free; 6]));
}

/// The levels from 1 up of a pyramid on the GPU, face by face.
fn read_levels(gpu: &Gpu, pyramid: &GpuPyramid) -> Vec<Vec<Vec<u16>>> {
    (1..=pyramid.max_level())
        .map(|k| gpu.read_with(|enc, face| pyramid.read_level(&gpu.device, enc, k, face)))
        .collect()
}

/// The same levels on the CPU.
fn cpu_levels(cpu: &Heightmap) -> Vec<Vec<Vec<u16>>> {
    let pyramid = Pyramid::new(cpu);
    (1..=pyramid.max_level())
        .map(|k| (0..FACES).map(|f| pyramid.face(k, f).to_vec()).collect())
        .collect()
}

/// A map in tiles with the texels of `cpu`, and its pyramid after a full
/// update.
fn tiles_and_pyramid(gpu: &Gpu, cpu: &Heightmap, tile: usize) -> (GpuTiles, GpuPyramid) {
    let n = cpu.face_size();
    let tiles = GpuTiles::with_tile_size(&gpu.device, n as u32, tile, cpu.base());
    for face in 0..FACES {
        let mut data = Vec::new();
        cpu.read_rect(face, full(n), &mut data);
        tiles.upload(&gpu.queue, face as u32, full(n), &data);
    }
    let pyramid = GpuPyramid::new(&gpu.device, n as u32);
    let mut enc = gpu.encoder();
    pyramid.update(&mut enc, &tiles.tiles());
    gpu.submit(enc);
    (tiles, pyramid)
}

#[test]
#[ignore = "gpu"]
fn gpu_levels_match_the_cpu_levels() {
    let gpu = gpu();
    for (n, tile) in [(64, 16), (44, 16), (8, 256)] {
        let cpu = hills(n);
        let (_tiles, pyramid) = tiles_and_pyramid(gpu, &cpu, tile);
        let levels = read_levels(gpu, &pyramid);
        assert!(!levels.is_empty());
        assert!(levels == cpu_levels(&cpu), "face size {n}");
    }
}

#[test]
#[ignore = "gpu"]
fn a_cleared_pyramid_holds_the_base_level() {
    let gpu = gpu();
    let pyramid = GpuPyramid::new(&gpu.device, 32);
    let mut enc = gpu.encoder();
    pyramid.clear(&mut enc, 4321);
    gpu.submit(enc);
    let levels = read_levels(gpu, &pyramid);
    assert_eq!(levels.len(), 3);
    for (k, faces) in levels.iter().enumerate() {
        let m = 32 >> (k + 1);
        assert!(faces.iter().all(|f| *f == vec![4321; m * m]));
    }
}

#[test]
#[ignore = "gpu"]
fn an_update_after_stamps_matches_a_full_build() {
    let gpu = gpu();
    let mut cpu = hills(64);
    let map = gpu.copy_of(&cpu);
    let pyramid = GpuPyramid::new(&gpu.device, 64);
    let mut enc = gpu.encoder();
    pyramid.update(&mut enc, &map.tiles());
    gpu.submit(enc);
    let before = read_levels(gpu, &pyramid);

    // A stroke across the edge of faces 0 and 2, and over the corner of
    // faces 0, 2, and 4, and one stamp inside face 1.
    let stamps: Vec<_> = [(45.0, 5.0), (44.0, 20.0), (45.0, 35.26), (-163.0, -12.0)]
        .iter()
        .map(|&(lon, lat)| stamp(lonlat_to_dir(lon, lat), 0.08, 1.0, Mode::Flatten))
        .collect();
    let old = gpu.read_all(&map);
    gpu.stamp(&map, &mut cpu, &stamps);
    // Mark the smallest rectangle of the changed texels of each face, so a
    // level that misses a texel of it shows.
    let mut marked = 0;
    for (face, (old, new)) in old.iter().zip(&gpu.read_all(&map)).enumerate() {
        let changed: Vec<_> = (0..64 * 64).filter(|&i| old[i] != new[i]).collect();
        if changed.is_empty() {
            continue;
        }
        let xs = changed.iter().map(|i| i % 64);
        let ys = changed.iter().map(|i| i / 64);
        let rect = TexelRect {
            x0: xs.clone().min().unwrap(),
            y0: ys.clone().min().unwrap(),
            x1: xs.max().unwrap() + 1,
            y1: ys.max().unwrap() + 1,
        };
        pyramid.mark(face as u32, rect);
        marked += 1;
    }
    assert!(marked >= 4);
    let mut enc = gpu.encoder();
    pyramid.update(&mut enc, &map.tiles());
    gpu.submit(enc);
    let after = read_levels(gpu, &pyramid);
    assert!(after != before);

    let mut read = Heightmap::new(64, 0);
    for (face, levels) in gpu.read_all(&map).iter().enumerate() {
        read.store_rect(face, full(64), levels);
    }
    assert!(after == cpu_levels(&read));
}

/// A shader that writes `field` at a list of points, one pixel each.
const FIELD_TEST_WGSL: &str = "
struct Params {
    n: u32,
    max_level: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
// The direction and the lod of each pixel.
@group(0) @binding(1) var<storage, read> points: array<vec4<f32>>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let at = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(at * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let p = points[u32(at.x)];
    return vec4<f32>(field(p.xyz, params.n, p.w, params.max_level).x, 0.0, 0.0, 1.0);
}
";

fn buffer(gpu: &Gpu, usage: wgpu::BufferUsages, bytes: &[u8]) -> wgpu::Buffer {
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes.len() as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue.write_buffer(&buffer, 0, bytes);
    buffer
}

fn fragment_buffer(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// The values of `field` in the shader at each point: a direction and a lod.
fn shader_field(
    gpu: &Gpu,
    tiles: &GpuTiles,
    pyramid: &GpuPyramid,
    points: &[[f32; 4]],
) -> Vec<f32> {
    let device = &gpu.device;
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            fragment_buffer(0, wgpu::BufferBindingType::Uniform),
            fragment_buffer(1, wgpu::BufferBindingType::Storage { read_only: true }),
        ],
    });
    let params: Vec<u8> = [pyramid.face_size(), pyramid.max_level()]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let params = buffer(gpu, wgpu::BufferUsages::UNIFORM, &params);
    let bytes: Vec<u8> = points
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let points_buffer = buffer(gpu, wgpu::BufferUsages::STORAGE, &bytes);
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: points_buffer.as_entire_binding(),
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(
            format!("{TILES_WGSL}\n{FIELD_WGSL}\n{FIELD_TEST_WGSL}").into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[
            Some(&layout),
            Some(&tiles_layout(device)),
            Some(&pyramid_layout(device)),
        ],
        immediate_size: 0,
    });
    let format = wgpu::TextureFormat::R32Float;
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(format.into())],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let size = wgpu::Extent3d {
        width: points.len() as u32,
        height: 1,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let row_bytes =
        (points.len() * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: row_bytes as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.encoder();
    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations::default(),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.set_bind_group(1, &tiles.tiles(), &[]);
    pass.set_bind_group(2, pyramid.bind_group(), &[]);
    pass.draw(0..3, 0..1);
    drop(pass);
    enc.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &out,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes as u32),
                rows_per_image: None,
            },
        },
        size,
    );
    gpu.submit(enc);
    let (sender, receiver) = mpsc::channel();
    out.map_async(wgpu::MapMode::Read, .., move |result| {
        sender.send(result).expect("the test waits");
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device answers");
    receiver
        .recv()
        .expect("the readback runs")
        .expect("the buffer maps");
    let bytes = out.get_mapped_range(..).expect("the buffer is mapped");
    let values = bytes[..points.len() * 4].as_chunks::<4>().0;
    values.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

/// Directions at the face middles, on and next to the face edges, at and
/// next to the cube corners, and at other places.
fn test_directions() -> Vec<V3> {
    let unit = |d: V3| {
        let l = dot(d, d).sqrt();
        [d[0] / l, d[1] / l, d[2] / l]
    };
    let mut dirs = Vec::new();
    for axis in 0..3 {
        for sign in [1.0, -1.0] {
            let mut d = [0.0; 3];
            d[axis] = sign;
            dirs.push(d);
        }
        for (a, b) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            for t in [0.0, 0.37, -0.81] {
                let mut d = [0.0; 3];
                d[axis] = t;
                d[(axis + 1) % 3] = a;
                d[(axis + 2) % 3] = b;
                dirs.push(unit(d));
                d[(axis + 1) % 3] = a * 1.004;
                dirs.push(unit(d));
            }
        }
    }
    for i in 0..8 {
        let s = |bit: usize| if i & bit == 0 { 1.0 } else { -1.0 };
        dirs.push(unit([s(1), s(2), s(4)]));
        dirs.push(unit([s(1) * 1.01, s(2), s(4) * 0.995]));
    }
    for i in 0..20 {
        let (lon, lat) = (i as f64 * 37.0 - 180.0, (i as f64 * 1.7).sin() * 80.0);
        dirs.push(lonlat_to_dir(lon, lat));
    }
    dirs
}

#[test]
#[ignore = "gpu"]
fn the_shader_field_matches_the_cpu_field() {
    let gpu = gpu();
    let cpu = hills(64);
    let (tiles, pyramid) = tiles_and_pyramid(gpu, &cpu, 16);
    let lods = [-1.0, 0.0, 0.3, 1.0, 1.5, 2.75, 4.0, 7.0];
    let mut points = Vec::new();
    for d in test_directions() {
        for lod in lods {
            points.push([d[0] as f32, d[1] as f32, d[2] as f32, lod]);
        }
    }
    let values = shader_field(gpu, &tiles, &pyramid, &points);
    let field = Pyramid::new(&cpu);
    let all: Vec<u16> = (0..FACES).flat_map(|f| field.face(0, f).to_vec()).collect();
    let (low, high) = (all.iter().min().unwrap(), all.iter().max().unwrap());
    let range = f64::from(high - low) / 65535.0;
    for (point, value) in points.iter().zip(values) {
        let d = [point[0], point[1], point[2]].map(f64::from);
        let expected = field.field(d, f64::from(point[3]));
        let difference = (f64::from(value) - expected).abs();
        assert!(
            difference <= 1e-3 * range,
            "{point:?}: the shader gives {value}, the CPU gives {expected}"
        );
    }
}
