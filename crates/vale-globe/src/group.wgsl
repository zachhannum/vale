// Puts the pixels of an offscreen target on the screen, in the rectangle
// that `rect` gives. The target holds colors multiplied by their alpha. A
// mask of the same size cuts the pixels to a shape.

struct Place {
    // The left, the top, the right, and the bottom of the target on the
    // screen, in pixels.
    rect: vec4<f32>,
    // The width and the height of the screen in pixels, then 1 if the mask
    // is on.
    screen: vec4<f32>,
    // The left and the top of the squares of the pixels of a page, and their
    // side, in pixels of the screen. A side of 0 turns the squares off.
    grid: vec4<f32>,
}

@group(0) @binding(0) var<uniform> place: Place;
@group(0) @binding(1) var layers: texture_2d<f32>;
@group(0) @binding(2) var mask: texture_2d<f32>;

@vertex
fn vs_group(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    let p = mix(place.rect.xy, place.rect.zw, corner);
    return vec4<f32>(p.x / place.screen.x * 2.0 - 1.0, 1.0 - p.y / place.screen.y * 2.0, 0.0, 1.0);
}

@fragment
fn fs_group(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    var p = vec2<i32>(floor(frag.xy - place.rect.xy));
    if place.grid.z > 0.0 {
        // Each square shows the target at its middle.
        let side = place.grid.z;
        let middle = place.grid.xy + (floor((frag.xy - place.grid.xy) / side) + 0.5) * side;
        let last = vec2<i32>(textureDimensions(layers, 0)) - vec2<i32>(1, 1);
        p = clamp(vec2<i32>(floor(middle - place.rect.xy)), vec2<i32>(0, 0), last);
    }
    var color = textureLoad(layers, p, 0);
    if place.screen.z > 0.5 {
        color = color * textureLoad(mask, p, 0).r;
    }
    return color;
}
