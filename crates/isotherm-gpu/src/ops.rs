//! The grid operators of `isotherm_sim::ops` as compute shaders, in f32.
//!
//! Each cell has one `Cell` record with its geometry and one `vec4<u32>`
//! of links. A link holds the index of the neighbor in its low 30 bits and
//! the index of the same edge in the list of the neighbor in its top 2 bits.
//! An operator that needs gradients runs a gradient pass first.

use isotherm_sim::Grid;

/// The declarations that each shader shares.
macro_rules! prelude {
    () => {
        r#"
struct Cell {
    center_area: vec4<f32>,
    length: vec4<f32>,
    distance: vec4<f32>,
    normal: array<vec4<f32>, 4>,
    tangent: array<vec4<f32>, 4>,
    offset: array<vec4<f32>, 4>,
    lsq: array<vec4<f32>, 4>,
}

struct Params {
    count: u32,
    limit: u32,
    pad0: u32,
    pad1: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> cells: array<Cell>;
@group(0) @binding(2) var<storage, read> links: array<vec4<u32>>;

fn neighbor(i: u32, e: u32) -> u32 {
    return links[i][e] & 0x3fffffffu;
}

fn back(i: u32, e: u32) -> u32 {
    return links[i][e] >> 30u;
}
"#
    };
}

/// The value of the vector field `u` at the midpoint of edge `e` of cell
/// `i`, from the gradients `gu` of its three components.
macro_rules! edge_vector {
    () => {
        r#"
fn at(c: u32, r: vec3<f32>) -> vec3<f32> {
    let d = vec3(dot(gu[3u * c].xyz, r), dot(gu[3u * c + 1u].xyz, r), dot(gu[3u * c + 2u].xyz, r));
    return u[c].xyz + d;
}

fn edge_vector(i: u32, e: u32) -> vec3<f32> {
    let j = neighbor(i, e);
    let r_j = cells[j].offset[back(i, e)].xyz;
    return 0.5 * (at(i, cells[i].offset[e].xyz) + at(j, r_j));
}
"#
    };
}

/// The least-squares gradient. The entry point `scalar` writes the gradient
/// of `q` to `g`. The entry point `vector` writes the gradients of the three
/// components of `u` to `gu`, three for each cell.
pub const GRADIENT_WGSL: &str = concat!(
    prelude!(),
    r#"
@group(0) @binding(3) var<storage, read> q: array<f32>;
@group(0) @binding(4) var<storage, read> u: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read_write> g: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read_write> gu: array<vec4<f32>>;

@compute @workgroup_size(64)
fn scalar(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.count {
        return;
    }
    var sum = vec3(0.0);
    for (var e = 0u; e < 4u; e++) {
        sum += cells[i].lsq[e].xyz * (q[neighbor(i, e)] - q[i]);
    }
    g[i] = vec4(sum, 0.0);
}

@compute @workgroup_size(64)
fn vector(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.count {
        return;
    }
    for (var k = 0u; k < 3u; k++) {
        var sum = vec3(0.0);
        for (var e = 0u; e < 4u; e++) {
            sum += cells[i].lsq[e].xyz * (u[neighbor(i, e)][k] - u[i][k]);
        }
        gu[3u * i + k] = vec4(sum, 0.0);
    }
}
"#
);

/// The divergence of `u`, after the `vector` pass of [`GRADIENT_WGSL`].
pub const DIVERGENCE_WGSL: &str = concat!(
    prelude!(),
    r#"
@group(0) @binding(4) var<storage, read> u: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read> gu: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read_write> out: array<f32>;
"#,
    edge_vector!(),
    r#"
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.count {
        return;
    }
    var sum = 0.0;
    for (var e = 0u; e < 4u; e++) {
        sum += cells[i].length[e] * dot(edge_vector(i, e), cells[i].normal[e].xyz);
    }
    out[i] = sum / cells[i].center_area.w;
}
"#
);

/// `∇·(k ∇q)`, after the `scalar` pass of [`GRADIENT_WGSL`], with the
/// correction of the gradient along the line between the two centers.
pub const LAPLACIAN_WGSL: &str = concat!(
    prelude!(),
    r#"
@group(0) @binding(3) var<storage, read> q: array<f32>;
@group(0) @binding(5) var<storage, read> k: array<f32>;
@group(0) @binding(6) var<storage, read> g: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.count {
        return;
    }
    var sum = 0.0;
    for (var e = 0u; e < 4u; e++) {
        let j = neighbor(i, e);
        let t = cells[i].tangent[e].xyz;
        let mean = 0.5 * (g[i].xyz + g[j].xyz);
        let along = (q[j] - q[i]) / cells[i].distance[e] - dot(mean, t);
        let g_e = mean + t * along;
        let k_e = 0.5 * (k[i] + k[j]);
        sum += cells[i].length[e] * k_e * dot(g_e, cells[i].normal[e].xyz);
    }
    out[i] = sum / cells[i].center_area.w;
}
"#
);

/// `−∇·(u q)` with second-order upwind values, after both passes of
/// [`GRADIENT_WGSL`]. With `params.limit`, the Barth–Jespersen limiter
/// keeps each edge value within the range of the cell and its neighbors.
pub const ADVECTION_WGSL: &str = concat!(
    prelude!(),
    r#"
@group(0) @binding(3) var<storage, read> q: array<f32>;
@group(0) @binding(4) var<storage, read> u: array<vec4<f32>>;
@group(0) @binding(6) var<storage, read> g: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read> gu: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read_write> out: array<f32>;
"#,
    edge_vector!(),
    r#"
fn limiter(c: u32) -> f32 {
    if params.limit == 0u {
        return 1.0;
    }
    var lo = q[c];
    var hi = q[c];
    for (var e = 0u; e < 4u; e++) {
        lo = min(lo, q[neighbor(c, e)]);
        hi = max(hi, q[neighbor(c, e)]);
    }
    var phi = 1.0;
    for (var e = 0u; e < 4u; e++) {
        let delta = dot(g[c].xyz, cells[c].offset[e].xyz);
        if delta > 1e-30 {
            phi = min(phi, (hi - q[c]) / delta);
        } else if delta < -1e-30 {
            phi = min(phi, (lo - q[c]) / delta);
        }
    }
    return max(phi, 0.0);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.count {
        return;
    }
    var sum = 0.0;
    for (var e = 0u; e < 4u; e++) {
        let speed = dot(edge_vector(i, e), cells[i].normal[e].xyz);
        var value: f32;
        if speed >= 0.0 {
            value = q[i] + limiter(i) * dot(g[i].xyz, cells[i].offset[e].xyz);
        } else {
            let j = neighbor(i, e);
            let r_j = cells[j].offset[back(i, e)].xyz;
            value = q[j] + limiter(j) * dot(g[j].xyz, r_j);
        }
        sum += cells[i].length[e] * speed * value;
    }
    out[i] = -sum / cells[i].center_area.w;
}
"#
);

/// The bytes of the `Cell` record of one cell.
const CELL_BYTES: usize = 3 * 16 + 4 * 4 * 16;

/// The cells of one invocation group.
const GROUP: u32 = 64;

/// The geometry and the links of a grid, in storage buffers.
pub struct GpuGrid {
    count: u32,
    cells: wgpu::Buffer,
    links: wgpu::Buffer,
}

impl GpuGrid {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, grid: &Grid) -> GpuGrid {
        let mut cells = Vec::with_capacity(grid.len() * CELL_BYTES / 4);
        let mut links = Vec::with_capacity(grid.len() * 4);
        let v4 = |v: [f64; 3], w: f64| [v[0] as f32, v[1] as f32, v[2] as f32, w as f32];
        for i in 0..grid.len() {
            let edges = grid.edges(i);
            cells.extend(v4(grid.center(i), grid.area(i)));
            cells.extend(edges.map(|e| e.length as f32));
            cells.extend(edges.map(|e| e.distance as f32));
            for e in edges {
                cells.extend(v4(e.normal, 0.0));
            }
            for e in edges {
                cells.extend(v4(e.tangent, 0.0));
            }
            for e in edges {
                cells.extend(v4(e.mid_offset, 0.0));
            }
            for w in grid.lsq(i) {
                cells.extend(v4(*w, 0.0));
            }
            links.extend(edges.map(|e| e.neighbor | (u32::from(e.back) << 30)));
        }
        GpuGrid {
            count: grid.len() as u32,
            cells: storage(
                device,
                queue,
                "isotherm cells",
                bytemuck::cast_slice(&cells),
            ),
            links: storage(
                device,
                queue,
                "isotherm links",
                bytemuck::cast_slice(&links),
            ),
        }
    }

    /// The bytes of the tables of a grid with `face_size` cells on each
    /// face edge.
    pub fn table_bytes(face_size: usize) -> u64 {
        (6 * face_size * face_size * (CELL_BYTES + 16)) as u64
    }
}

/// The compute pipelines of the operators.
pub struct GpuOps {
    scalar_gradient: wgpu::ComputePipeline,
    vector_gradient: wgpu::ComputePipeline,
    divergence: wgpu::ComputePipeline,
    laplacian: wgpu::ComputePipeline,
    advection: wgpu::ComputePipeline,
}

/// The buffers of one call, by binding.
struct Bindings {
    params: wgpu::Buffer,
    q: wgpu::Buffer,
    u: wgpu::Buffer,
    k: wgpu::Buffer,
    g: wgpu::Buffer,
    gu: wgpu::Buffer,
    out: wgpu::Buffer,
}

impl GpuOps {
    pub fn new(device: &wgpu::Device) -> GpuOps {
        let pipeline = |label: &str, source: &'static str, entry: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        GpuOps {
            scalar_gradient: pipeline("isotherm gradient", GRADIENT_WGSL, "scalar"),
            vector_gradient: pipeline("isotherm vector gradient", GRADIENT_WGSL, "vector"),
            divergence: pipeline("isotherm divergence", DIVERGENCE_WGSL, "main"),
            laplacian: pipeline("isotherm laplacian", LAPLACIAN_WGSL, "main"),
            advection: pipeline("isotherm advection", ADVECTION_WGSL, "main"),
        }
    }

    /// The gradient of `q`, read back from the GPU.
    pub fn gradient(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        grid: &GpuGrid,
        q: &[f32],
    ) -> Vec<[f32; 3]> {
        let b = bindings(device, queue, grid, q, None, None, false);
        self.run(device, queue, grid, &b, &[&self.scalar_gradient]);
        read(device, queue, &b.g, grid.count as usize * 4)
            .chunks(4)
            .map(|v| [v[0], v[1], v[2]])
            .collect()
    }

    /// The divergence of `u`, read back from the GPU.
    pub fn divergence(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        grid: &GpuGrid,
        u: &[[f32; 3]],
    ) -> Vec<f32> {
        let b = bindings(device, queue, grid, &[], Some(u), None, false);
        self.run(
            device,
            queue,
            grid,
            &b,
            &[&self.vector_gradient, &self.divergence],
        );
        read(device, queue, &b.out, grid.count as usize)
    }

    /// `∇·(k ∇q)`, with `k = 1` if it is `None`, read back from the GPU.
    pub fn laplacian(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        grid: &GpuGrid,
        q: &[f32],
        k: Option<&[f32]>,
    ) -> Vec<f32> {
        let b = bindings(device, queue, grid, q, None, k, false);
        self.run(
            device,
            queue,
            grid,
            &b,
            &[&self.scalar_gradient, &self.laplacian],
        );
        read(device, queue, &b.out, grid.count as usize)
    }

    /// `−∇·(u q)`, read back from the GPU.
    pub fn advection(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        grid: &GpuGrid,
        q: &[f32],
        u: &[[f32; 3]],
        limit: bool,
    ) -> Vec<f32> {
        let b = bindings(device, queue, grid, q, Some(u), None, limit);
        self.run(
            device,
            queue,
            grid,
            &b,
            &[
                &self.scalar_gradient,
                &self.vector_gradient,
                &self.advection,
            ],
        );
        read(device, queue, &b.out, grid.count as usize)
    }

    /// Runs the passes in order, in one submit.
    fn run(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        grid: &GpuGrid,
        b: &Bindings,
        passes: &[&wgpu::ComputePipeline],
    ) {
        let mut encoder = device.create_command_encoder(&Default::default());
        for pipeline in passes {
            let layout = pipeline.get_bind_group_layout(0);
            let entries = self.entries(pipeline, grid, b);
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &entries,
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(grid.count.div_ceil(GROUP), 1, 1);
        }
        queue.submit([encoder.finish()]);
    }

    /// The bind group entries of the bindings that a pipeline uses.
    fn entries<'a>(
        &self,
        pipeline: &wgpu::ComputePipeline,
        grid: &'a GpuGrid,
        b: &'a Bindings,
    ) -> Vec<wgpu::BindGroupEntry<'a>> {
        let used: &[u32] = if pipeline == &self.scalar_gradient {
            &[3, 6]
        } else if pipeline == &self.vector_gradient {
            &[4, 7]
        } else if pipeline == &self.divergence {
            &[4, 7, 8]
        } else if pipeline == &self.laplacian {
            &[3, 5, 6, 8]
        } else {
            &[3, 4, 6, 7, 8]
        };
        let buffer = |binding: u32| match binding {
            3 => &b.q,
            4 => &b.u,
            5 => &b.k,
            6 => &b.g,
            7 => &b.gu,
            _ => &b.out,
        };
        let mut entries = vec![
            entry(0, &b.params),
            entry(1, &grid.cells),
            entry(2, &grid.links),
        ];
        entries.extend(used.iter().map(|&i| entry(i, buffer(i))));
        entries
    }
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

/// The buffers of one call. A missing input is a buffer of zeros, and a
/// missing `k` is a buffer of ones.
fn bindings(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    grid: &GpuGrid,
    q: &[f32],
    u: Option<&[[f32; 3]]>,
    k: Option<&[f32]>,
    limit: bool,
) -> Bindings {
    let n = grid.count as usize;
    let scalar = |label, values: Option<&[f32]>, fill: f32| {
        let v = values.map_or_else(|| vec![fill; n], <[f32]>::to_vec);
        storage(device, queue, label, bytemuck::cast_slice(&v))
    };
    let q = scalar("isotherm q", (!q.is_empty()).then_some(q), 0.0);
    let k = scalar("isotherm k", k, 1.0);
    let u4: Vec<f32> = match u {
        Some(u) => u.iter().flat_map(|v| [v[0], v[1], v[2], 0.0]).collect(),
        None => vec![0.0; 4 * n],
    };
    let params = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("isotherm params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(
        &params,
        0,
        bytemuck::cast_slice(&[grid.count, u32::from(limit), 0, 0]),
    );
    Bindings {
        params,
        q,
        u: storage(device, queue, "isotherm u", bytemuck::cast_slice(&u4)),
        k,
        g: storage(device, queue, "isotherm g", &vec![0; 16 * n]),
        gu: storage(device, queue, "isotherm gu", &vec![0; 48 * n]),
        out: storage(device, queue, "isotherm out", &vec![0; 4 * n]),
    }
}

/// A storage buffer with `bytes`.
fn storage(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.len().max(16) as u64,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytes);
    buffer
}

/// The first `count` floats of a buffer. Blocks until the GPU is done.
fn read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    count: usize,
) -> Vec<f32> {
    let size = (count * 4) as u64;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("isotherm readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    staging.map_async(wgpu::MapMode::Read, .., |result| {
        result.expect("the readback buffer maps");
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("the GPU finishes");
    let values = {
        let view = staging.get_mapped_range(..).expect("the buffer is mapped");
        view.as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    };
    staging.unmap();
    values
}
