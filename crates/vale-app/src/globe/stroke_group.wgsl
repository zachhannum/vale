// Draws the opaque shapes of a translucent stroke into a target of their
// own, then puts the target on the screen with the alpha of the stroke. The
// overlaps of the shapes get the alpha only once.

struct Params {
    // The left, the top, the right, and the bottom of the target on the
    // screen, in pixels.
    rect: vec4<f32>,
    // The width and the height of the screen in pixels, the pixels per
    // point, and the alpha of the group.
    screen: vec4<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var layer: texture_2d<f32>;

struct MeshOut {
    @builtin(position) position: vec4<f32>,
    // Gamma-space color with premultiplied alpha, as egui gives it.
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_mesh(
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> MeshOut {
    let p = pos * params.screen.z - params.rect.xy;
    let size = params.rect.zw - params.rect.xy;
    var out: MeshOut;
    out.position = vec4<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0, 0.0, 1.0);
    out.color = color;
    return out;
}

fn linear_from_gamma_rgb(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let lower = srgb / vec3<f32>(12.92);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

// For a target that holds gamma-space colors.
@fragment
fn fs_mesh_gamma(in: MeshOut) -> @location(0) vec4<f32> {
    return in.color;
}

// For an sRGB target, which converts linear colors to gamma space.
@fragment
fn fs_mesh_linear(in: MeshOut) -> @location(0) vec4<f32> {
    return vec4<f32>(linear_from_gamma_rgb(in.color.rgb), in.color.a);
}

@vertex
fn vs_composite(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    let p = mix(params.rect.xy, params.rect.zw, corner);
    return vec4<f32>(p.x / params.screen.x * 2.0 - 1.0, 1.0 - p.y / params.screen.y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_composite(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(frag.xy - params.rect.xy));
    return textureLoad(layer, p, 0) * params.screen.w;
}
