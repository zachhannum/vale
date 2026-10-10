// A group of brush stamps on one face of the cube map. The result is the same
// as `Heightmap::stamp` for each stamp in order. `gpu.rs` fills the uniforms
// and sets the order of the passes.

struct Stamp {
    // The brush center on this face in texels: the whole part and the rest.
    center: vec2<i32>,
    center_rest: vec2<f32>,
    // The flat face coordinates of the brush center, and the cosine of the
    // angle of each one.
    flat: vec2<f32>,
    cos_center: vec2<f32>,
    // The texels that the stamp can change: x0, y0, x1, y1.
    rect: vec4<i32>,
    radius: f32,
    hardness: f32,
    flow: f32,
    strength: f32,
    level: f32,
}

// The same number as `GROUP_STAMPS` in `gpu.rs`.
const GROUP_STAMPS = 32;

struct Group {
    face: u32,
    mode: u32,
    reach: i32,
    size: i32,
    count: u32,
    // The level of the sea.
    sea: u32,
    // The number of texels along one side of a face of the channel map.
    channel_size: u32,
    // 1 if the carve mode has a window, and 0 if not.
    window_on: u32,
    // The first texel, the face, and the cell size of the window.
    window_origin: vec2<i32>,
    window_face: u32,
    window_cell: u32,
    stamps: array<Stamp, GROUP_STAMPS>,
}

const PI_4 = 0.7853981633974483;

const RAISE = 0u;
const LOWER = 1u;
const SMOOTH = 2u;
const CARVE = 4u;

@group(0) @binding(0) var<uniform> group: Group;
// A copy of this face from before the pass.
@group(0) @binding(1) var before: texture_2d<u32>;
// The faces past the +u, -u, +v, and -v edges of this face.
@group(0) @binding(2) var past_pu: texture_2d<u32>;
@group(0) @binding(3) var past_nu: texture_2d<u32>;
@group(0) @binding(4) var past_pv: texture_2d<u32>;
@group(0) @binding(5) var past_nv: texture_2d<u32>;
// The channel map of all faces. The red byte of a texel is the distance to
// the nearest river in channel texels, times 32. The green byte is the size
// of that river, or 0 if no river is near.
@group(0) @binding(6) var channels: texture_2d_array<u32>;
// The channel texels of the window, with the same two bytes.
@group(0) @binding(7) var window_channels: texture_2d<u32>;

// The same number as `WINDOW_MARGIN` in `flow.rs`.
const WINDOW_MARGIN = 8.0;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32(i & 1u), f32(i >> 1u)) * 4.0 - 1.0;
    return vec4<f32>(p, 0.0, 1.0);
}

// The flat coordinate of a texel index that can be past the face edge.
fn flat_of(i: i32) -> f32 {
    let s = clamp((f32(i) + 0.5) / f32(group.size) * 2.0 - 1.0, -1.99, 1.99);
    if abs(s) <= 1.0 {
        return tan_series(s * PI_4);
    }
    // Past the edge, the tangent is one over the tangent of the angle from
    // the far end of the range.
    return sign(s) / tan_series((2.0 - abs(s)) * PI_4);
}

// The level of a texel past the face edge. The texel grid continues past the
// edge, and the read gets the nearest texel of the face that is there.
fn level_past_edge(p: vec2<i32>) -> u32 {
    let axis = group.face / 2u;
    var d: vec3<f32>;
    d[axis] = 1.0 - 2.0 * f32(group.face % 2u);
    d[(axis + 1u) % 3u] = flat_of(p.x);
    d[(axis + 2u) % 3u] = flat_of(p.y);
    let size = abs(d);
    var to = 2u;
    if size.x >= size.y && size.x >= size.z {
        to = 0u;
    } else if size.y >= size.z {
        to = 1u;
    }
    let flat = vec2<f32>(d[(to + 1u) % 3u], d[(to + 2u) % 3u]) / size[to];
    let warp = vec2<f32>(atan_unit(flat.x), atan_unit(flat.y)) / PI_4;
    let texel = floor((warp + 1.0) * 0.5 * f32(group.size));
    let at = clamp(vec2<i32>(texel), vec2<i32>(0), vec2<i32>(group.size - 1));
    if to == (axis + 1u) % 3u {
        if d[to] < 0.0 {
            return textureLoad(past_nu, at, 0).r;
        }
        return textureLoad(past_pu, at, 0).r;
    }
    if d[to] < 0.0 {
        return textureLoad(past_nv, at, 0).r;
    }
    return textureLoad(past_pv, at, 0).r;
}

fn level_before(p: vec2<i32>) -> f32 {
    if all(p >= vec2<i32>(0)) && all(p < vec2<i32>(group.size)) {
        return f32(textureLoad(before, p, 0).r);
    }
    return f32(level_past_edge(p));
}

// The builtin functions of a driver can be wrong in the fifth digit. The
// weight of the brush needs the seventh digit, so these series replace them.

// The sine of an angle of less than 1 radian.
fn sin_series(x: vec2<f32>) -> vec2<f32> {
    let q = x * x;
    return x * (1.0 + q * (-1.0 / 6.0 + q * (1.0 / 120.0 + q * (-1.0 / 5040.0
        + q * (1.0 / 362880.0 + q * (-1.0 / 39916800.0))))));
}

// The cosine of an angle of less than 1 radian.
fn cos_series(x: vec2<f32>) -> vec2<f32> {
    let q = x * x;
    return 1.0 + q * (-1.0 / 2.0 + q * (1.0 / 24.0 + q * (-1.0 / 720.0
        + q * (1.0 / 40320.0 + q * (-1.0 / 3628800.0 + q * (1.0 / 479001600.0))))));
}

// The largest tangent that `atan_series` takes. Its angle is more than the
// largest brush radius.
const MAX_TANGENT = 0.35;

fn atan_series(t: f32) -> f32 {
    let q = t * t;
    return t * (1.0 + q * (-1.0 / 3.0 + q * (1.0 / 5.0 + q * (-1.0 / 7.0
        + q * (1.0 / 9.0 + q * (-1.0 / 11.0 + q * (1.0 / 13.0)))))));
}

// The tangent of an angle from -PI / 4 to PI / 4.
fn tan_series(x: f32) -> f32 {
    let pair = vec2<f32>(x);
    return sin_series(pair).x / cos_series(pair).x;
}

// The arc tangent of a number from -1 to 1. Each step halves the angle.
fn atan_unit(t: f32) -> f32 {
    let half = t / (1.0 + sqrt(1.0 + t * t));
    let quarter = half / (1.0 + sqrt(1.0 + half * half));
    return 4.0 * atan_series(quarter);
}

// The distance from a texel to the nearest river in channel texels, and the
// size of that river. The steps are those of `ChannelMap::sample`.
fn river_at(at: vec2<i32>) -> vec2<f32> {
    let size = f32(group.channel_size);
    let c = (vec2<f32>(at) + 0.5) * size / f32(group.size) - 0.5;
    let whole = floor(c);
    let t = c - whole;
    let last = vec2<i32>(i32(group.channel_size) - 1);
    let p0 = clamp(vec2<i32>(whole), vec2<i32>(0), last);
    let p1 = clamp(vec2<i32>(whole) + 1, vec2<i32>(0), last);
    let face = i32(group.face);
    let a = vec2<f32>(textureLoad(channels, p0, face, 0).rg);
    let b = vec2<f32>(textureLoad(channels, vec2<i32>(p1.x, p0.y), face, 0).rg);
    let c0 = vec2<f32>(textureLoad(channels, vec2<i32>(p0.x, p1.y), face, 0).rg);
    let d = vec2<f32>(textureLoad(channels, p1, face, 0).rg);
    // The products are exact when both sizes are powers of two. The builtin
    // `mix` can use other steps.
    let top = a.x * (1.0 - t.x) + b.x * t.x;
    let bottom = c0.x * (1.0 - t.x) + d.x * t.x;
    let distance = (top * (1.0 - t.y) + bottom * t.y) / 32.0;
    return vec2<f32>(distance, max(max(a.y, b.y), max(c0.y, d.y)));
}

// The arc tangent of each number.
fn atan_any(t: f32) -> f32 {
    if abs(t) <= 1.0 {
        return atan_unit(t);
    }
    return sign(t) * (2.0 * PI_4 - atan_unit(1.0 / abs(t)));
}

// The distance from a texel to the nearest river of the window in channel
// texels of the window, and the size of that river from 0 to 255. The third
// number is 1 if the window covers the texel, and 0 if not. The steps are
// those of `Window::channel_of_texel`, `Window::channel_of_dir`, and
// `ChannelWindow::lookup`.
fn window_river_at(at: vec2<i32>) -> vec3<f32> {
    if group.window_on == 0u {
        return vec3<f32>(0.0);
    }
    let cell = f32(group.window_cell);
    var c: vec2<f32>;
    if group.face == group.window_face {
        // The difference of the whole numbers is exact.
        c = (vec2<f32>(at - group.window_origin) + 0.5) * 2.0 / cell - 0.5;
    } else {
        // The direction of the texel, then its place on the face of the
        // window. The texel grid of that face continues past its edges.
        let axis = group.face / 2u;
        var d: vec3<f32>;
        d[axis] = 1.0 - 2.0 * f32(group.face % 2u);
        d[(axis + 1u) % 3u] = flat_of(at.x);
        d[(axis + 2u) % 3u] = flat_of(at.y);
        let to = group.window_face / 2u;
        let depth = d[to] * (1.0 - 2.0 * f32(group.window_face % 2u));
        if depth <= 0.2 * length(d) {
            return vec3<f32>(0.0);
        }
        let flat = vec2<f32>(d[(to + 1u) % 3u], d[(to + 2u) % 3u]) / depth;
        let warp = vec2<f32>(atan_any(flat.x), atan_any(flat.y)) / PI_4;
        let texel = (warp + 1.0) * 0.5 * f32(group.size);
        c = (texel - vec2<f32>(group.window_origin)) * 2.0 / cell - 0.5;
    }
    let last = f32(textureDimensions(window_channels).x) - 1.0 - WINDOW_MARGIN;
    if any(c < vec2<f32>(WINDOW_MARGIN)) || any(c > vec2<f32>(last)) {
        return vec3<f32>(0.0);
    }
    let whole = floor(c);
    let t = c - whole;
    let p0 = vec2<i32>(whole);
    let p1 = p0 + 1;
    let a = vec2<f32>(textureLoad(window_channels, p0, 0).rg);
    let b = vec2<f32>(textureLoad(window_channels, vec2<i32>(p1.x, p0.y), 0).rg);
    let c0 = vec2<f32>(textureLoad(window_channels, vec2<i32>(p0.x, p1.y), 0).rg);
    let d = vec2<f32>(textureLoad(window_channels, p1, 0).rg);
    let top = a * (1.0 - t.x) + b * t.x;
    let bottom = c0 * (1.0 - t.x) + d * t.x;
    let both = top * (1.0 - t.y) + bottom * t.y;
    return vec3<f32>(both.x / 32.0, both.y, 1.0);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let at = vec2<i32>(floor(position.xy));
    let n = f32(group.size);
    let theta = ((vec2<f32>(at) + 0.5) / n * 2.0 - 1.0) * PI_4;
    let cos_theta = cos_series(theta);
    // The level after each stamp is a whole number, as it is in the texture
    // between two passes.
    var value = f32(textureLoad(before, at, 0).r);
    var changed = false;
    // The distance to the river and its size. The window comes first. With
    // no river near, the size is 0.
    var river = vec2<f32>(0.0);
    if group.mode == CARVE {
        let in_window = window_river_at(at);
        if in_window.z > 0.0 {
            river = in_window.xy;
        } else {
            river = river_at(at);
        }
    }
    for (var i = 0u; i < group.count; i++) {
        let stamp = group.stamps[i];
        // `Heightmap::stamp` visits the texels of this rectangle only.
        if any(at < stamp.rect.xy) || any(at >= stamp.rect.zw) {
            continue;
        }
        // The angle from the brush center to the texel along each face axis.
        // The whole part of the center keeps the difference exact.
        let step = (vec2<f32>(at - stamp.center) + 0.5 - stamp.center_rest) * (2.0 * PI_4 / n);
        // The flat coordinates of the texel minus those of the center.
        let d = sin_series(step) / (cos_theta * stamp.cos_center);
        let flat = stamp.flat + d;
        // The tangent of the angle between (1, center) and (1, texel), from
        // the cross product and the dot product.
        let twist = d.x * stamp.flat.y - d.y * stamp.flat.x;
        let across = sqrt(twist * twist + dot(d, d));
        let along = 1.0 + dot(flat, stamp.flat);
        if across >= MAX_TANGENT * along {
            continue;
        }
        let angle = atan_series(across / along);
        if angle >= stamp.radius {
            continue;
        }
        let k = angle / stamp.radius;
        var weight = 1.0;
        if k > stamp.hardness {
            let t = (k - stamp.hardness) / (1.0 - stamp.hardness);
            weight = 1.0 - t * t * (3.0 - 2.0 * t);
        }
        let amount = weight * stamp.flow;
        let old = value;
        // The change of the level. A sum of the old level and a small change
        // has too few digits after the point for the rounding step.
        var change = 0.0;
        switch group.mode {
            case RAISE {
                change = amount * stamp.strength;
            }
            case LOWER {
                change = -amount * stamp.strength;
            }
            case SMOOTH {
                let r = group.reach;
                let sum = level_before(at + vec2<i32>(-r, -r))
                    + level_before(at + vec2<i32>(0, -r))
                    + level_before(at + vec2<i32>(r, -r))
                    + level_before(at + vec2<i32>(-r, 0))
                    + level_before(at + vec2<i32>(r, 0))
                    + level_before(at + vec2<i32>(-r, r))
                    + level_before(at + vec2<i32>(0, r))
                    + level_before(at + vec2<i32>(r, r))
                    + old;
                change = (sum - 9.0 * old) / 9.0 * min(amount, 1.0);
            }
            case CARVE {
                // A large river has a wide and deep valley. With no river
                // near, the size is 0 and the level stays.
                if river.y > 0.0 {
                    let size = river.y / 255.0;
                    let k = clamp(river.x / (1.5 + 4.5 * size), 0.0, 1.0);
                    let profile = 1.0 - k * k * (3.0 - 2.0 * k);
                    change = -amount * stamp.strength * profile * (0.25 + 0.75 * size);
                }
            }
            default {
                change = (stamp.level - old) * min(amount, 1.0);
            }
        }
        // Round half up, as Rust `round` does for a positive number. The old
        // level is a whole number, so the rounding of the change is enough.
        value = clamp(old + floor(change + 0.5), 0.0, 65535.0);
        if group.mode == CARVE {
            // Land does not go below sea level.
            value = max(value, min(old, f32(group.sea)));
        }
        changed = true;
    }
    if !changed {
        discard;
    }
    return vec4<u32>(u32(value), 0u, 0u, 0u);
}
