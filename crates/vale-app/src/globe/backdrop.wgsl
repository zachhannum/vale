// The blurred copy of the canvas that a card of the iPad layout shows. The
// frame drawn so far shrinks into a texture at a reduced size, and two passes
// blur it. A card of glass reads a copy at the full size with a light blur.

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
    // The width and the height of the copy with the light blur in pixels,
    // then its reduction.
    clear: vec4<f32>,
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var bilinear: sampler;
// The copy with the light blur. Only the cards of glass read it.
@group(0) @binding(3) var clear: texture_2d<f32>;

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

// A copy of the frame at the full size.
@fragment
fn fs_copy(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    return textureSampleLevel(source, bilinear, frag.xy / size, 0.0);
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

// The glass follows the shader of the liquidglass library by Yassine
// Bouane (MIT). Its lengths in pixels are for a screen with 3 pixels per
// point, so `unit` scales them. The settings are the settings of the
// library.
const REFRACTION: f32 = 1.42;
const CHROMA: f32 = 0.05;
const EDGE_HIGHLIGHT: f32 = 0.05;
const FRESNEL: f32 = 1.0;
const DISTORTION: f32 = 0.05;
const BRIGHTNESS: f32 = -0.55;
// The radius of the curve of the rim, in points.
const Z_RADIUS: f32 = 40.0;
const INDEX: f32 = 1.5;

// The signed distance of `p` from the edge of a rounded rectangle, with
// `p` relative to the center.
fn rounded(p: vec2<f32>, half: vec2<f32>, radii: vec4<f32>) -> f32 {
    let side = mix(radii.xz, radii.yw, vec2<f32>(step(0.0, p.x)));
    let radius = mix(side.x, side.y, step(0.0, p.y));
    let q = abs(p) - half + radius;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

// The height of the rim at the distance `d` inside the edge: a quarter
// circle of the radius `z`.
fn bevel(d: f32, z: f32) -> f32 {
    if d <= 0.0 {
        return 0.0;
    }
    if d >= z {
        return z;
    }
    return sqrt(d * (2.0 * z - d));
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// The canvas below a card of glass. The card is a slab of glass whose rim
// is curved, and light refracts at its top and bottom surfaces. `p` is the
// place relative to the center, and `pixels` is the pixels per point.
fn glass(pos: vec2<f32>, p: vec2<f32>, half: vec2<f32>, radii: vec4<f32>, pixels: f32) -> vec3<f32> {
    let unit = pixels / 3.0;
    let sdf = rounded(p, half, radii);
    let inside = -sdf;
    let edge = smoothstep(min(half.x, half.y) * 0.35, 0.0, inside);

    // The normal of the top surface, from the slope of the rim.
    let z = Z_RADIUS * pixels;
    let e = 2.0 * unit;
    let dx = vec2<f32>(e, 0.0);
    let dy = vec2<f32>(0.0, e);
    let slope = vec2<f32>(
        bevel(-rounded(p + dx, half, radii), z) - bevel(-rounded(p - dx, half, radii), z),
        bevel(-rounded(p + dy, half, radii), z) - bevel(-rounded(p - dy, half, radii), z),
    ) / (2.0 * e);
    let normal = normalize(vec3<f32>(-slope, 1.0));
    let depth = smoothstep(0.0, z, inside);

    // The ray bends at the top and at the bottom surface, and more where
    // the glass is thick. The whole card also pulls the canvas a little
    // toward its center.
    let power = 1.0 - 1.0 / INDEX;
    let thick = bevel(inside, z) / max(z, 1.0);
    var shift = slope * power * (2.0 + 0.5 * thick) * REFRACTION * 30.0 * unit;
    shift += -p / max(half, vec2<f32>(1.0)) * REFRACTION * 4.0 * unit * depth;
    let noise = p / unit * 0.08;
    shift += (vec2<f32>(hash(noise), hash(noise + vec2<f32>(37.0))) - 0.5) * DISTORTION * 4.0 * unit;

    // Red bends outward and blue inward.
    let fringe = normal.xy * CHROMA * 36.0 * (edge * 0.7 + 0.3) * unit;
    let scale = 1.0 / (g.clear.xy * g.clear.z);
    let at = (pos + shift - g.canvas.xy) * scale;
    var color = vec3<f32>(
        textureSampleLevel(clear, bilinear, at + fringe * scale, 0.0).r,
        textureSampleLevel(clear, bilinear, at, 0.0).g,
        textureSampleLevel(clear, bilinear, at - fringe * scale, 0.0).b,
    );
    color *= (1.0 + BRIGHTNESS) * (1.0 + 0.06 * depth);

    // The light on the glass: a reflection that grows where the surface is
    // steep, a thin line inside the edge that is brighter at the top, and
    // a glow along the rim.
    let fresnel = pow(1.0 - abs(normal.z), 4.0) * FRESNEL;
    let border = 1.5 * unit;
    var line = smoothstep(-border - unit, -border, sdf) * (1.0 - smoothstep(-unit, 0.0, sdf));
    line *= 0.4 + 0.6 * (0.5 - 0.5 * p.y / half.y);
    let rim = edge * EDGE_HIGHLIGHT * 0.22;
    let glow = smoothstep(5.0 * unit, 0.0, inside) * EDGE_HIGHLIGHT * 0.15;
    let reflection = (normal.y * 0.5 + 0.5) * fresnel * 0.08;
    color += vec3<f32>(rim + glow + line * EDGE_HIGHLIGHT * 0.55 + reflection);
    color = mix(color, vec3<f32>(1.0), fresnel * 0.2);
    return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
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
        let color = glass(in.position.xy, p, in.rect.zw, in.radii, in.glass.y);
        return vec4<f32>(color * cover, cover);
    }
    let uv = (in.position.xy - g.canvas.xy) / (g.small.xy * g.blur.y);
    let color = textureSampleLevel(source, bilinear, uv, 0.0).rgb * g.blur.z;
    return vec4<f32>(color * cover, cover);
}
