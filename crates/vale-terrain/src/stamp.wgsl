// A group of brush stamps on one face of the cube map. The result is the same
// as `Heightmap::stamp` for each stamp in order. `gpu.rs` fills the uniforms
// and sets the order of the passes. The source of `tiles.wgsl` comes before
// this file, and each read of a level goes through its `tile_level`.

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
    // The steepest slope that the weather mode leaves, in levels for each
    // texel step. It is a multiple of 1/16.
    talus: f32,
    // The size of the features of the noise of the warp mode as an angle,
    // and the seed of that noise.
    scale: f32,
    seed: u32,
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
    // 1 if the carve mode, the ridge mode, or the deposit mode has a window,
    // and 0 if not. In an erode pass, 1 if the cells of the step are the cells of a window.
    window_on: u32,
    // The first texel, the face, and the cell size of the window.
    window_origin: vec2<i32>,
    window_face: u32,
    window_cell: u32,
    // The texel of the face at the first texel of the render target.
    origin: vec2<i32>,
    // The number of cells of an erode pass along one side of a face.
    cells: u32,
    // The limits of the stamps of the group or of the erode pass. The low 16
    // bits are the floor, and the high 16 bits are the ceiling.
    limits: u32,
    // The texels of the drop texture that hold the drops of an erode pass:
    // x0, y0, x1, y1. The drop of each other cell is 0.
    drop_rect: vec4<i32>,
    stamps: array<Stamp, GROUP_STAMPS>,
}

const PI_4 = 0.7853981633974483;

const RAISE = 0u;
const LOWER = 1u;
const SMOOTH = 2u;
const CARVE = 4u;
// A pass of this mode has no stamps. It applies one step of the erode brush,
// as `Heightmap::erode` does.
const ERODE = 5u;
const WEATHER = 6u;
const WARP = 7u;
const RIDGE = 8u;
const DEPOSIT = 9u;
// A pass of this mode has no stamps. It lays the ocean ridge of `ridge`, as
// `Heightmap::ridge` does.
const OCEAN_RIDGE = 10u;

// The same number as `WARP_MARGIN` in `heightmap.rs`.
const WARP_MARGIN = 3.0;

@group(0) @binding(0) var<uniform> group: Group;
// The channel map of all faces. The third byte of a texel is the distance to
// the river with the deepest valley there, in channel texels, times 32. The
// fourth byte is the size of that river, or 0 if no river is near.
@group(0) @binding(1) var channels: texture_2d_array<u32>;
// The channel texels of the window, with the same bytes.
@group(0) @binding(2) var window_channels: texture_2d<u32>;
// The drops of an erode step on the cells of all faces. A face has one more
// cell at each side, from the faces that are there.
@group(0) @binding(3) var drops: texture_2d_array<u32>;
// The drops of an erode step on the cells of a window.
@group(0) @binding(4) var window_drops: texture_2d<u32>;
// The levels that the deposit mode fills the ground to, on the cells of all
// faces. A cell is 2 channel texels wide.
@group(0) @binding(5) var fills: texture_2d_array<u32>;
// The same levels on the cells of the window.
@group(0) @binding(6) var window_fills: texture_2d<u32>;

// One piece of the line of an ocean ridge. The numbers are those of
// `RidgeSegment::to_f32`.
struct RidgeSegment {
    n: vec3<f32>,
    // The distance along the line at the start of the piece, from the place
    // of the cell.
    rest: f32,
    m0: vec3<f32>,
    len: f32,
    m1: vec3<f32>,
    q0: f32,
    q1: f32,
    // 1 for a round cap at the start, plus 2 for a round cap at the end.
    caps: f32,
    cell: f32,
    unused: f32,
}

// The same number as `RIDGE_SEGMENTS` in `ridge.rs`.
const RIDGE_SEGMENTS = 128;

// The settings and the segments of a ridge pass.
struct RidgeLine {
    radius: f32,
    // The mean length of a crest segment.
    segment: f32,
    shift: f32,
    roughness: f32,
    valley: f32,
    crest: f32,
    // The texel step at the center of a face, and the sine of the reach.
    texel: f32,
    sin_reach: f32,
    seed: u32,
    count: u32,
    // The length of the line: the number of a multiple of the segment
    // length, and the distance from that multiple.
    length: vec2<f32>,
    segments: array<RidgeSegment, RIDGE_SEGMENTS>,
}

@group(0) @binding(7) var<uniform> ridge: RidgeLine;

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

// The direction of a texel of a face, not as a unit vector. The texel can be
// past the face edge.
fn texel_dir(face: u32, p: vec2<i32>) -> vec3<f32> {
    let axis = face / 2u;
    var d: vec3<f32>;
    d[axis] = 1.0 - 2.0 * f32(face % 2u);
    d[(axis + 1u) % 3u] = flat_of(p.x);
    d[(axis + 2u) % 3u] = flat_of(p.y);
    return d;
}

// The level of a texel past the face edge. The texel grid continues past the
// edge, and the read gets the nearest texel of the face that is there.
fn level_past_edge(p: vec2<i32>) -> u32 {
    let d = texel_dir(group.face, p);
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
    var face = 2 * i32(to);
    if d[to] < 0.0 {
        face += 1;
    }
    return tile_level(face, at);
}

fn level_before(p: vec2<i32>) -> f32 {
    if all(p >= vec2<i32>(0)) && all(p < vec2<i32>(group.size)) {
        return f32(tile_level(i32(group.face), p));
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

// The pair of a channel texel that the mode of the group reads. The ridge
// mode reads the nearest river. The carve mode reads the river with the
// deepest valley.
fn pair_of(texel: vec4<u32>) -> vec2<f32> {
    return vec2<f32>(select(texel.ba, texel.rg, group.mode == RIDGE));
}

// The distance from a texel to the river of its pair, in channel texels, and
// the size of that river. The steps are those of `ChannelMap::valley` and
// `ChannelMap::ridge`.
fn river_at(at: vec2<i32>) -> vec2<f32> {
    let size = f32(group.channel_size);
    let c = (vec2<f32>(at) + 0.5) * size / f32(group.size) - 0.5;
    let whole = floor(c);
    let t = c - whole;
    let last = vec2<i32>(i32(group.channel_size) - 1);
    let p0 = clamp(vec2<i32>(whole), vec2<i32>(0), last);
    let p1 = clamp(vec2<i32>(whole) + 1, vec2<i32>(0), last);
    let face = i32(group.face);
    let a = pair_of(textureLoad(channels, p0, face, 0));
    let b = pair_of(textureLoad(channels, vec2<i32>(p1.x, p0.y), face, 0));
    let c0 = pair_of(textureLoad(channels, vec2<i32>(p0.x, p1.y), face, 0));
    let d = pair_of(textureLoad(channels, p1, face, 0));
    // The products are exact when both sizes are powers of two. The builtin
    // `mix` can use other steps.
    let top = a * (1.0 - t.x) + b * t.x;
    let bottom = c0 * (1.0 - t.x) + d * t.x;
    let both = top * (1.0 - t.y) + bottom * t.y;
    return vec2<f32>(both.x / 32.0, both.y);
}

// The arc tangent of each number.
fn atan_any(t: f32) -> f32 {
    if abs(t) <= 1.0 {
        return atan_unit(t);
    }
    return sign(t) * (2.0 * PI_4 - atan_unit(1.0 / abs(t)));
}

// The place of a texel in the window, in cells. The center of the first cell
// is at 0.5. The third number is 1 if the texel has a place, and 0 if it is
// too far from the face of the window. The steps are those of `Window::place`.
fn window_place(at: vec2<i32>) -> vec3<f32> {
    let cell = f32(group.window_cell);
    if group.face == group.window_face {
        // The difference of the whole numbers is exact.
        return vec3<f32>((vec2<f32>(at - group.window_origin) + 0.5) / cell, 1.0);
    }
    // The direction of the texel, then its place on the face of the window.
    // The texel grid of that face continues past its edges.
    let d = texel_dir(group.face, at);
    let to = group.window_face / 2u;
    let depth = d[to] * (1.0 - 2.0 * f32(group.window_face % 2u));
    if depth <= 0.2 * length(d) {
        return vec3<f32>(0.0);
    }
    let flat = vec2<f32>(d[(to + 1u) % 3u], d[(to + 2u) % 3u]) / depth;
    let warp = vec2<f32>(atan_any(flat.x), atan_any(flat.y)) / PI_4;
    let texel = (warp + 1.0) * 0.5 * f32(group.size);
    return vec3<f32>((texel - vec2<f32>(group.window_origin)) / cell, 1.0);
}

// The place of a texel in the window, in channel texels of the window. The
// third number is 1 if the window covers the texel, and 0 if not. The steps
// are those of `Window::channel_of_texel`, `Window::channel_of_dir`, and
// `Window::covers`.
fn window_channel_at(at: vec2<i32>) -> vec3<f32> {
    if group.window_on == 0u {
        return vec3<f32>(0.0);
    }
    let place = window_place(at);
    if place.z == 0.0 {
        return vec3<f32>(0.0);
    }
    // A cell is 2 channel texels wide.
    let c = place.xy * 2.0 - 0.5;
    let last = f32(textureDimensions(window_channels).x) - 1.0 - WINDOW_MARGIN;
    if any(c < vec2<f32>(WINDOW_MARGIN)) || any(c > vec2<f32>(last)) {
        return vec3<f32>(0.0);
    }
    return vec3<f32>(c, 1.0);
}

// The distance from a texel to the river of its pair in the window, in
// channel texels of the window, and the size of that river from 0 to 255.
// The third number is 1 if the window covers the texel, and 0 if not. The
// steps are those of `ChannelWindow::valley` and `ChannelWindow::ridge`.
fn window_river_at(at: vec2<i32>) -> vec3<f32> {
    let place = window_channel_at(at);
    if place.z == 0.0 {
        return vec3<f32>(0.0);
    }
    let c = place.xy;
    let whole = floor(c);
    let t = c - whole;
    let p0 = vec2<i32>(whole);
    let p1 = p0 + 1;
    let a = pair_of(textureLoad(window_channels, p0, 0));
    let b = pair_of(textureLoad(window_channels, vec2<i32>(p1.x, p0.y), 0));
    let c0 = pair_of(textureLoad(window_channels, vec2<i32>(p0.x, p1.y), 0));
    let d = pair_of(textureLoad(window_channels, p1, 0));
    let top = a * (1.0 - t.x) + b * t.x;
    let bottom = c0 * (1.0 - t.x) + d * t.x;
    let both = top * (1.0 - t.y) + bottom * t.y;
    return vec3<f32>(both.x / 32.0, both.y, 1.0);
}

// The blend of 4 fill levels as a whole number. The steps are those of
// `fill_at` in `flow.rs`.
fn fill_blend(a: f32, b: f32, c: f32, d: f32, t: vec2<f32>) -> f32 {
    let top = a * (1.0 - t.x) + b * t.x;
    let bottom = c * (1.0 - t.x) + d * t.x;
    return floor(top * (1.0 - t.y) + bottom * t.y + 0.5);
}

// The fill level at a texel, from the cells of the face. The steps are those
// of `ChannelMap::fill_level`.
fn fill_at(at: vec2<i32>) -> f32 {
    let cells = i32(textureDimensions(fills).x);
    let c = (vec2<f32>(at) + 0.5) * f32(cells) / f32(group.size) - 0.5;
    let whole = floor(c);
    let last = vec2<i32>(cells - 1);
    let p0 = clamp(vec2<i32>(whole), vec2<i32>(0), last);
    let p1 = clamp(vec2<i32>(whole) + 1, vec2<i32>(0), last);
    let face = i32(group.face);
    return fill_blend(
        f32(textureLoad(fills, p0, face, 0).r),
        f32(textureLoad(fills, vec2<i32>(p1.x, p0.y), face, 0).r),
        f32(textureLoad(fills, vec2<i32>(p0.x, p1.y), face, 0).r),
        f32(textureLoad(fills, p1, face, 0).r),
        c - whole,
    );
}

// The fill level at a texel, from the cells of the window. The second number
// is 1 if the window covers the texel, and 0 if not. The steps are those of
// `ChannelWindow::fill_level`.
fn window_fill_at(at: vec2<i32>) -> vec2<f32> {
    let place = window_channel_at(at);
    if place.z == 0.0 {
        return vec2<f32>(0.0);
    }
    let c = place.xy / 2.0 - 0.25;
    let whole = floor(c);
    let last = vec2<i32>(textureDimensions(window_fills)) - 1;
    let p0 = clamp(vec2<i32>(whole), vec2<i32>(0), last);
    let p1 = clamp(vec2<i32>(whole) + 1, vec2<i32>(0), last);
    let level = fill_blend(
        f32(textureLoad(window_fills, p0, 0).r),
        f32(textureLoad(window_fills, vec2<i32>(p1.x, p0.y), 0).r),
        f32(textureLoad(window_fills, vec2<i32>(p0.x, p1.y), 0).r),
        f32(textureLoad(window_fills, p1, 0).r),
        c - whole,
    );
    return vec2<f32>(level, 1.0);
}

// The drop of a cell of an erode pass. The cell can be one place past the end
// of the grid.
fn cell_drop(cell: vec2<i32>) -> f32 {
    if group.window_on == 0u {
        let p = cell + 1;
        if any(p < group.drop_rect.xy) || any(p >= group.drop_rect.zw) {
            return 0.0;
        }
        return f32(textureLoad(drops, p, i32(group.face), 0).r);
    }
    if any(cell < group.drop_rect.xy) || any(cell >= group.drop_rect.zw) {
        return 0.0;
    }
    return f32(textureLoad(window_drops, cell, 0).r);
}

// The drop of an erode pass at a texel, in levels. The steps are those of
// `ErodeStep::drop_at`.
fn drop_at(at: vec2<i32>) -> f32 {
    var c: vec2<f32>;
    if group.window_on == 0u {
        c = (vec2<f32>(at) + 0.5) * f32(group.cells) / f32(group.size) - 0.5;
    } else {
        let place = window_place(at);
        if place.z == 0.0 {
            return 0.0;
        }
        c = place.xy - 0.5;
    }
    var wide = group.cells < u32(group.size);
    if group.window_on != 0u {
        wide = group.window_cell > 1u;
    }
    if !wide {
        let whole = floor(c);
        let t = c - whole;
        let p = vec2<i32>(whole);
        let top = cell_drop(p) * (1.0 - t.x) + cell_drop(p + vec2<i32>(1, 0)) * t.x;
        let bottom = cell_drop(p + vec2<i32>(0, 1)) * (1.0 - t.x) + cell_drop(p + 1) * t.x;
        return top * (1.0 - t.y) + bottom * t.y;
    }
    // The weights of a quadratic B-spline on the 9 cells around the texel.
    let whole = floor(c + 0.5);
    let t = c - whole;
    let p = vec2<i32>(whole);
    let wx = array<f32, 3>(0.5 * (0.5 - t.x) * (0.5 - t.x), 0.75 - t.x * t.x, 0.5 * (0.5 + t.x) * (0.5 + t.x));
    let wy = array<f32, 3>(0.5 * (0.5 - t.y) * (0.5 - t.y), 0.75 - t.y * t.y, 0.5 * (0.5 + t.y) * (0.5 + t.y));
    var drop = 0.0;
    for (var j = 0; j < 3; j++) {
        for (var i = 0; i < 3; i++) {
            drop += wx[i] * wy[j] * cell_drop(p + vec2<i32>(i - 1, j - 1));
        }
    }
    return drop;
}

// The effect of a stamp at a texel, from 0 to the flow of the stamp. It is 0
// outside the rectangle of the stamp and outside the brush circle.
fn amount_at(stamp: Stamp, p: vec2<i32>) -> f32 {
    return brush_at(stamp, p).x;
}

// The effect of a stamp at a texel, as `amount_at` gives it, and the angle
// from the brush center to the texel. Both are 0 outside the brush.
fn brush_at(stamp: Stamp, p: vec2<i32>) -> vec2<f32> {
    // `Heightmap::stamp` visits the texels of this rectangle only.
    if any(p < stamp.rect.xy) || any(p >= stamp.rect.zw) {
        return vec2<f32>(0.0);
    }
    let n = f32(group.size);
    let theta = ((vec2<f32>(p) + 0.5) / n * 2.0 - 1.0) * PI_4;
    let cos_theta = cos_series(theta);
    // The angle from the brush center to the texel along each face axis.
    // The whole part of the center keeps the difference exact.
    let step = (vec2<f32>(p - stamp.center) + 0.5 - stamp.center_rest) * (2.0 * PI_4 / n);
    // The flat coordinates of the texel minus those of the center.
    let d = sin_series(step) / (cos_theta * stamp.cos_center);
    let flat = stamp.flat + d;
    // The tangent of the angle between (1, center) and (1, texel), from
    // the cross product and the dot product.
    let twist = d.x * stamp.flat.y - d.y * stamp.flat.x;
    let across = sqrt(twist * twist + dot(d, d));
    let along = 1.0 + dot(flat, stamp.flat);
    if across >= MAX_TANGENT * along {
        return vec2<f32>(0.0);
    }
    let angle = atan_series(across / along);
    if angle >= stamp.radius {
        return vec2<f32>(0.0);
    }
    let k = angle / stamp.radius;
    var weight = 1.0;
    if k > stamp.hardness {
        let t = (k - stamp.hardness) / (1.0 - stamp.hardness);
        weight = 1.0 - t * t * (3.0 - 2.0 * t);
    }
    return vec2<f32>(weight * stamp.flow, angle);
}

// A hash of a lattice point. The steps are those of `hash` in
// `heightmap.rs`, and the bits are the same.
fn hash(i: vec3<u32>, seed: u32) -> u32 {
    var h = seed ^ 0x9e3779b9u ^ (i.x * 0x8da6b343u) ^ (i.y * 0xd8163841u) ^ (i.z * 0xcb1ab31fu);
    h ^= h >> 16u;
    h *= 0x7feb352du;
    h ^= h >> 15u;
    h *= 0x846ca68bu;
    return h ^ (h >> 16u);
}

// The noise vector of a lattice point, with each part from -1 to 1.
fn lattice_noise(i: vec3<u32>, seed: u32) -> vec3<f32> {
    let h = hash(i, seed);
    let k = vec3<u32>(h, h >> 10u, h >> 20u) & vec3<u32>(1023u);
    return (vec3<f32>(k) - 511.5) / 511.5;
}

// A noise vector at a point. The steps are those of `noise3` in
// `heightmap.rs`.
fn noise3(p: vec3<f32>, seed: u32) -> vec3<f32> {
    let cell = floor(p);
    // A cell below zero has the bits of its whole number, as in Rust.
    let i = vec3<u32>(vec3<i32>(cell));
    let rest = p - cell;
    let t = rest * rest * (3.0 - 2.0 * rest);
    var along_y = array<vec3<f32>, 2>();
    for (var z = 0u; z < 2u; z++) {
        var along_x = array<vec3<f32>, 2>();
        for (var y = 0u; y < 2u; y++) {
            let a = lattice_noise(i + vec3<u32>(0u, y, z), seed);
            let b = lattice_noise(i + vec3<u32>(1u, y, z), seed);
            along_x[y] = a + (b - a) * t.x;
        }
        along_y[z] = along_x[0] + (along_x[1] - along_x[0]) * t.y;
    }
    return along_y[0] + (along_y[1] - along_y[0]) * t.z;
}

// The face of a direction, and the place of the direction in the texels of
// that face. A texel center is at a whole number.
fn place_of(d: vec3<f32>) -> vec3<f32> {
    let size = abs(d);
    var to = 2u;
    if size.x >= size.y && size.x >= size.z {
        to = 0u;
    } else if size.y >= size.z {
        to = 1u;
    }
    var face = 2.0 * f32(to);
    if d[to] < 0.0 {
        face += 1.0;
    }
    let flat = vec2<f32>(d[(to + 1u) % 3u], d[(to + 2u) % 3u]) / size[to];
    let warp = vec2<f32>(atan_unit(flat.x), atan_unit(flat.y)) / PI_4;
    return vec3<f32>(face, (warp + 1.0) * 0.5 * f32(group.size) - 0.5);
}

// The level at a place between 4 texels, in the ratio of the distances to
// their centers. `rest` is the place from the first texel. The texels are in
// rows.
fn blend(levels: array<f32, 4>, rest: vec2<f32>) -> f32 {
    // With 4 equal levels, each step gives that level.
    let top = levels[0] + (levels[1] - levels[0]) * rest.x;
    let bottom = levels[2] + (levels[3] - levels[2]) * rest.x;
    return top + (bottom - top) * rest.y;
}

// The level at a direction, from the 4 texels around it on its face. The
// texels at the face edge continue past the edge.
fn level_on_face(d: vec3<f32>) -> f32 {
    let place = place_of(d);
    let whole = floor(place.yz);
    let last = vec2<i32>(group.size - 1);
    var levels = array<f32, 4>();
    for (var i = 0; i < 4; i++) {
        let p = vec2<i32>(whole) + vec2<i32>(i & 1, i >> 1u);
        levels[i] = f32(tile_level(i32(place.x), clamp(p, vec2<i32>(0), last)));
    }
    return blend(levels, place.yz - whole);
}

// The level at a direction, from the 4 texels around it on its face. The
// texel grid continues past the face edges, and the level of a texel there
// comes from `level_on_face`.
fn level_between(d: vec3<f32>) -> f32 {
    let place = place_of(d);
    let face = u32(place.x);
    let whole = floor(place.yz);
    var levels = array<f32, 4>();
    for (var i = 0; i < 4; i++) {
        let p = vec2<i32>(whole) + vec2<i32>(i & 1, i >> 1u);
        if all(p >= vec2<i32>(0)) && all(p < vec2<i32>(group.size)) {
            levels[i] = f32(tile_level(i32(face), p));
        } else {
            levels[i] = level_on_face(texel_dir(face, p));
        }
    }
    return blend(levels, place.yz - whole);
}

// The level of a texel after one warp stamp. The texel takes the level of
// the ground at an offset place. The steps are those of `warp_offset` and
// `Heightmap::level_between`.
fn warp_level(stamp: Stamp, at: vec2<i32>, old: f32) -> f32 {
    let brush = brush_at(stamp, at);
    let margin = WARP_MARGIN * 2.0 * PI_4 / f32(group.size);
    let len = min(stamp.strength * brush.x, stamp.radius - brush.y - margin);
    if len <= 0.0 {
        return old;
    }
    let p = normalize(texel_dir(group.face, at));
    let v = noise3(p / stamp.scale, stamp.seed);
    // The part of the noise vector along the ground.
    let t = v - p * dot(v, p);
    let source = p + t * (len / max(length(t), 1.0));
    return floor(level_between(source) + 0.5);
}

// The place of a direction on the line of a ridge. `cell` and `rest` give the
// distance along the line: the number of a multiple of the segment length,
// and the distance from that multiple.
struct RidgePoint {
    on: bool,
    cell: f32,
    rest: f32,
    across: f32,
    // The square of the distance past the end of the line.
    past2: f32,
    flow: f32,
    // The distance along the line to its nearer end.
    end: f32,
}

// The arc sine of a number below 0.3.
fn asin_small(x: f32) -> f32 {
    return atan_series(x / sqrt(1.0 - x * x));
}

// The angle between two unit vectors, or more than the reach if they are far
// apart.
fn angle_to(p: vec3<f32>, q: vec3<f32>) -> f32 {
    return 2.0 * asin_small(min(0.5 * length(p - q), 0.3));
}

// The place of a unit direction on the line. The steps are those of
// `RidgePlan::place`. The pool holds the ground from before the stroke, so a
// texel far from the new part of the line has no own case.
fn ridge_place(p: vec3<f32>) -> RidgePoint {
    let reach = 1.5 * ridge.radius;
    var best = RidgePoint(false, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    var best_distance = reach;
    for (var i = 0u; i < ridge.count; i++) {
        let s = ridge.segments[i];
        let side = dot(p, s.n);
        if abs(side) >= ridge.sin_reach {
            continue;
        }
        let across = asin_small(side);
        // The distance to the nearest point of the segment.
        let a = normalize(cross(s.m0, s.n));
        let b = normalize(cross(s.m1, s.n));
        var nearest = abs(across);
        if dot(p, cross(s.n, a)) < 0.0 {
            nearest = angle_to(p, a);
        } else if dot(p, cross(s.n, b)) > 0.0 {
            nearest = angle_to(p, b);
        }
        if nearest >= reach {
            continue;
        }
        let da = dot(p, s.m0);
        let db = -dot(p, s.m1);
        let caps = u32(s.caps);
        var along = 0.0;
        var distance = nearest;
        if (caps & 1u) != 0u && da < 0.0 {
            along = 0.0;
        } else if (caps & 2u) != 0u && db < 0.0 {
            along = 1.0;
        } else if da >= 0.0 && db >= 0.0 {
            if da + db > 0.0 {
                along = da / (da + db);
            }
            distance = max(abs(across), nearest * nearest / reach);
        } else {
            continue;
        }
        if distance < best_distance {
            best_distance = distance;
            // The numbers stay small, so the distance from the boundary of
            // the crest segment keeps its digits.
            let local = s.rest + along * s.len;
            let cells = floor(local / ridge.segment + 0.5);
            let rest = local - cells * ridge.segment;
            // Near an end, each of the two distances is small.
            let from_start = (s.cell + cells) * ridge.segment + rest;
            let to_end = (ridge.length.x - s.cell - cells) * ridge.segment
                + (ridge.length.y - rest);
            best = RidgePoint(
                true,
                s.cell + cells,
                rest,
                across,
                max(distance * distance - across * across, 0.0),
                s.q0 + (s.q1 - s.q0) * along,
                max(min(from_start, to_end), 0.0),
            );
        }
    }
    return best;
}

// A number from 0 to 1 from the low 24 bits of a hash.
fn unit(h: u32) -> f32 {
    return f32(h & 0xffffffu) / 16777216.0;
}

// The builtin `smoothstep` can use other steps.
fn smooth_between(low: f32, high: f32, x: f32) -> f32 {
    let t = clamp((x - low) / (high - low), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// `bell` in `ridge.rs`.
fn ridge_bell(x: f32) -> f32 {
    let rest = max(1.0 - x * x, 0.0);
    return rest * rest;
}

// `profile` in `ridge.rs`.
fn ridge_profile(x: f32) -> f32 {
    return sqrt(sqrt(x * x + 0.04)) - sqrt(0.2);
}

// The noise at the place of a texel, as `RidgeNoise` in `ridge.rs`.
struct RidgeNoise {
    bend: vec3<f32>,
    slow: vec3<f32>,
    mid: vec3<f32>,
}

// The steps are those of `RidgePlan::noise`.
fn ridge_noise(p: vec3<f32>) -> RidgeNoise {
    let r = ridge.radius;
    let wide = noise3(p / (0.9 * r), ridge.seed ^ 0x51ed270bu);
    let fine = noise3(p / (0.37 * r), ridge.seed ^ 0x2f6b8a35u);
    return RidgeNoise(
        wide + fine * 0.5,
        noise3(p / (1.7 * r), ridge.seed ^ 0x7a3c91d7u),
        noise3(p / (0.55 * r), ridge.seed ^ 0x4cf5ad43u),
    );
}

// A number from 0 to 1 for a whole number and a channel. A number below
// zero has the bits of its whole number, as in Rust.
fn ridge_unit(i: i32, channel: u32) -> f32 {
    return unit(hash(vec3<u32>(u32(i), channel, 0u), ridge.seed));
}

// `RidgePlan::group_start`.
fn ridge_group_start(c: i32) -> i32 {
    let h = hash(vec3<u32>(u32(c), 1u, 0u), ridge.seed);
    return c * 4 + i32(h & 3u);
}

// `RidgePlan::strength`.
fn ridge_strength(i: i32) -> f32 {
    let x = ridge_unit(i, 2u);
    if i == ridge_group_start(i >> 2u) {
        return 0.7 + 0.3 * x;
    }
    return 0.1 + 0.4 * x * x;
}

// `RidgePlan::shift`.
fn ridge_shift(k: i32) -> f32 {
    let c = k >> 2u;
    var group = c - 1;
    if k >= ridge_group_start(c) {
        group = c;
    }
    let wide = 2.0 * ridge_unit(group, 3u) - 1.0;
    let own = 2.0 * ridge_unit(k, 4u) - 1.0;
    return ridge.shift * (0.75 * wide + 0.25 * own);
}

// The move of boundary `i`, as a part of the segment length.
fn ridge_jitter(i: i32) -> f32 {
    return 0.35 * (2.0 * ridge_unit(i, 0u) - 1.0);
}

// The part of the strength of the crack at boundary `i` that a place gets
// at `x` of the crest segment from that boundary.
fn ridge_end(x: f32, i: i32) -> f32 {
    let near = max(1.0 - 2.0 * x, 0.0);
    let strength = ridge_strength(i);
    return near * near * strength * strength;
}

// Each number from -1 to 1 as a number from 0 to 1, with more of the
// numbers near the two ends.
fn ridge_gain(v: vec3<f32>) -> vec3<f32> {
    let t = clamp((v + 0.6) / 1.2, vec3<f32>(0.0), vec3<f32>(1.0));
    return t * t * (3.0 - 2.0 * t);
}

// One row of `brick` in `ridge.rs`.
fn brick_row(x: f32, j: u32, channel: u32, seed: u32) -> f32 {
    let moved = x + unit(hash(vec3<u32>(j, 0x5bd1e995u, channel), seed));
    let cell = floor(moved);
    let i = u32(i32(cell));
    let rest = moved - cell;
    let a = unit(hash(vec3<u32>(i, j, channel), seed));
    let b = unit(hash(vec3<u32>(i + 1u, j, channel), seed));
    return a + (b - a) * (rest * rest * (3.0 - 2.0 * rest));
}

// `brick` in `ridge.rs`.
fn brick(x: f32, y: f32, channel: u32, seed: u32) -> f32 {
    let cell = floor(y);
    let j = u32(i32(cell));
    let rest = y - cell;
    let a = brick_row(x, j, channel, seed);
    let b = brick_row(x, j + 1u, channel, seed);
    return a + (b - a) * (rest * rest * (3.0 - 2.0 * rest));
}

// The crack at boundary `j`, at `x` along the line from it. `before` is
// true on the side of the crest segment before the boundary. `base` is the
// depth of the strongest crack, and `narrow` makes a weak crack faint. The
// numbers are the depth of the trough at the place, the part of the trough
// from 0 to 1, and the height of the shoulder.
fn ridge_crack(j: i32, x: f32, before: bool, base: f32, narrow: f32) -> vec3<f32> {
    let strength = ridge_strength(j);
    let half = max(0.02 * ridge.radius, 3.0 * ridge.texel) * (1.0 + 1.2 * strength);
    let trough = ridge_bell(x / half);
    let faint = 1.0 - 0.9 * (1.0 - strength) * narrow;
    let depth = (0.02 + 0.3 * strength * strength) * base * faint;
    var shoulder = 0.0;
    let on_left = (hash(vec3<u32>(u32(j), 6u, 0u), ridge.seed) & 1u) == 1u;
    if on_left == before {
        shoulder = 0.4 * depth * ridge_bell((x / half - 1.6) / 0.9);
    }
    return vec3<f32>(depth * trough, trough, shoulder);
}

// The level of a texel on the ridge. `old` is its level from before the
// stroke. The steps are those of `RidgePlan::level`, without the limits.
fn ridge_level(at: RidgePoint, noise: RidgeNoise, old: f32) -> f32 {
    let r = ridge.radius;
    let t = ridge.texel;
    let seed = ridge.seed;
    let from_line = sqrt(at.across * at.across + at.past2);
    let taper = smooth_between(0.0, 1.2 * r, at.end);
    let lens = 0.2 + 0.8 * taper;
    let far = from_line / (r * lens);
    let hold = 1.0 - smooth_between(r, 1.5 * r, from_line);
    // The numbers stay small, so the distance from the boundary of the
    // crest segment keeps its digits.
    let part_of_reach = from_line / (1.5 * r);
    let curve = 0.5 * (noise.slow.y - noise.slow.z);
    let bent = at.rest
        + r * (0.08 * noise.bend.x + 0.3 * curve * part_of_reach * part_of_reach);
    let cells = floor(bent / ridge.segment + 0.5);
    let across = at.across + 0.08 * r * hold * noise.bend.y;
    let i = i32(at.cell + cells);
    let jitter = ridge_jitter(i);
    let s = bent - cells * ridge.segment - jitter * ridge.segment;
    var k = i;
    var along = s;
    var len = 0.0;
    if s < 0.0 {
        len = ridge.segment * (1.0 + jitter - ridge_jitter(i - 1));
        k = i - 1;
        along = s + len;
    } else {
        len = ridge.segment * (1.0 + ridge_jitter(i + 1) - jitter);
    }
    let part = clamp(along / len, 0.0, 1.0);
    let ends = ridge_end(part, k) + ridge_end(1.0 - part, k + 1);
    // The steps of the crest at 3 boundaries can reach a place.
    let zone = min(
        max(0.02 * r, 3.0 * t) + 0.6 * max(from_line - ridge.shift, 0.0),
        0.9 * ridge.segment,
    );
    var before = ridge_shift(i - 2);
    var shift = before;
    for (var near = -1; near <= 1; near++) {
        let j = i + near;
        let ahead = s + (jitter - f32(near) - ridge_jitter(j)) * ridge.segment;
        let next = ridge_shift(j);
        shift += (next - before) * smooth_between(-zone, zone, ahead);
        before = next;
    }
    let off = across - shift;
    let from_crest = sqrt(off * off + at.past2);
    let slow = ridge_gain(noise.slow);
    let mid = ridge_gain(noise.mid);
    let less = 0.5 * slow + 0.5 * mid;
    let height = 1.0 - 0.25 * less.x;
    var side = 0.0;
    if from_crest > 0.0 {
        side = off / from_crest;
    }
    let narrow = 0.5 * (less.y + less.z) + 0.5 * (less.y - less.z) * side;
    let width = r * (1.0 - 0.45 * narrow) * lens;
    let soft = off / sqrt(from_crest * from_crest + 0.0025 * r * r);
    let narrow_near = 0.5 * (less.y + less.z) + 0.5 * (less.y - less.z) * soft;
    let valley_width = max(0.05 * r * (1.0 + 0.4 * noise.bend.z), 2.0 * t);
    var top = 0.0;
    if ridge.valley > 0.0 {
        top = valley_width;
    }
    let x = min(max(from_crest - top, 0.0) / max(width - top, 0.25 * width), 1.0);
    let sharp = 1.0 - ridge_profile(x) / ridge_profile(1.0);
    let w = sharp * sharp * (1.0 + 0.25) / (sharp + 0.25);
    let lift = max(ridge.crest - old, 0.0);
    let rough = 1.9 * ridge.roughness + 1.4 * ridge.valley;
    let low = min(lift / (rough + 1.0), 1.0);
    let rise = (lift - low * 1.9 * ridge.roughness) * height * taper * w;
    let deep = (0.6 + 0.4 * ends) * (0.75 + noise.bend.z / 6.0);
    let in_valley = 1.0 - smooth_between(0.0, valley_width, from_crest);
    let valley = ridge.valley * low * deep * taper * in_valley;
    let rim_height = 0.5 * (mid.y + mid.z) + 0.5 * (mid.y - mid.z) * side;
    let rim_place = ridge_bell((from_crest - 1.6 * valley_width) / (1.1 * valley_width));
    let rim = 0.4 * ridge.valley * low * taper * rim_height * rim_place;
    // The crack at each end of the crest segment.
    let fade = taper * (1.0 - smooth_between(0.6, 1.5, far));
    let base = lift * (0.4 + 0.6 * w) * fade;
    let first = ridge_crack(k, along, false, base, narrow_near);
    let second = ridge_crack(k + 1, len - along, true, base, narrow_near);
    let crack = first.x + second.x;
    let open = (1.0 - first.y) * (1.0 - second.y);
    let shoulder = (first.z + second.z) * (1.0 - w);
    let finest = max(0.02 * r, 7.0 * t);
    // The weight of each octave of the hills, and the number of times that
    // its cell is longer along the line than across it.
    var octaves = array<vec2<f32>, 3>(
        vec2<f32>(1.0, 3.0),
        vec2<f32>(0.8, 2.5),
        vec2<f32>(0.6, 2.0),
    );
    var sum = 0.0;
    var total = 0.0;
    var moved = 0.0;
    for (var octave = 2; octave >= 0; octave--) {
        let size = finest * f32(1u << u32(octave));
        if octave > 0 && size > 0.15 * r {
            continue;
        }
        let channel = u32(k) * 4u + u32(octave);
        let n = brick(
            along / (octaves[octave].y * size),
            off / size + moved,
            channel,
            seed ^ 0x1b873593u,
        );
        moved = 0.5 * (2.0 * n - 1.0);
        sum += octaves[octave].x * smooth_between(0.25, 0.75, n);
        total += octaves[octave].x;
    }
    let near_crest = 1.0 - smooth_between(0.0, 0.3 * r, from_crest);
    let bury = smooth_between(0.5, 1.5, far);
    let lumps = max(sum / total - bury, 0.0) * open * (1.0 + 0.6 * near_crest);
    // The bands. The distance along the line is the one from the place of
    // the cell.
    let lane = (bent - cells * ridge.segment) / ridge.segment * 4.0;
    let lane_cell = floor(lane);
    let index = u32(i * 4 + i32(lane_cell));
    let lane_rest = lane - lane_cell;
    let band_a = unit(hash(vec3<u32>(index, 5u, 0u), seed ^ 0x1b873593u));
    let band_b = unit(hash(vec3<u32>(index + 1u, 5u, 0u), seed ^ 0x1b873593u));
    let band = band_a + (band_b - band_a) * (lane_rest * lane_rest * (3.0 - 2.0 * lane_rest));
    let bands = 0.3 * band * mid.x * (1.0 - bury);
    let hills = ridge.roughness * low * taper * (lumps + bands);
    let q = at.flow;
    let change = q * (max(rise + hills + rim - valley, 0.0) + shoulder - crack);
    return clamp(old + floor(change + 0.5), 0.0, 65535.0);
}

// The level of a texel after a change from `old` to `next`. The steps are
// those of `Limits::apply`.
fn limited(old: f32, next: f32) -> f32 {
    let low = f32(group.limits & 0xffffu);
    let high = f32(group.limits >> 16u);
    if next < old {
        return max(next, min(old, low));
    }
    return min(next, max(old, high));
}

// The change of the level of a texel from one weather stamp. Each pair of
// texels that are side by side moves a whole number of levels from the higher
// texel to the lower one. The limits bound the move. The steps are those of
// `Heightmap::weather_face`.
fn weather(stamp: Stamp, at: vec2<i32>, old: f32) -> f32 {
    // The texel, then its four neighbors. One call of `amount_at` in the
    // source gives the same bits for a texel in each pass that reads it, so
    // the two texels of a pair get the same flux.
    var steps = array<vec2<i32>, 5>(
        vec2<i32>(0, 0),
        vec2<i32>(-1, 0),
        vec2<i32>(1, 0),
        vec2<i32>(0, -1),
        vec2<i32>(0, 1),
    );
    let low = f32(group.limits & 0xffffu);
    let high = f32(group.limits >> 16u);
    var own = 0.0;
    var change = 0.0;
    for (var i = 0; i < 5; i++) {
        let p = at + steps[i];
        let amount = amount_at(stamp, p);
        if i == 0 {
            own = amount;
            continue;
        }
        // A pair with a texel outside the rectangle has no weight. The
        // rectangle is inside the face.
        let weight = min(min(own, amount), 1.0);
        if weight <= 0.0 {
            continue;
        }
        let other = f32(tile_level(i32(group.face), p));
        let drop = old - other;
        let over = abs(drop) - stamp.talus;
        if over > 0.0 {
            // The steps are those of `Limits::pair_room`.
            let down = max(max(old, other) - low, 0.0);
            let up = max(high - min(old, other), 0.0);
            let room = floor(min(down, up) * 0.25);
            change -= sign(drop) * min(floor(weight * over * 0.125), room);
        }
    }
    return change;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let at = vec2<i32>(floor(position.xy)) + group.origin;
    // The level after each stamp is a whole number, as it is in the texture
    // between two passes.
    var value = f32(tile_level(i32(group.face), at));
    if group.mode == ERODE {
        // The lowest level is 0. A texel above the floor does not go below
        // it, and a texel at or below the floor stays.
        let drop = floor(drop_at(at) + 0.5);
        value = max(value - drop, min(value, f32(group.limits & 0xffffu)));
    }
    if group.mode == OCEAN_RIDGE {
        let p = normalize(texel_dir(group.face, at));
        let point = ridge_place(p);
        if point.on {
            value = limited(value, ridge_level(point, ridge_noise(p), value));
        }
        return vec4<u32>(u32(value), 0u, 0u, 0u);
    }
    // The distance to the river and its size. The window comes first. With
    // no river near, the size is 0.
    var river = vec2<f32>(0.0);
    if group.mode == CARVE || group.mode == RIDGE {
        let in_window = window_river_at(at);
        if in_window.z > 0.0 {
            river = in_window.xy;
        } else {
            river = river_at(at);
        }
    }
    // The fill level. The window comes first.
    var fill = 0.0;
    if group.mode == DEPOSIT {
        let in_window = window_fill_at(at);
        if in_window.y > 0.0 {
            fill = in_window.x;
        } else {
            fill = fill_at(at);
        }
    }
    for (var i = 0u; i < group.count; i++) {
        let stamp = group.stamps[i];
        if group.mode == WEATHER {
            // The change is a whole number, and the new level is between
            // the old levels of the pair texels.
            value += weather(stamp, at, value);
            continue;
        }
        if group.mode == WARP {
            value = limited(value, warp_level(stamp, at, value));
            continue;
        }
        // With no effect, each mode keeps the level.
        let amount = amount_at(stamp, at);
        if amount == 0.0 {
            continue;
        }
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
            case RIDGE {
                // The sea floor stays. The distance is 32 times the byte of
                // the texel, and the largest byte is 255.
                if old > f32(group.sea) {
                    change = amount * stamp.strength * (river.x * 32.0 / 255.0);
                }
            }
            case DEPOSIT {
                // The sea floor stays, and so does ground at or above the
                // fill level.
                if old >= f32(group.sea) {
                    change = max(fill - old, 0.0) * min(amount, 1.0);
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
        value = limited(old, value);
    }
    // A copy moves each texel of the rectangle to the tiles, so a texel with
    // no change keeps its level here.
    return vec4<u32>(u32(value), 0u, 0u, 0u);
}
