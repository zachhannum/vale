// Draws the meshes of egui shapes into the target of a group, with the colors
// of egui: multiplied by their alpha and in sRGB gamma. A mesh reads no
// texture, because each shape of a vector layer has the white texel.

struct Locals {
    // The width and the height of the target in points.
    size: vec4<f32>,
}

@group(0) @binding(0) var<uniform> locals: Locals;

struct VectorVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_vector(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: u32,
) -> VectorVertex {
    let rgba = vec4<f32>(
        f32(color & 255u),
        f32((color >> 8u) & 255u),
        f32((color >> 16u) & 255u),
        f32((color >> 24u) & 255u),
    ) / 255.0;
    let ndc = vec2<f32>(2.0 * pos.x / locals.size.x - 1.0, 1.0 - 2.0 * pos.y / locals.size.y);
    return VectorVertex(vec4<f32>(ndc, 0.0, 1.0), rgba);
}

// The color for a target that holds gamma values.
@fragment
fn fs_vector_gamma(in: VectorVertex) -> @location(0) vec4<f32> {
    return in.color;
}

// The color for an sRGB target, which takes linear values.
@fragment
fn fs_vector_linear(in: VectorVertex) -> @location(0) vec4<f32> {
    let c = in.color.rgb;
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(high, low, c < vec3<f32>(0.04045)), in.color.a);
}
