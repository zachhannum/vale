// The smooth display of a raster layer. Put `TILES_WGSL` before this file.
// `pyramid.rs` fills group 2. `smooth.rs` has the same functions on the CPU.
//
// `n` is the face size of level 0, and level `k` has `max(1, n >> k)` texels
// along one side. A value is from 0 to 1. The levels below `tiles.levels`
// come from the tiles, and the higher levels from the pyramid.
//
// - `field(d, n, lod, max_level)`: the value at a world direction with a
//   blend of levels, and its slope across one texel of level 0.
// - `height(d, n, level)`: the bilinear value at one level, and its slope
//   across one texel of that level.
// - `texel(face, x, y, m, level)`: one texel. A texel one step past the face
//   comes from the next face.
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

// The texel of the next face that touches a texel on a face edge. `x` or `y`
// is one texel past the face.
fn across(face: i32, x: i32, y: i32, m: i32, level: u32) -> f32 {
    let axis = face / 2;
    // The point on the edge, next to the two texels.
    var p: array<f32, 3>;
    p[axis] = 1.0 - 2.0 * f32(face % 2);
    p[(axis + 1) % 3] = flat_at(x, f32(m));
    p[(axis + 2) % 3] = flat_at(y, f32(m));
    var next = (axis + 1) % 3;
    if y < 0 || y >= m {
        next = (axis + 2) % 3;
    }
    var to = next * 2;
    if p[next] < 0.0 {
        to += 1;
    }
    let f = vec2<f32>(p[(next + 1) % 3], p[(next + 2) % 3]) / abs(p[next]);
    let s = atan(f) / FIELD_QUARTER_PI;
    let i = vec2<i32>(floor((s * 0.5 + 0.5) * f32(m)));
    let c = clamp(i, vec2<i32>(0), vec2<i32>(m - 1));
    return level_load(to, c.x, c.y, level);
}

// The value of a texel on a face of `m` texels. A texel one step past the
// face comes from the next face, so the filter has no seam.
fn texel(face: i32, x: i32, y: i32, m: i32, level: u32) -> f32 {
    let last = m - 1;
    let cx = clamp(x, 0, last);
    let cy = clamp(y, 0, last);
    if x == cx && y == cy {
        return level_load(face, x, y, level);
    }
    if x != cx && y != cy {
        // Three faces meet at a cube corner, so no texel is there. The mean
        // of the three corner texels is the same from each face.
        let sum = level_load(face, cx, cy, level) + across(face, x, cy, m, level)
            + across(face, cx, y, m, level);
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

// The bilinear value at a world direction at one level. The y and z parts
// are the change of the value across one texel of that level.
fn height(d: vec3<f32>, n: u32, level: u32) -> vec3<f32> {
    let m = i32(max(1u, n >> level));
    let p = place(d);
    let face = p.face;
    let f = p.at * f32(m) - 0.5;
    let i = vec2<i32>(floor(f));
    let t = f - floor(f);
    let h00 = texel(face, i.x, i.y, m, level);
    let h10 = texel(face, i.x + 1, i.y, m, level);
    let h01 = texel(face, i.x, i.y + 1, m, level);
    let h11 = texel(face, i.x + 1, i.y + 1, m, level);
    let slope = vec2<f32>(mix(h10 - h00, h11 - h01, t.y), mix(h01 - h00, h11 - h10, t.x));
    return vec3<f32>(mix(mix(h00, h10, t.x), mix(h01, h11, t.x), t.y), slope);
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
