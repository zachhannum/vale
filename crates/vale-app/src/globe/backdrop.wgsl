// The blurred copy of the canvas that a card of the iPad layout shows. The
// canvas is drawn into a texture. A second texture holds the canvas at a
// reduced size, and two passes blur it.

struct Globals {
    // The left and the top of the canvas on the screen, then its width and
    // its height, in pixels.
    canvas: vec4<f32>,
    // The width and the height of the reduced copy in pixels, then the
    // direction of the blur pass.
    small: vec4<f32>,
    // The standard deviation of the blur in pixels of the reduced copy, the
    // reduction, and the gain of the backdrop.
    blur: vec4<f32>,
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var bilinear: sampler;

// The largest distance of a blur tap from the center, in pixels.
const MAX_REACH: i32 = 48;

// One triangle that covers the viewport.
fn corner(i: u32) -> vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return corner(i);
}

// Copies the canvas texture to the screen.
@fragment
fn fs_blit(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let last = vec2<i32>(g.canvas.zw) - 1;
    let at = clamp(vec2<i32>(frag.xy - g.canvas.xy), vec2<i32>(0), last);
    return textureLoad(source, at, 0);
}

// Reduces the canvas texture. The four taps give the mean of the pixels that
// one pixel of the copy covers, at a reduction of 4.
@fragment
fn fs_down(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let center = frag.xy * g.blur.y;
    let tap = g.blur.y * 0.25;
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < 4; i++) {
        let side = vec2<f32>(f32(i & 1), f32(i >> 1)) * 2.0 - 1.0;
        sum += textureSampleLevel(source, bilinear, (center + side * tap) / g.canvas.zw, 0.0);
    }
    return sum * 0.25;
}

fn gauss(x: f32, sigma: f32) -> f32 {
    return exp(-0.5 * x * x / (sigma * sigma));
}

// One direction of the Gaussian blur. One tap between two pixels reads both
// of them.
@fragment
fn fs_blur(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let sigma = max(g.blur.x, 0.01);
    let reach = min(i32(ceil(sigma * 2.5)), MAX_REACH);
    let uv = frag.xy / g.small.xy;
    let tap = g.small.zw / g.small.xy;
    var sum = textureSampleLevel(source, bilinear, uv, 0.0);
    var total = 1.0;
    for (var i = 1; i <= reach; i += 2) {
        let near = gauss(f32(i), sigma);
        let far = gauss(f32(i + 1), sigma);
        let weight = near + far;
        let offset = (f32(i) * near + f32(i + 1) * far) / weight * tap;
        sum += textureSampleLevel(source, bilinear, uv + offset, 0.0) * weight;
        sum += textureSampleLevel(source, bilinear, uv - offset, 0.0) * weight;
        total += 2.0 * weight;
    }
    return sum / total;
}

struct CardVertex {
    @builtin(position) position: vec4<f32>,
    // The center of the card, then half of its width and height, in pixels.
    @location(0) @interpolate(flat) rect: vec4<f32>,
    // The corner radii in pixels: top left, top right, bottom left, bottom right.
    @location(1) @interpolate(flat) radii: vec4<f32>,
}

// egui sets the viewport to the card, so the triangle covers the card.
@vertex
fn vs_card(
    @builtin(vertex_index) i: u32,
    @location(0) rect: vec4<f32>,
    @location(1) radii: vec4<f32>,
) -> CardVertex {
    return CardVertex(corner(i), rect, radii);
}

// The blurred canvas inside the rounded rectangle of a card.
@fragment
fn fs_card(in: CardVertex) -> @location(0) vec4<f32> {
    let p = in.position.xy - in.rect.xy;
    let side = mix(in.radii.xz, in.radii.yw, vec2<f32>(step(0.0, p.x)));
    let radius = mix(side.x, side.y, step(0.0, p.y));
    let q = abs(p) - in.rect.zw + radius;
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let cover = clamp(0.5 - distance, 0.0, 1.0);
    let uv = (in.position.xy - g.canvas.xy) / (g.small.xy * g.blur.y);
    let color = textureSampleLevel(source, bilinear, uv, 0.0).rgb * g.blur.z;
    return vec4<f32>(color * cover, cover);
}
