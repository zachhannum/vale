// One brush stamp on one face of the cube map. The result is the same as
// `Heightmap::stamp`. `gpu.rs` fills the uniforms and sets the order of the
// passes.

struct Stamp {
    // The brush center on this face in texels: the whole part and the rest.
    center: vec2<i32>,
    center_rest: vec2<f32>,
    // The flat face coordinates of the brush center, and the cosine of the
    // angle of each one.
    flat: vec2<f32>,
    cos_center: vec2<f32>,
    radius: f32,
    hardness: f32,
    flow: f32,
    strength: f32,
    level: f32,
    face: u32,
    mode: u32,
    reach: i32,
    size: i32,
}

const PI_4 = 0.7853981633974483;

const RAISE = 0u;
const LOWER = 1u;
const SMOOTH = 2u;

@group(0) @binding(0) var<uniform> stamp: Stamp;
// A copy of this face from before the pass.
@group(0) @binding(1) var before: texture_2d<u32>;
// The faces past the +u, -u, +v, and -v edges of this face.
@group(0) @binding(2) var past_pu: texture_2d<u32>;
@group(0) @binding(3) var past_nu: texture_2d<u32>;
@group(0) @binding(4) var past_pv: texture_2d<u32>;
@group(0) @binding(5) var past_nv: texture_2d<u32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32(i & 1u), f32(i >> 1u)) * 4.0 - 1.0;
    return vec4<f32>(p, 0.0, 1.0);
}

// The flat coordinate of a texel index that can be past the face edge.
fn flat_of(i: i32) -> f32 {
    let s = clamp((f32(i) + 0.5) / f32(stamp.size) * 2.0 - 1.0, -1.99, 1.99);
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
    let axis = stamp.face / 2u;
    var d: vec3<f32>;
    d[axis] = 1.0 - 2.0 * f32(stamp.face % 2u);
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
    let texel = floor((warp + 1.0) * 0.5 * f32(stamp.size));
    let at = clamp(vec2<i32>(texel), vec2<i32>(0), vec2<i32>(stamp.size - 1));
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
    if all(p >= vec2<i32>(0)) && all(p < vec2<i32>(stamp.size)) {
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

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let at = vec2<i32>(floor(position.xy));
    let n = f32(stamp.size);
    // The angle from the brush center to the texel along each face axis. The
    // whole part of the center keeps the difference exact.
    let step = (vec2<f32>(at - stamp.center) + 0.5 - stamp.center_rest) * (2.0 * PI_4 / n);
    let theta = ((vec2<f32>(at) + 0.5) / n * 2.0 - 1.0) * PI_4;
    // The flat coordinates of the texel minus those of the center.
    let d = sin_series(step) / (cos_series(theta) * stamp.cos_center);
    let flat = stamp.flat + d;
    // The tangent of the angle between (1, center) and (1, texel), from the
    // cross product and the dot product.
    let twist = d.x * stamp.flat.y - d.y * stamp.flat.x;
    let across = sqrt(twist * twist + dot(d, d));
    let along = 1.0 + dot(flat, stamp.flat);
    if across >= MAX_TANGENT * along {
        discard;
    }
    let angle = atan_series(across / along);
    if angle >= stamp.radius {
        discard;
    }
    let k = angle / stamp.radius;
    var weight = 1.0;
    if k > stamp.hardness {
        let t = (k - stamp.hardness) / (1.0 - stamp.hardness);
        weight = 1.0 - t * t * (3.0 - 2.0 * t);
    }
    let amount = weight * stamp.flow;
    let old = f32(textureLoad(before, at, 0).r);
    var result = old;
    switch stamp.mode {
        case RAISE {
            result = old + amount * stamp.strength;
        }
        case LOWER {
            result = old - amount * stamp.strength;
        }
        case SMOOTH {
            let r = stamp.reach;
            let sum = level_before(at + vec2<i32>(-r, -r))
                + level_before(at + vec2<i32>(0, -r))
                + level_before(at + vec2<i32>(r, -r))
                + level_before(at + vec2<i32>(-r, 0))
                + level_before(at + vec2<i32>(r, 0))
                + level_before(at + vec2<i32>(-r, r))
                + level_before(at + vec2<i32>(0, r))
                + level_before(at + vec2<i32>(r, r))
                + old;
            result = old + (sum / 9.0 - old) * min(amount, 1.0);
        }
        default {
            result = old + (stamp.level - old) * min(amount, 1.0);
        }
    }
    // Round half up, as Rust `round` does for a positive number.
    return vec4<u32>(u32(floor(clamp(result, 0.0, 65535.0) + 0.5)), 0u, 0u, 0u);
}
