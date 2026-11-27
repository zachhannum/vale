// The smooth display of a raster layer. Put `TILES_WGSL` before this file.
// `pyramid.rs` fills group 2. `smooth.rs` has the same functions on the CPU.
//
// `n` is the face size of level 0, and level `k` has `max(1, n >> k)` texels
// along one side. A value is from 0 to 1. The levels below `tiles.levels`
// come from the tiles, and the higher levels from the pyramid.
//
// - `field(d, n, lod, max_level)`: the value at a world direction with a
//   blend of levels, and its slope across one texel of level 0.
// - `shown_field(d, n, land_lod, sea_lod, max_level, sea, depth)`: `field`
//   at `land_lod` above sea level, with a blend to `field` at `sea_lod`
//   below it.
// - `height(d, n, level)`: the value at one level, and its slope across one
//   texel of that level. The value is the uniform cubic B-spline of 4 by 4
//   texels, so a band edge is a smooth curve across the texels.
// - `texel(face, x, y, m, level)`: one texel. A texel one or two steps past
//   the face comes from the next face.
// - `place(d)`: the face of a direction, and the place on that face.

// The levels from 1 up. Mip level `k - 1` holds level `k`.
@group(2) @binding(0) var pyramid: texture_2d_array<u32>;

const FIELD_QUARTER_PI: f32 = 0.7853981633974483;

// The value of a texel at a level. The texel is on the face. A level below
// `tiles.levels` comes from the tiles.
fn level_load(face: i32, x: i32, y: i32, level: u32) -> f32 {
    if level < tiles.levels {
        return f32(tile_level_of(face, vec2<i32>(x, y), level)) / 65535.0;
    }
    return f32(textureLoad(pyramid, vec2<i32>(x, y), face, i32(level) - 1).r) / 65535.0;
}

// The flat face coordinate at the center of a texel on a face of `m` texels.
// A texel past the face gives the face edge.
fn flat_at(i: i32, m: f32) -> f32 {
    let s = (f32(i) + 0.5) / m * 2.0 - 1.0;
    return tan(clamp(s, -1.0, 1.0) * FIELD_QUARTER_PI);
}

// The texel of the next face at a texel past a face edge. `x` or `y` is one
// or two texels past the face. The texel one step past the face is the
// texel of the next face that touches the edge there, and the texel two
// steps past is the texel after it, away from the edge.
fn across(face: i32, x: i32, y: i32, m: i32, level: u32) -> f32 {
    let axis = face / 2;
    // The point on the edge, next to the two texels.
    var p: array<f32, 3>;
    p[axis] = 1.0 - 2.0 * f32(face % 2);
    p[(axis + 1) % 3] = flat_at(x, f32(m));
    p[(axis + 2) % 3] = flat_at(y, f32(m));
    var next = (axis + 1) % 3;
    var past = x;
    if y < 0 || y >= m {
        next = (axis + 2) % 3;
        past = y;
    }
    // The number of steps past the face, after the first.
    let extra = select(past - m, -past - 1, past < 0);
    var to = next * 2;
    if p[next] < 0.0 {
        to += 1;
    }
    let f = vec2<f32>(p[(next + 1) % 3], p[(next + 2) % 3]) / abs(p[next]);
    let s = atan(f) / FIELD_QUARTER_PI;
    let i = vec2<i32>(floor((s * 0.5 + 0.5) * f32(m)));
    var c = clamp(i, vec2<i32>(0), vec2<i32>(m - 1));
    // The axis of this face goes away from the edge on the next face.
    let step = extra * i32(p[axis]);
    if (next + 1) % 3 == axis {
        c.x = clamp(c.x - step, 0, m - 1);
    } else {
        c.y = clamp(c.y - step, 0, m - 1);
    }
    return level_load(to, c.x, c.y, level);
}

// The value of a texel on a face of `m` texels. A texel one or two steps
// past the face comes from the next face, so the filter has no seam.
fn texel(face: i32, x: i32, y: i32, m: i32, level: u32) -> f32 {
    let last = m - 1;
    let cx = clamp(x, 0, last);
    let cy = clamp(y, 0, last);
    if x == cx && y == cy {
        return level_load(face, x, y, level);
    }
    if x != cx && y != cy {
        // Three faces meet at a cube corner, so no texel is there. A texel
        // `kx` and `ky` steps past the corner is the mean of the texel `kx`
        // and `ky` steps in from the corner and of that texel turned a
        // quarter turn each way, so each face finds the same values near
        // the corner.
        let kx = select(x - last, -x, x < 0);
        let ky = select(y - last, -y, y < 0);
        let in_x = clamp(select(m - kx, kx - 1, x < 0), 0, last);
        let in_y = clamp(select(m - ky, ky - 1, y < 0), 0, last);
        let turn_x = clamp(select(m - ky, ky - 1, x < 0), 0, last);
        let turn_y = clamp(select(m - kx, kx - 1, y < 0), 0, last);
        let past_x = select(last + ky, -ky, x < 0);
        let past_y = select(last + kx, -kx, y < 0);
        let sum = level_load(face, in_x, in_y, level) + across(face, turn_x, past_y, m, level)
            + across(face, past_x, turn_y, m, level);
        return sum / 3.0;
    }
    return across(face, x, y, m, level);
}

struct Place {
    face: i32,
    // The place on the face, from 0 to 1 along each axis.
    at: vec2<f32>,
}

// The place of a world direction on its cube face.
fn place(d: vec3<f32>) -> Place {
    let a = abs(d);
    var axis = 2;
    if a.x >= a.y && a.x >= a.z {
        axis = 0;
    } else if a.y >= a.z {
        axis = 1;
    }
    var face = axis * 2;
    if d[axis] < 0.0 {
        face += 1;
    }
    let flat = vec2<f32>(d[(axis + 1) % 3], d[(axis + 2) % 3]) / a[axis];
    let s = atan(flat) / FIELD_QUARTER_PI;
    return Place(face, s * 0.5 + 0.5);
}

// The weights of the uniform cubic B-spline at `t` from 0 to 1, for the
// texels at -1, 0, 1, and 2.
fn spline_weights(t: f32) -> vec4<f32> {
    let s = 1.0 - t;
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4<f32>(s * s * s, 3.0 * t3 - 6.0 * t2 + 4.0, 3.0 * (t + t2 - t3) + 1.0, t3) / 6.0;
}

// The derivatives of `spline_weights` by `t`.
fn spline_slopes(t: f32) -> vec4<f32> {
    let s = 1.0 - t;
    let t2 = t * t;
    return vec4<f32>(-s * s, 3.0 * t2 - 4.0 * t, 1.0 + 2.0 * t - 3.0 * t2, t2) * 0.5;
}

// The value at a world direction at one level, from 4 by 4 texels. The y
// and z parts are the change of the value across one texel of that level,
// from the derivative of the same spline.
fn height(d: vec3<f32>, n: u32, level: u32) -> vec3<f32> {
    let m = i32(max(1u, n >> level));
    let p = place(d);
    let face = p.face;
    let f = p.at * f32(m) - 0.5;
    let i = vec2<i32>(floor(f)) - 1;
    let t = f - floor(f);
    let wx = spline_weights(t.x);
    let sx = spline_slopes(t.x);
    let wy = spline_weights(t.y);
    let sy = spline_slopes(t.y);
    // Most reads have all 16 texels on the face.
    let inside = all(i >= vec2<i32>(0)) && all(i + 3 < vec2<i32>(m));
    var rows: array<vec4<f32>, 4>;
    for (var r = 0; r < 4; r++) {
        let y = i.y + r;
        if inside {
            rows[r] = vec4<f32>(
                level_load(face, i.x, y, level),
                level_load(face, i.x + 1, y, level),
                level_load(face, i.x + 2, y, level),
                level_load(face, i.x + 3, y, level),
            );
        } else {
            rows[r] = vec4<f32>(
                texel(face, i.x, y, m, level),
                texel(face, i.x + 1, y, m, level),
                texel(face, i.x + 2, y, m, level),
                texel(face, i.x + 3, y, m, level),
            );
        }
    }
    // The sum is of the differences from one texel, so flat ground keeps
    // its exact value.
    let base = rows[1].y;
    var h = vec3<f32>(0.0);
    for (var r = 0; r < 4; r++) {
        let row = rows[r] - base;
        let v = dot(wx, row);
        h += vec3<f32>(wy[r] * v, wy[r] * dot(sx, row), sy[r] * v);
    }
    return vec3<f32>(base + h.x, h.yz);
}

// `height` at a level, with the slope across one texel of level 0.
fn level_height(d: vec3<f32>, n: u32, level: u32) -> vec3<f32> {
    let h = height(d, n, level);
    return vec3<f32>(h.x, h.yz / f32(1u << level));
}

// The value at a world direction at a fractional level `lod`, and its slope
// across one texel of level 0. A `lod` of 0 or less reads level 0 alone.
// Between two whole levels, the result is the blend of both. `max_level` is
// the coarsest level in the pyramid.
fn field(d: vec3<f32>, n: u32, lod: f32, max_level: u32) -> vec3<f32> {
    if !(lod > 0.0) || max_level == 0u {
        return height(d, n, 0u);
    }
    let l = min(lod, f32(max_level));
    let k = u32(floor(l));
    if k >= max_level {
        return level_height(d, n, max_level);
    }
    let a = level_height(d, n, k);
    let b = level_height(d, n, k + 1u);
    return mix(a, b, l - f32(k));
}

// The value that the display of a height layer shows at a world direction,
// and its slope across one texel of level 0. At or above the sea level
// `sea`, it is `field` at `land_lod`. Below it, it goes from that field at
// the coast to `field` at `sea_lod` at `depth` below sea level, along a
// smoothstep of the depth. The sea field is held at least one value step
// below sea level, so the result stays below sea level. A `depth` below 0
// means no sea. `combine_sea` in `smooth.rs` is the same on the CPU.
fn shown_field(
    d: vec3<f32>,
    n: u32,
    land_lod: f32,
    sea_lod: f32,
    max_level: u32,
    sea: f32,
    depth: f32,
) -> vec3<f32> {
    let land = field(d, n, land_lod, max_level);
    if depth < 0.0 || land_lod == sea_lod {
        return land;
    }
    let below = land.x - sea;
    if !(below < 0.0) {
        return land;
    }
    // The blend `a` and its derivative `da` by the land value.
    var a = 1.0;
    var da = 0.0;
    if depth > 0.0 {
        let t = min(-below / depth, 1.0);
        a = t * t * (3.0 - 2.0 * t);
        da = -6.0 * t * (1.0 - t) / depth;
    }
    if !(a > 0.0) {
        return land;
    }
    let c = field(d, n, sea_lod, max_level);
    let step = 1.0 / 65535.0;
    let deep = c.x - sea;
    let clamped = deep < -step;
    let m = select(-step, deep, clamped);
    let sea_slope = select(vec2<f32>(0.0), c.yz, clamped);
    let slope = (1.0 - a) * land.yz + a * sea_slope + da * (m - below) * land.yz;
    return vec3<f32>(sea + (1.0 - a) * below + a * m, slope);
}
