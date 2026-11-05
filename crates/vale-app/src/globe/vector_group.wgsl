// Puts the opaque shapes of a translucent stroke on the target of a group,
// with the alpha of the stroke. The shapes are in a target of their own with
// the size of the target of the group.

struct Params {
    // The alpha of the stroke.
    alpha: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var shapes: texture_2d<f32>;

@vertex
fn vs_group(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_group(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(shapes, vec2<i32>(at.xy), 0) * params.alpha.x;
}
