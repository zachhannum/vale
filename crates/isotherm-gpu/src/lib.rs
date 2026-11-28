//! The wgpu compute shaders of the steps of `isotherm-sim`.

mod ops;

pub use ops::{ADVECTION_WGSL, DIVERGENCE_WGSL, GRADIENT_WGSL, GpuGrid, GpuOps, LAPLACIAN_WGSL};

/// The daily mean insolation of each cell, as in
/// `isotherm_sim::daily_mean_insolation`. Each invocation reads the latitude
/// of one cell, in radians, and writes its flux, in W/m².
pub const INSOLATION_WGSL: &str = r#"
struct Params {
    s0: f32,
    declination: f32,
    count: u32,
    pad: u32,
}

const PI: f32 = 3.14159265358979;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> latitude: array<f32>;
@group(0) @binding(2) var<storage, read_write> flux: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= params.count {
        return;
    }
    let lat = latitude[i];
    let d = params.declination;
    let h0 = acos(clamp(-tan(lat) * tan(d), -1.0, 1.0));
    flux[i] = params.s0 / PI * (h0 * sin(lat) * sin(d) + cos(lat) * cos(d) * sin(h0));
}
"#;

/// The shader module descriptor of `INSOLATION_WGSL`.
pub fn insolation_module() -> wgpu::ShaderModuleDescriptor<'static> {
    wgpu::ShaderModuleDescriptor {
        label: Some("isotherm insolation"),
        source: wgpu::ShaderSource::Wgsl(INSOLATION_WGSL.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(source: &str) {
        let module = naga::front::wgsl::parse_str(source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn insolation_shader_is_valid() {
        validate(INSOLATION_WGSL);
        assert_eq!(insolation_module().label, Some("isotherm insolation"));
    }

    #[test]
    fn gradient_shader_is_valid() {
        validate(GRADIENT_WGSL);
    }

    #[test]
    fn divergence_shader_is_valid() {
        validate(DIVERGENCE_WGSL);
    }

    #[test]
    fn laplacian_shader_is_valid() {
        validate(LAPLACIAN_WGSL);
    }

    #[test]
    fn advection_shader_is_valid() {
        validate(ADVECTION_WGSL);
    }

    #[test]
    fn the_tables_take_320_bytes_a_cell() {
        assert_eq!(GpuGrid::table_bytes(64), 6 * 64 * 64 * 320);
    }
}
