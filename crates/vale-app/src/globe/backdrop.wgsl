// The blurred copy of the canvas that a card of the iPad layout shows. The
// frame drawn so far shrinks into a texture at a reduced size, and two passes
// blur it. A card of glass also reads the reduced copy before the blur.

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
// The reduced copy before the blur. Only the cards read it.
@group(0) @binding(3) var sharp: texture_2d<f32>;

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

// One pixel of the reduced copy: the mean of the 4 by 4 pixels of the frame
// that it covers. Each bilinear tap reads 2 by 2 pixels.
@fragment
fn fs_down(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    let center = frag.xy * g.blur.y;
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < 4; i += 1) {
        let offset = vec2<f32>(f32(i & 1) * 2.0 - 1.0, f32(i >> 1u) * 2.0 - 1.0);
        sum += textureSampleLevel(source, bilinear, (center + offset) / size, 0.0);
    }
    return sum * 0.25;
}

struct CardVertex {
    @builtin(position) position: vec4<f32>,
    // The center of the card, then half of its width and height, in pixels.
    @location(0) @interpolate(flat) rect: vec4<f32>,
    // The corner radii in pixels: top left, top right, bottom left, bottom right.
    @location(1) @interpolate(flat) radii: vec4<f32>,
    // 1 for a card of glass, then the pixels per point.
    @location(2) @interpolate(flat) glass: vec4<f32>,
}

// egui sets the viewport to the card, so the triangle covers the card.
@vertex
fn vs_card(
    @builtin(vertex_index) i: u32,
    @location(0) rect: vec4<f32>,
    @location(1) radii: vec4<f32>,
    @location(2) glass: vec4<f32>,
) -> CardVertex {
    return CardVertex(corner(i), rect, radii, glass);
}

// The width of the curved rim of a card of glass, in points.
const RIM_POINTS: f32 = 18.0;
// The ratio of the refractive indices of air and glass.
const ETA: f32 = 0.667;
// The part of the dark tint of the card that the glass adds.
const GLASS_GAIN: f32 = 0.62;

// The canvas below a card of glass. The card is a slab whose rim is a
// quarter circle in section. A ray from the eye refracts at the surface of
// the slab and meets the canvas below it, so the canvas below the rim moves
// toward the edge. The middle of the card is flat and shows the blur. `p`
// is the place relative to the center, `q` and `distance` are the terms of
// the rounded rectangle, and `pixels` is the pixels per point.
fn glass(pos: vec2<f32>, p: vec2<f32>, q: vec2<f32>, distance: f32, half: vec2<f32>, pixels: f32) -> vec3<f32> {
    let rim = min(RIM_POINTS * pixels, min(half.x, half.y));
    // 0 in the flat middle, 1 at the edge.
    let u = clamp(1.0 + distance / rim, 0.0, 0.98);
    let side = select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
    var out = vec2<f32>(0.0, side.y);
    if q.x > 0.0 && q.y > 0.0 {
        out = normalize(q) * side;
    } else if q.x > q.y {
        out = vec2<f32>(side.x, 0.0);
    }
    let height = sqrt(1.0 - u * u);
    let slope = u / height;
    let normal = normalize(vec3<f32>(out * slope, 1.0));
    let ray = refract(vec3<f32>(0.0, 0.0, -1.0), normal, ETA);
    let shift = ray.xy / -ray.z * height * rim;
    let scale = 1.0 / (g.small.xy * g.blur.y);
    let at = (pos - g.canvas.xy) * scale;
    // Blue bends more than red.
    let sharp_color = vec3<f32>(
        textureSampleLevel(sharp, bilinear, at + shift * 0.94 * scale, 0.0).r,
        textureSampleLevel(sharp, bilinear, at + shift * scale, 0.0).g,
        textureSampleLevel(sharp, bilinear, at + shift * 1.08 * scale, 0.0).b,
    );
    let blurred = textureSampleLevel(source, bilinear, at + shift * scale, 0.0).rgb;
    var color = mix(blurred, sharp_color, smoothstep(0.4, 0.9, u)) * GLASS_GAIN;
    // The light comes from the top left. The far side of the rim shows a
    // weaker reflection of it.
    let light = normalize(vec2<f32>(-1.0, -1.2));
    let facing = max(dot(out, light), 0.0);
    let back = max(dot(out, -light), 0.0);
    let line = exp(distance / (1.2 * pixels));
    let glow = pow(u, 4.0);
    color += vec3<f32>(line * (0.9 * facing * facing + 0.35 * back * back + 0.12));
    color += vec3<f32>(glow * (0.10 + 0.12 * facing));
    return color;
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
    if in.glass.x > 0.0 {
        let color = glass(in.position.xy, p, q, distance, in.rect.zw, in.glass.y);
        return vec4<f32>(color * cover, cover);
    }
    let uv = (in.position.xy - g.canvas.xy) / (g.small.xy * g.blur.y);
    let color = textureSampleLevel(source, bilinear, uv, 0.0).rgb * g.blur.z;
    return vec4<f32>(color * cover, cover);
}
