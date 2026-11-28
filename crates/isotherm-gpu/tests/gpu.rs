//! The operators on the GPU against the CPU reference of `isotherm-sim`.

use std::sync::OnceLock;

use isotherm_gpu::{GpuGrid, GpuOps};
use isotherm_sim::grid::cross;
use isotherm_sim::{Grid, Ops, Vector};

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    ops: GpuOps,
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
        let ops = GpuOps::new(&device);
        Gpu { device, queue, ops }
    })
}

/// The grid sizes of the tests. Size 16 has more cells at the face edges
/// and the corners for each cell inside.
const SIZES: [usize; 2] = [16, 32];

fn y32(c: Vector) -> f64 {
    (c[0] * c[0] - c[1] * c[1]) * c[2]
}

/// A solid-body rotation about an axis through two corners of the cube.
fn rotation(c: Vector) -> Vector {
    cross([1.0, 1.0, 1.0], c)
}

fn scalars(grid: &Grid, f: impl Fn(Vector) -> f64) -> Vec<f64> {
    (0..grid.len()).map(|i| f(grid.center(i))).collect()
}

fn vectors(grid: &Grid, f: impl Fn(Vector) -> Vector) -> Vec<Vector> {
    (0..grid.len()).map(|i| f(grid.center(i))).collect()
}

fn f32s(values: &[f64]) -> Vec<f32> {
    values.iter().map(|&v| v as f32).collect()
}

fn vec3s(values: &[Vector]) -> Vec<[f32; 3]> {
    values.iter().map(|v| v.map(|x| x as f32)).collect()
}

/// The largest difference between the GPU and the CPU, as a fraction of
/// the largest CPU value. Panics above 1e-3.
fn compare(name: &str, n: usize, gpu: &[f32], cpu: &[f64]) {
    assert_eq!(gpu.len(), cpu.len());
    let scale = cpu.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    let diff = gpu
        .iter()
        .zip(cpu)
        .fold(0.0_f64, |m, (&g, &c)| m.max((f64::from(g) - c).abs()));
    let relative = diff / scale;
    eprintln!("{name} n={n}: max difference {relative:.2e} of {scale:.3}");
    assert!(relative < 1e-3, "{name} n={n}: {relative:.2e}");
}

#[test]
#[ignore = "gpu"]
fn gradient_matches_the_cpu() {
    let gpu = gpu();
    for n in SIZES {
        let grid = Grid::new(n);
        let q = scalars(&grid, y32);
        let mut cpu = vec![[0.0; 3]; grid.len()];
        Ops::new(&grid).gradient(&q, &mut cpu);
        let gg = GpuGrid::new(&gpu.device, &gpu.queue, &grid);
        let got = gpu.ops.gradient(&gpu.device, &gpu.queue, &gg, &f32s(&q));
        for c in 0..3 {
            let g: Vec<f32> = got.iter().map(|v| v[c]).collect();
            let cpu: Vec<f64> = cpu.iter().map(|v| v[c]).collect();
            compare(&format!("gradient {c}"), n, &g, &cpu);
        }
    }
}

#[test]
#[ignore = "gpu"]
fn divergence_matches_the_cpu() {
    let gpu = gpu();
    for n in SIZES {
        let grid = Grid::new(n);
        // The gradient of z has a divergence of −2z, which is not zero.
        let u = vectors(&grid, |c| {
            let z = c[2];
            [-z * c[0], -z * c[1], 1.0 - z * z]
        });
        let mut cpu = vec![0.0; grid.len()];
        Ops::new(&grid).divergence(&u, &mut cpu);
        let gg = GpuGrid::new(&gpu.device, &gpu.queue, &grid);
        let got = gpu.ops.divergence(&gpu.device, &gpu.queue, &gg, &vec3s(&u));
        compare("divergence", n, &got, &cpu);
    }
}

#[test]
#[ignore = "gpu"]
fn laplacian_matches_the_cpu() {
    let gpu = gpu();
    for n in SIZES {
        let grid = Grid::new(n);
        let q = scalars(&grid, y32);
        let k = scalars(&grid, |c| 1.0 + 0.5 * c[0]);
        let gg = GpuGrid::new(&gpu.device, &gpu.queue, &grid);
        let mut ops = Ops::new(&grid);
        let mut cpu = vec![0.0; grid.len()];
        ops.laplacian(&q, None, &mut cpu);
        let got = gpu
            .ops
            .laplacian(&gpu.device, &gpu.queue, &gg, &f32s(&q), None);
        compare("laplacian", n, &got, &cpu);
        ops.laplacian(&q, Some(&k), &mut cpu);
        let got = gpu
            .ops
            .laplacian(&gpu.device, &gpu.queue, &gg, &f32s(&q), Some(&f32s(&k)));
        compare("laplacian with k", n, &got, &cpu);
    }
}

#[test]
#[ignore = "gpu"]
fn advection_matches_the_cpu() {
    let gpu = gpu();
    for n in SIZES {
        let grid = Grid::new(n);
        let q = scalars(&grid, |c| c[2]);
        let u = vectors(&grid, rotation);
        let gg = GpuGrid::new(&gpu.device, &gpu.queue, &grid);
        let mut ops = Ops::new(&grid);
        let mut cpu = vec![0.0; grid.len()];
        for limit in [false, true] {
            ops.advection(&q, &u, limit, &mut cpu);
            let got = gpu
                .ops
                .advection(&gpu.device, &gpu.queue, &gg, &f32s(&q), &vec3s(&u), limit);
            compare(&format!("advection limit={limit}"), n, &got, &cpu);
        }
        // The limiter acts at the edge of a step.
        let step = scalars(&grid, |c| if c[0] > 0.3 { 1.0 } else { 0.0 });
        ops.advection(&step, &u, true, &mut cpu);
        let got = gpu
            .ops
            .advection(&gpu.device, &gpu.queue, &gg, &f32s(&step), &vec3s(&u), true);
        compare("advection of a step", n, &got, &cpu);
    }
}
