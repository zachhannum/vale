//! The brush on the GPU against the brush on the CPU.

use std::sync::{OnceLock, mpsc};

use vale_terrain::math::{V3, lonlat_to_dir};
use vale_terrain::{FACES, GpuHeightmap, Heightmap, Mode, STAMP_SLOTS, Stamp, TexelRect};

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
/// stamps touch one texel, and the largest difference is 2.
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

    /// A GPU heightmap with the texels of `cpu`.
    fn copy_of(&self, cpu: &Heightmap) -> GpuHeightmap {
        let map = GpuHeightmap::new(&self.device, cpu.face_size() as u32);
        let mut enc = self.encoder();
        map.clear(&mut enc, cpu.base());
        // An upload runs before the commands of the submit that follows it.
        self.submit(enc);
        for (face, rect) in cpu.allocated_rects() {
            let mut data = Vec::new();
            cpu.read_rect(face, rect, &mut data);
            map.upload(&self.queue, face as u32, rect, &data);
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
    let mut enc = gpu.encoder();
    map.clear(&mut enc, 1234);
    gpu.submit(enc);
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
