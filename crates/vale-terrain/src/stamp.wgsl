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
    // The motion of the push mode, as in `Stamp::motion`.
    motion: vec3<f32>,
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
const PUSH = 11u;

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

// The settings and the segments of a ridge pass, with the numbers of
// `RidgePlan::level` that are the same for each texel. The numbers are those
// of `RidgeUniforms` in `gpu.rs`.
struct RidgeLine {
    radius: f32,
    // The mean length of a crest segment.
    segment: f32,
    // The texel step at the center of a face, and the sine of the reach.
    texel: f32,
    sin_reach: f32,
    crest: f32,
    world_km: f32,
    spread: f32,
    offset: f32,
    seed: u32,
    count: u32,
    group: i32,
    level: f32,
    // The length of the line: the number of a multiple of the segment
    // length, and the distance from that multiple.
    length: vec2<f32>,
    round: f32,
    top: f32,
    foot: f32,
    mean: f32,
    magma: f32,
    spacing: f32,
    mean_throw: f32,
    heave: f32,
    box_half: f32,
    half_texel: f32,
    turn_cell: f32,
    blend: f32,
    valley: f32,
    high: f32,
    valley_part: f32,
    flat_floor: f32,
    band_km: f32,
    segment_depth: f32,
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

// The level of a texel after one push stamp. The texel takes the level of
// the ground behind it along the motion. The steps are those of
// `push_offset` and `Heightmap::level_between`.
fn push_level(stamp: Stamp, at: vec2<i32>, old: f32) -> f32 {
    let brush = brush_at(stamp, at);
    let margin = WARP_MARGIN * 2.0 * PI_4 / f32(group.size);
    let len = min(length(stamp.motion) * brush.x, stamp.radius - brush.y - margin);
    let p = normalize(texel_dir(group.face, at));
    // The part of the motion along the ground.
    let t = stamp.motion - p * dot(stamp.motion, p);
    let full = length(t);
    if len <= 0.0 || full == 0.0 {
        return old;
    }
    return floor(level_between(p - t * (len / full)) + 0.5);
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

// The builtin functions of a driver can be wrong in the fifth digit, and a
// level of the ridge needs about the sixth. These functions replace them.

// The exponential function. The steps split the number into a power of 2
// and a rest of less than half of ln 2.
fn exp_precise(x: f32) -> f32 {
    if x < -80.0 {
        return 0.0;
    }
    let k = round(x * 1.4426950408889634);
    // The high part of ln 2 has few bits, so its product with `k` is exact.
    let r = (x - k * 0.693145751953125) - k * 1.4286068203094172e-6;
    let e = 1.0 + r * (1.0 + r * (1.0 / 2.0 + r * (1.0 / 6.0 + r * (1.0 / 24.0
        + r * (1.0 / 120.0 + r * (1.0 / 720.0 + r * (1.0 / 5040.0 + r / 40320.0)))))));
    return ldexp(e, i32(k));
}

// The natural logarithm of a number above zero.
fn ln_precise(x: f32) -> f32 {
    let parts = frexp(x);
    var m = parts.fract;
    var e = parts.exp;
    if m < 0.70710678 {
        m *= 2.0;
        e -= 1;
    }
    // ln m = 2 atanh z.
    let z = (m - 1.0) / (m + 1.0);
    let q = z * z;
    let s = 2.0 * z * (1.0 + q * (1.0 / 3.0 + q * (1.0 / 5.0 + q * (1.0 / 7.0
        + q * (1.0 / 9.0 + q / 11.0)))));
    return f32(e) * 0.6931471805599453 + s;
}

// `x` to the power `y`, for `x` of zero or more. Zero to a power above zero
// is zero.
fn pow_precise(x: f32, y: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    return exp_precise(y * ln_precise(x));
}

// The cosine of `TAU * t`.
fn cos_turn(t: f32) -> f32 {
    let half_angle = 4.0 * PI_4 * (t - round(t));
    let s = sin_series(vec2<f32>(half_angle)).x;
    return 1.0 - 2.0 * s * s;
}

// The hyperbolic tangent of a number of zero or more.
fn tanh_precise(x: f32) -> f32 {
    let e = exp_precise(-2.0 * min(x, 40.0));
    return (1.0 - e) / (1.0 + e);
}

// The constants of `ridge.rs`, with the same names. `RIDGE_REACH` is `REACH`.
const RIDGE_REACH = 1.5;
const BEND_ALONG = 0.08;
const BEND_ACROSS = 0.08;
const CURVE = 0.3;
const SALT_FAULT = 0x1b873593u;
const SALT_TURN = 0x6c8e9cf5u;
const SALT_MIGRATE = 0x3d4fa1c7u;
const SALT_MAGMA = 0x9b05688cu;
const SALT_WALL = 0x4c95e1a3u;
const SALT_LUMP = 0x2d8e7f19u;
const SALT_FRACTURE = 0x5f3ac2e1u;
const JITTER = 0.3;
const SECOND_ORDER_KM = 130.0;
const GROUP_SHIFT = 0.8;
const TAN_TURN = 0.14;
const MIGRATE = 0.1;
const MIGRATE_MYR = 4.0;
const WANDER_KM = 3.0;
const WANDER_MYR = 0.8;
const MIGRATE_ROUND = 0.5;
const SHORTEST_PART = 0.2;
const SEGMENT_MAGMA = 0.03;
const KEEP_SOFT = 0.1;
const MAGMA_MYR = 2.0;
const MAGMA_BAND = 0.26;
const THROW_SPACING = 0.65;
const MAGMA_RELIEF = 0.12;
const MAGMA_MEAN = 0.5;
const WAVE_MYR = 0.8;
const WAVE_KM = 40.0;
const SEGMENT_MYR = 6.0;
const SEGMENT_CHANGE = 0.35;
const HOOK_PART = 0.15;
const HOOK_ROWS = 1.0;
const TROUGH_DEPTH = 1.3;
const TROUGH_OFFSET_KM = 100.0;
const TROUGH_POWER = 1.3;
const STRENGTH = vec2<f32>(0.5, 1.6);
const TROUGH_MAX = 1.5;
const OFFSET_TROUGH = 0.8;
const TROUGH_KM = 10.0;
const TROUGH_WIDENS = 0.8;
const TROUGH_KM_RANGE = vec2<f32>(2.0, 20.0);
const TROUGH_FLOOR = 0.3;
const ACTIVE = 0.4;
const ACTIVE_MYR = 3.0;
const PATCH_MYR = 4.0;
const PATCH_OLD_MYR = 10.0;
const PATCH = 0.5;
const TROUGH_FILL = 0.4;
const TROUGH_MYR = 20.0;
const TRANSVERSE_SHARE = 0.5;
const TRANSVERSE = 0.6;
const TRANSVERSE_AT = 1.4;
const TRANSVERSE_WIDTH = 0.9;
const NODAL_DEPTH = 0.005;
const NODAL_MAX = 0.6;
const NODAL_KM = 12.0;
const NODAL_ALONG_KM = 30.0;
const NODAL_IN = 0.3;
const HIGH_KM = 5.0;
const FLOOR_HALF = vec2<f32>(5.0, 8.0);
const WALL_WIDTH = vec2<f32>(10.0, 14.0);
const VALLEY_ALONG = 0.15;
const DOWN = 0.85;
const DOWN_KM = 35.0;
const BAND = 0.25;
const WALL_FAULTS = 4;
const STAIR = 0.65;
const MOUNT_KM = 24.0;
const WALL_JITTER = 0.3;
const WALL_PIECE = vec2<f32>(15.0, 40.0);
const JOG = 0.25;
const RISE_BEND_KM = 70.0;
const VALLEY_END = 0.5;
const INSIDE_CORNER = 1.25;
const ABOVE_CREST = 0.2;
const LUMP_KM = 4.0;
const LUMP_TEXELS = 1.2;
const LUMP_HEIGHT = 0.25;
const LUMP_SIZE = 0.4;
const AVR_HEIGHT = 0.25;
const AVR_KM = 2.0;
const AVR_END = 0.12;
const AVR_PIECE_KM = 18.0;
const AVR_GAPS = 0.3;
const SEDIMENT_RATE = 0.005;
const SEDIMENT_SOFT = 0.3;
const POND = 0.3;
const HILL_LOW = 1.0;
const TAPER = 1.2;
const TAPER_WIDTH = 0.2;
const FOOT = 0.25;
const DIP_TAN = 1.2;
const THROW_MIN = 0.6;
const THROW_MAX = 3.0;
const FAULT_JITTER = 0.4;
const INWARD = 0.8;
const GAMMA = 0.02;
const PIECE_MIN = 10.0;
const PIECE_MAX = 30.0;
const PIECE_JITTER = 0.15;
const OVERLAP = 0.3;
const STEP_OVER = 0.3;
const BOW = 0.2;
const PLACE_SPREAD = 0.5 + FAULT_JITTER + STEP_OVER + BOW;
const BLOCK_WIDTH = 2.5;
const WIDTH_MIN = 1.0;
const WIDTH_MAX = 5.0;
const SCARP_MAX = 0.4;
const BLOCK_MEAN = 1.08;
const HILL_TOP = 3.6;
const GROW = 1.5;
const HILL_FADE = 0.8;
const MAX_SMOOTH = 0.3;
const CAP_KNEE = 0.85;
const FAULT_SPACING = array<vec2<f32>, 5>(
    vec2<f32>(0.5, 8.0),
    vec2<f32>(0.6, 5.0),
    vec2<f32>(0.7, 4.0),
    vec2<f32>(0.8, 3.0),
    vec2<f32>(0.9, 2.0),
);
const FAULT_HEAVE = array<vec2<f32>, 5>(
    vec2<f32>(0.5, 5.0),
    vec2<f32>(0.6, 2.5),
    vec2<f32>(0.7, 1.2),
    vec2<f32>(0.8, 0.7),
    vec2<f32>(0.9, 0.3),
);

// `lerp_table` in `ridge.rs`, for a table of 5 points.
fn lerp_table(table: array<vec2<f32>, 5>, x: f32) -> f32 {
    var t = table;
    if x <= t[0].x {
        return t[0].y;
    }
    for (var i = 0; i < 4; i++) {
        if x <= t[i + 1].x {
            let a = t[i];
            let b = t[i + 1];
            return a.y + (b.y - a.y) * (x - a.x) / (b.x - a.x);
        }
    }
    return t[4].y;
}

// `depth` in `ridge.rs`.
fn ridge_depth(age: f32) -> f32 {
    if age < 20.0 {
        return 2600.0 + 365.0 * sqrt(age);
    }
    return 5651.0 - 2473.0 * exp_precise(-0.0278 * age);
}

// `mix` in `ridge.rs`. The builtin `mix` can use other steps.
fn ridge_mix(a: f32, b: f32, t: f32) -> f32 {
    return a + (b - a) * t;
}

// `bump` in `ridge.rs`.
fn bump(q: f32) -> f32 {
    let x = max(1.0 - q * q, 0.0);
    return x * x;
}

// `soft_cap` in `ridge.rs`.
fn soft_cap(value: f32, cap: f32) -> f32 {
    let knee = CAP_KNEE * cap;
    if value <= knee {
        return value;
    }
    let room = cap - knee;
    if room <= 0.0 {
        return knee;
    }
    return knee + room * tanh_precise((value - knee) / room);
}

// `smooth_max` in `ridge.rs`.
fn smooth_max(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return max(a, b);
    }
    let h = max(k - abs(a - b), 0.0) / k;
    return max(a, b) + 0.25 * k * h * h;
}

// `pond` in `ridge.rs`.
fn pond(height: f32, level: f32) -> f32 {
    let full = smooth_max(height, level, SEDIMENT_SOFT * level);
    return height + POND * (full - height);
}

// `block` in `ridge.rs`. `BACK` is 1, so the back of a block is a square.
fn block_area(y: f32, scarp: f32, w: f32) -> f32 {
    if y <= -scarp {
        return 0.0;
    } else if y < 0.0 {
        return (y + scarp) * (y + scarp) / (2.0 * scarp);
    } else if y < w {
        let rest = 1.0 - y / w;
        return 0.5 * scarp + w / 2.0 * (1.0 - rest * rest);
    }
    return 0.5 * scarp + w / 2.0;
}

fn block(y: f32, scarp: f32, w: f32, b: f32) -> f32 {
    if y + b <= -scarp || y - b >= w {
        return 0.0;
    }
    return (block_area(y + b, scarp, w) - block_area(y - b, scarp, w)) / (2.0 * b);
}

// `ellipse` in `ridge.rs`. The arc sine is the arc tangent of `x / s`, so
// an error of `s` changes the area little.
fn ellipse_area(x: f32) -> f32 {
    let c = clamp(x, -1.0, 1.0);
    let s = sqrt(max(1.0 - c * c, 0.0));
    var angle = sign(c) * 2.0 * PI_4;
    if s > 0.0 {
        angle = atan_any(c / s);
    }
    return 0.5 * (c * s + angle);
}

fn ellipse(q: f32, b: f32) -> f32 {
    return (ellipse_area(q + b) - ellipse_area(q - b)) / (2.0 * b);
}

// A hash of a whole number and two other numbers. A number below zero has
// the bits of its whole number, as in Rust.
fn ridge_hash(a: i32, b: u32, c: u32, seed: u32) -> u32 {
    return hash(vec3<u32>(bitcast<u32>(a), b, c), seed);
}

// `noise1` in `ridge.rs`: the value and its change.
fn noise1(x: f32, a: u32, b: u32, seed: u32) -> vec2<f32> {
    let n = floor(x);
    let s = x - n;
    let c = i32(n);
    let h0 = 2.0 * unit(hash(vec3<u32>(bitcast<u32>(c), a, b), seed)) - 1.0;
    let h1 = 2.0 * unit(hash(vec3<u32>(bitcast<u32>(c + 1), a, b), seed)) - 1.0;
    return vec2<f32>(h0 + (h1 - h0) * s * s * (3.0 - 2.0 * s), (h1 - h0) * 6.0 * s * (1.0 - s));
}

// `noise1_round` in `ridge.rs`.
fn noise1_round(x: f32, a: u32, b: u32, seed: u32) -> vec2<f32> {
    let n = floor(x);
    let s = x - n;
    let c = i32(n);
    var h: array<f32, 4>;
    for (var i = 0; i < 4; i++) {
        h[i] = 2.0 * unit(hash(vec3<u32>(bitcast<u32>(c + i - 1), a, b), seed)) - 1.0;
    }
    let m1 = 0.5 * (h[2] - h[0]);
    let m2 = 0.5 * (h[3] - h[1]);
    let s2 = s * s;
    let s3 = s * s * s;
    let value = (2.0 * s3 - 3.0 * s2 + 1.0) * h[1]
        + (s3 - 2.0 * s2 + s) * m1
        + (-2.0 * s3 + 3.0 * s2) * h[2]
        + (s3 - s2) * m2;
    let slope = (6.0 * s2 - 6.0 * s) * (h[1] - h[2])
        + (3.0 * s2 - 4.0 * s + 1.0) * m1
        + (3.0 * s2 - 2.0 * s) * m2;
    return vec2<f32>(clamp(value, -1.0, 1.0), slope);
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
    let reach = RIDGE_REACH * ridge.radius;
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

// The noise at the place of a texel, as `RidgeNoise` in `ridge.rs`.
struct RidgeNoise {
    bend: vec3<f32>,
    slow: vec3<f32>,
}

// The steps are those of `RidgePlan::noise`.
fn ridge_noise(p: vec3<f32>) -> RidgeNoise {
    let r = ridge.radius;
    let wide = noise3(p / (0.9 * r), ridge.seed ^ 0x51ed270bu);
    let fine = noise3(p / (0.37 * r), ridge.seed ^ 0x2f6b8a35u);
    return RidgeNoise(wide + fine * 0.5, noise3(p / (1.7 * r), ridge.seed ^ 0x7a3c91d7u));
}

// The shader keeps each distance along the line as a distance from the
// start of crest cell `cell`: the place `cell * ridge.segment` with no
// bend. Thus the numbers stay small.

// `RidgePlan::boundary`, from the start of crest cell `cell`.
fn ridge_boundary(j: i32, cell: i32) -> f32 {
    let jitter = 2.0 * unit(ridge_hash(j, 0u, 0u, ridge.seed)) - 1.0;
    return (f32(j - cell) + JITTER * jitter) * ridge.segment;
}

// `RidgePlan::group_start`.
fn ridge_group_start(c: i32) -> i32 {
    let choices = u32(ridge.group / 2 + 1);
    let h = ridge_hash(c, 1u, 0u, ridge.seed);
    return c * ridge.group + i32(h % choices);
}

// `RidgePlan::group`. The division rounds down, as `div_euclid` does.
fn ridge_group(k: i32) -> i32 {
    var c = k / ridge.group;
    if c * ridge.group > k {
        c -= 1;
    }
    if k >= ridge_group_start(c) {
        return c;
    }
    return c - 1;
}

// `RidgePlan::transform`.
fn ridge_transform(j: i32) -> bool {
    return j == ridge_group_start(ridge_group(j));
}

// `RidgePlan::group_shift`.
fn ridge_group_shift(c: i32) -> f32 {
    let part = 2.0 * unit(ridge_hash(c, 3u, 0u, ridge.seed)) - 1.0;
    return ridge.offset * GROUP_SHIFT * part;
}

// `RidgePlan::shift`.
fn ridge_shift(k: i32) -> f32 {
    let own = 2.0 * unit(ridge_hash(k, 4u, 0u, ridge.seed)) - 1.0;
    return ridge_group_shift(ridge_group(k)) + ridge.offset * (1.0 - GROUP_SHIFT) * own;
}

// `RidgePlan::turn`: the move and its change.
fn ridge_turn(tau: f32) -> vec2<f32> {
    let seed = ridge.seed ^ SALT_TURN;
    let x = tau / ridge.turn_cell;
    let nf = floor(x);
    let s = x - nf;
    let n = i32(nf);
    var sum = 0.0;
    var last = TAN_TURN * (2.0 * unit(ridge_hash(min(n, 0), 5u, 0u, seed)) - 1.0);
    for (var c = min(n, 0); c < max(n, 0); c++) {
        let next = TAN_TURN * (2.0 * unit(ridge_hash(c + 1, 5u, 0u, seed)) - 1.0);
        sum += 0.5 * (last + next);
        last = next;
    }
    if n < 0 {
        sum = -sum;
    }
    let h0 = TAN_TURN * (2.0 * unit(ridge_hash(n, 5u, 0u, seed)) - 1.0);
    let h1 = TAN_TURN * (2.0 * unit(ridge_hash(n + 1, 5u, 0u, seed)) - 1.0);
    sum += h0 * s + (h1 - h0) * s * s * s * (1.0 - 0.5 * s);
    let slope = h0 + (h1 - h0) * s * s * (3.0 - 2.0 * s);
    return vec2<f32>(ridge.spread * ridge.turn_cell * sum, slope);
}

// `RidgePlan::migration`.
fn ridge_migration(j: i32, tau: f32) -> vec2<f32> {
    let seed = ridge.seed ^ SALT_MIGRATE;
    let wander = WANDER_KM / ridge.world_km;
    var migrate = MIGRATE * ridge.segment;
    if ridge_transform(j) {
        migrate = 0.0;
    }
    let a = bitcast<u32>(j);
    let slow = noise1(tau / MIGRATE_MYR, a, 0u, seed);
    let slow_zero = noise1(0.0, a, 0u, seed).x;
    let small = noise1(tau / WANDER_MYR, a, 1u, seed);
    let small_zero = noise1(0.0, a, 1u, seed).x;
    var sum = vec2<f32>(0.0);
    sum.x += 0.5 * migrate * (slow.x - slow_zero);
    sum.y += 0.5 * migrate * slow.y / MIGRATE_MYR;
    sum.x += 0.5 * wander * (small.x - small_zero);
    sum.y += 0.5 * wander * small.y / WANDER_MYR;
    return sum;
}

// `RidgePlan::trace`: the place along the line from the start of crest cell
// `cell`, its change with the distance across the line, and the age.
fn ridge_trace(j: i32, cell: i32, across: f32, turn: vec2<f32>) -> vec3<f32> {
    let v = ridge.spread;
    let middle = 0.5 * (ridge_shift(j - 1) + ridge_shift(j));
    let off = across - middle;
    let tau_off = off / v;
    let tau = sqrt(tau_off * tau_off + MIGRATE_ROUND * MIGRATE_ROUND) - MIGRATE_ROUND;
    let moved = ridge_migration(j, tau);
    let to_tau = off / v / (tau + MIGRATE_ROUND);
    return vec3<f32>(
        ridge_boundary(j, cell) + turn.x + moved.x,
        turn.y + moved.y * to_tau / v,
        tau,
    );
}

// `RidgePlan::fracture`: the depth, the half width, the height of the
// transverse ridge, and its side.
fn ridge_fracture(j: i32, offset_km: f32, is_transform: bool, age: f32) -> vec4<f32> {
    let seed = ridge.seed ^ SALT_FRACTURE;
    let size = offset_km / TROUGH_OFFSET_KM;
    let strength = ridge_mix(STRENGTH.x, STRENGTH.y, unit(ridge_hash(j, 0u, 0u, seed)));
    var kind = OFFSET_TROUGH;
    if is_transform {
        kind = 1.0;
    }
    let leave = 0.5 * offset_km / (ridge.spread * ridge.world_km);
    let fresh = 1.0 + ACTIVE * (1.0 - smooth_between(leave, leave + ACTIVE_MYR, age));
    let patches = noise1(age / PATCH_MYR, bitcast<u32>(j), 1u, seed).x;
    let patchy = PATCH * smooth_between(0.0, PATCH_OLD_MYR, age);
    let keep = 1.0 - patchy * (1.0 - smooth_between(-0.5, 0.5, patches));
    let filled = 1.0 - TROUGH_FILL * smooth_between(0.0, TROUGH_MYR, age);
    let full = TROUGH_DEPTH * pow_precise(size, TROUGH_POWER) * strength * kind * fresh;
    let depth = min(full, TROUGH_MAX) * keep * filled;
    let half_len = clamp(
        TROUGH_KM * pow_precise(size * strength * kind, TROUGH_WIDENS),
        TROUGH_KM_RANGE.x,
        TROUGH_KM_RANGE.y,
    );
    var height = 0.0;
    if is_transform && unit(ridge_hash(j, 2u, 0u, seed)) < TRANSVERSE_SHARE {
        height = TRANSVERSE * depth;
    }
    var side = 1.0;
    if unit(ridge_hash(j, 3u, 0u, seed)) < 0.5 {
        side = -1.0;
    }
    return vec4<f32>(depth, half_len, height, side);
}

// A throw of `RidgePlan::fault_sum` from a number from 0 to 1.
fn ridge_throw(u: f32) -> f32 {
    return ridge.mean_throw * (THROW_MIN + (1.0 - THROW_MIN) * min(-ln_precise(1.0 - u), THROW_MAX));
}

// `RidgePlan::faults`.
fn ridge_faults(k: i32, side: u32, along: f32, len: f32, x: f32) -> f32 {
    let seed = ridge.seed ^ SALT_FAULT;
    let b = ridge.box_half;
    let key = bitcast<u32>(k) * 32u + side * 16u;
    let reach = max(WIDTH_MAX, SCARP_MAX) + b;
    let first = max(i32(floor(x - PLACE_SPREAD - reach)), 0);
    let last = i32(floor(x + PLACE_SPREAD - 1.0 + reach));
    let spacing = ridge.spacing;
    let heave = ridge.heave;
    var sum = 0.0;
    for (var i = first; i <= last; i++) {
        let piece = clamp(
            ridge_throw(unit(hash(vec3<u32>(bitcast<u32>(i), 0xffffffffu, key), seed))) / GAMMA,
            PIECE_MIN,
            PIECE_MAX,
        );
        let phase = unit(hash(vec3<u32>(bitcast<u32>(i), 0xffffffffu, key + 1u), seed));
        let cell = i32(floor(along / piece + phase));
        for (var j = cell - 1; j <= cell + 1; j++) {
            let at = vec2<u32>(bitcast<u32>(i), bitcast<u32>(j));
            let middle = f32(j) + 0.5 - phase
                + PIECE_JITTER * (2.0 * unit(hash(vec3<u32>(at, key + 3u), seed)) - 1.0);
            let half_len = 0.5 * (1.0 + OVERLAP * unit(hash(vec3<u32>(at, key + 4u), seed)));
            let soft_end = min(ridge.half_texel, MAX_SMOOTH * piece);
            let start = (middle - half_len) * piece;
            let end = (middle + half_len) * piece;
            if along <= start - soft_end || along >= end + soft_end {
                continue;
            }
            let place = middle * piece / len;
            let magma = ridge.magma + SEGMENT_MAGMA * cos_turn(place - 0.5);
            let grown = lerp_table(FAULT_HEAVE, magma) / heave;
            let keep = min(spacing / lerp_table(FAULT_SPACING, magma), 1.0);
            let kept = smooth_between(
                -KEEP_SOFT,
                KEEP_SOFT,
                keep - unit(hash(vec3<u32>(at, key + 8u), seed)),
            );
            let q = (2.0 * along - start - end) / (end - start);
            let bow = max(1.0 - q * q, 0.0);
            let taper = ellipse(q, 2.0 * soft_end / (end - start));
            let height = ridge_throw(unit(hash(vec3<u32>(at, key + 5u), seed))) * grown;
            let inward = unit(hash(vec3<u32>(at, key + 2u), seed)) < INWARD;
            let place_across = f32(i) + 0.5
                + FAULT_JITTER * (2.0 * unit(hash(vec3<u32>(at, key + 1u), seed)) - 1.0)
                + STEP_OVER * (2.0 * unit(hash(vec3<u32>(at, key + 6u), seed)) - 1.0)
                + BOW * (2.0 * unit(hash(vec3<u32>(at, key + 7u), seed)) - 1.0) * bow;
            let scarp = clamp(height / (DIP_TAN * spacing), 1e-6, SCARP_MAX);
            var y = place_across - x;
            if inward {
                y = x - place_across;
            }
            let width = clamp(BLOCK_WIDTH * sqrt(height / ridge.mean_throw), WIDTH_MIN, WIDTH_MAX);
            sum += height * taper * kept * block(y, scarp, width, b);
        }
    }
    return sum;
}

// `RidgePlan::walls`.
fn ridge_walls(band_group: i32, side: u32, along: f32, x: f32, floor_half: f32, wall: f32) -> f32 {
    let seed = ridge.seed ^ SALT_WALL;
    let key = (bitcast<u32>(band_group) * 2u + side) * 8u;
    let row = wall / f32(WALL_FAULTS);
    var sum = 0.0;
    for (var n = 0; n < WALL_FAULTS; n++) {
        let fault = bitcast<u32>(n);
        let piece = ridge_mix(
            WALL_PIECE.x,
            WALL_PIECE.y,
            unit(hash(vec3<u32>(fault, 0xffffffffu, key), seed)),
        );
        let phase = unit(hash(vec3<u32>(fault, 0xffffffffu, key + 1u), seed));
        let cell = i32(floor(along / piece + phase));
        for (var j = cell - 1; j <= cell + 1; j++) {
            let at = vec2<u32>(fault, bitcast<u32>(j));
            let middle = f32(j) + 0.5 - phase
                + PIECE_JITTER * (2.0 * unit(hash(vec3<u32>(at, key + 2u), seed)) - 1.0);
            let half_len = 0.5 * (1.0 + OVERLAP * unit(hash(vec3<u32>(at, key + 3u), seed)));
            let soft_end = min(ridge.half_texel, MAX_SMOOTH * piece);
            let start = (middle - half_len) * piece;
            let end = (middle + half_len) * piece;
            if along <= start - soft_end || along >= end + soft_end {
                continue;
            }
            let q = (2.0 * along - start - end) / (end - start);
            let taper = ellipse(q, 2.0 * soft_end / (end - start));
            let fault_throw = (0.5 + unit(hash(vec3<u32>(at, key + 4u), seed))) / f32(WALL_FAULTS);
            let jitter = WALL_JITTER * (2.0 * unit(hash(vec3<u32>(at, key + 5u), seed)) - 1.0);
            let place = floor_half + row * (f32(n) + 0.5 + jitter);
            let scarp = max(fault_throw * row / DIP_TAN, 1e-3);
            sum += fault_throw * taper * block(x - place, scarp, MOUNT_KM, ridge.half_texel);
        }
    }
    return sum;
}

// `RidgePlan::lumps`.
fn ridge_lumps(band_group: i32, along: f32, across: f32, cell: f32) -> f32 {
    let seed = ridge.seed ^ SALT_LUMP;
    let a = along / cell;
    let c = across / cell;
    let ia = i32(floor(a));
    let ic = i32(floor(c));
    let key = bitcast<u32>(band_group) * 4u;
    var sum = 0.0;
    for (var i = ia - 1; i <= ia + 1; i++) {
        for (var j = ic - 1; j <= ic + 1; j++) {
            let at = vec2<u32>(bitcast<u32>(i), bitcast<u32>(j));
            let pa = f32(i) + unit(hash(vec3<u32>(at, key), seed));
            let pc = f32(j) + unit(hash(vec3<u32>(at, key + 1u), seed));
            let radius = ridge_mix(LUMP_SIZE, 1.0, unit(hash(vec3<u32>(at, key + 2u), seed)));
            let tall = unit(hash(vec3<u32>(at, key + 3u), seed));
            let da = a - pa;
            let dc = c - pc;
            sum += tall * tall * bump(sqrt(da * da + dc * dc) / radius);
        }
    }
    return sum;
}

// The numbers of a texel that `ridge_crest_segment` takes.
struct RidgeTexel {
    // The distance along the line with the bend, from the start of crest
    // cell `cell`.
    u: f32,
    cell: i32,
    across: f32,
    past2: f32,
    lift: f32,
    low: f32,
    lens: f32,
    taper: f32,
    fade: f32,
}

// The age of the sea floor at a distance from the crest, as `age` in
// `RidgePlan::level_in`.
fn ridge_age(d: f32) -> f32 {
    let a = d / ridge.spread;
    return sqrt(a * a + ridge.round * ridge.round) - ridge.round;
}

// The rise, the blocks, and the axial high of crest segment `k`, from the
// trace `lower` at its start to the trace `upper` at its end. The steps are
// those of `segment` in `RidgePlan::level_in`.
fn ridge_crest_segment(at: RidgeTexel, k: i32, lower: f32, upper: f32) -> vec3<f32> {
    let km = ridge.world_km;
    let v = ridge.spread;
    let level = ridge.level;
    let u = at.u;
    let across = at.across;
    let magma_seed = ridge.seed ^ SALT_MAGMA;
    let crest = ridge_shift(k);
    let off = across - crest;
    let from_crest = sqrt(off * off + at.past2);
    let len = max(upper - lower, SHORTEST_PART * ridge.segment);
    let place = clamp((u - lower) / len, 0.0, 1.0);
    let ends = 0.5 * (1.0 + cos_turn(place));
    let side = select(0u, 1u, off < 0.0);
    let tau = from_crest / v;
    let end = select(k + 1, k, place < 0.5);
    let middle = 0.5 * (ridge_shift(end - 1) + ridge_shift(end));
    let deeper_noise = noise1(
        (across - middle) / v / SEGMENT_MYR,
        bitcast<u32>(end),
        4u,
        magma_seed,
    ).x;
    let deeper = 1.0 - SEGMENT_CHANGE * (1.0 - deeper_noise);
    let sunk = ridge.segment_depth * clamp(len * km / SECOND_ORDER_KM, 0.5, 1.5)
        * level * ends * deeper;
    var near = 1.0 - place;
    var next = k + 1;
    if place < 0.5 {
        near = place;
        next = k - 1;
    }
    let hook = max(1.0 - near / HOOK_PART, 0.0);
    let is_transform = ridge_transform(end);
    var fade_out = 0.0;
    var jog = 0.5 * (1.0 - smooth_between(0.0, JOG, near));
    if is_transform {
        fade_out = smooth_between(0.0, 1.0, hook);
        jog = 0.0;
    }
    let next_shift = ridge_shift(next);
    let valley_axis = crest + (next_shift - crest) * jog;
    let valley_off = across - valley_axis;
    let from_axis = sqrt(valley_off * valley_off + at.past2) * km;
    let floor_half = ridge_mix(FLOOR_HALF.x, FLOOR_HALF.y, ends);
    let wall = ridge_mix(WALL_WIDTH.x, WALL_WIDTH.y, ends);
    let grow_end = floor_half + GROW * ridge.spacing + ridge.texel * km;
    let grow = smooth_between(0.5 * floor_half, grow_end, min(from_axis, from_crest * km));
    let flat_part = grow * (ridge.mean + MAGMA_MEAN * MAGMA_RELIEF * level);
    let near_axis = 1.0
        - ridge.valley_part * (1.0 - smooth_between(0.0, RISE_BEND_KM, from_crest * km));
    let from_rise = max(ridge_mix(from_axis / km, from_crest, near_axis), ridge.flat_floor)
        / at.lens;
    let top = ridge.top;
    let foot = ridge.foot;
    let w0 = clamp((foot - ridge_depth(ridge_age(from_rise))) / (foot - top), 0.0, 1.0);
    let w = w0 * w0 * (1.0 + FOOT) / (w0 + FOOT);
    let rise = at.taper * max(w * (at.lift - at.low * flat_part) - at.low * sunk * w * (2.0 - w), 0.0);
    let band_group = ridge_group(k);
    let center = ridge_group_shift(band_group);
    let origin = ridge_boundary(ridge_group_start(band_group), at.cell);
    let group_along = (u - origin) * km;
    let wave = noise1(group_along / WAVE_KM, bitcast<u32>(band_group), 2u, magma_seed).x;
    let band = noise1_round(
        ((across - center) / v + WAVE_MYR * wave) / MAGMA_MYR,
        bitcast<u32>(band_group),
        0u,
        magma_seed,
    );
    let zero = noise1_round(
        ((crest - center) / v + WAVE_MYR * wave) / MAGMA_MYR,
        bitcast<u32>(band_group),
        0u,
        magma_seed,
    ).x;
    let away = select(1.0, -1.0, off < 0.0);
    let rows = away * MAGMA_BAND * MAGMA_MYR * v * km / ridge.spacing;
    let rate = 1.0 + MAGMA_BAND * band.y;
    let grown = pow_precise(rate, -THROW_SPACING);
    let toward = select(1.0, -1.0, (next_shift - crest) * off > 0.0);
    let x = from_crest * km / ridge.spacing + rows * (band.x - zero) + HOOK_ROWS * hook * hook * toward;
    let faults = grow * grown * ridge_faults(k, side, (u - lower) * km, len * km, x);
    let held = BLOCK_MEAN * ridge.mean_throw * grow * grown;
    let fill = max(2.0 * SEDIMENT_RATE * tau - HILL_LOW * held, 0.0);
    let blocks = pond(faults, fill) - (pond(held, fill) - held)
        + grow * MAGMA_RELIEF * 0.5 * (1.0 + band.x);
    let valley = ridge.valley * (1.0 + VALLEY_ALONG * (2.0 * ends - 1.0));
    let down = DOWN * valley * (1.0 - VALLEY_END * fade_out) * bump(from_axis / DOWN_KM);
    var inside = 0.0;
    if toward < 0.0 {
        inside = INSIDE_CORNER;
    }
    let valley_side = select(0u, 1u, valley_off < 0.0);
    let stairs = STAIR * valley * (1.0 + (inside - 1.0) * fade_out)
        * ridge_walls(band_group, valley_side, group_along, from_axis, floor_half, wall);
    let on_floor = 1.0 - smooth_between(floor_half, floor_half + 0.5 * wall, from_axis);
    let cell = max(LUMP_KM, 2.0 * LUMP_TEXELS * ridge.half_texel);
    let lumps = ridge_lumps(band_group, group_along, valley_off * km, cell);
    let gaps = noise1(group_along / AVR_PIECE_KM, bitcast<u32>(k), 9u, magma_seed).x;
    let ridge_top = smooth_between(AVR_END, 0.5, near) * smooth_between(-AVR_GAPS, AVR_GAPS, gaps);
    let axial_ridge = AVR_HEIGHT * ridge_top * bump(from_axis / AVR_KM);
    let floor_height = LUMP_HEIGHT * lumps * on_floor + axial_ridge;
    let past_walls = smooth_between(floor_half, floor_half + wall, from_axis);
    let young = BAND * valley * past_walls * bump(from_axis / ridge.band_km);
    let relief = stairs - down + young + floor_height * (1.0 - fade_out);
    let axis = level * at.low * at.taper;
    return vec3<f32>(
        rise,
        blocks * level * at.low * at.taper * at.fade + relief * axis,
        ridge.high * bump(from_axis / HIGH_KM) * axis,
    );
}

// The level of a texel on the ridge. `old` is its level from before the
// stroke. The steps are those of `RidgePlan::level`, without the limits.
fn ridge_level(at: RidgePoint, noise: RidgeNoise, old: f32) -> f32 {
    let r = ridge.radius;
    let km = ridge.world_km;
    let v = ridge.spread;
    let level = ridge.level;
    let cell_len = ridge.segment;
    let from_line = sqrt(at.across * at.across + at.past2);
    let taper = smooth_between(0.0, TAPER * r, at.end);
    let lens = TAPER_WIDTH + (1.0 - TAPER_WIDTH) * taper;
    let far = from_line / (r * lens);
    let hold = 1.0 - smooth_between(r, RIDGE_REACH * r, from_line);
    // `RidgePlan::bent`.
    let far_line = from_line / (RIDGE_REACH * r);
    let curve = 0.5 * (noise.slow.y - noise.slow.z);
    let bent = at.rest + r * (BEND_ALONG * noise.bend.x + CURVE * curve * far_line * far_line);
    let cells = floor(bent / cell_len + 0.5);
    let cell = i32(at.cell + cells);
    let u = bent - cells * cell_len;
    let across = at.across + BEND_ACROSS * r * hold * noise.bend.y;
    let lift = max(ridge.crest - old, 0.0);
    let low = min(lift / (HILL_TOP * ridge.mean + 1.0), 1.0);
    let fade = 1.0 - smooth_between(HILL_FADE, RIDGE_REACH, far);
    let texel = RidgeTexel(u, cell, across, at.past2, lift, low, lens, taper, fade);
    let blend_width = ridge.blend;
    let turn = ridge_turn(across / v);
    let i = cell + i32(floor((u - turn.x) / cell_len));
    var rise = 0.0;
    var hills = 0.0;
    var deep = 0.0;
    var above = 0.0;
    var ridges = 0.0;
    var below = 0.0;
    var before = 0.0;
    for (var j = i - 1; j <= i + 2; j++) {
        let trace = ridge_trace(j, cell, across, turn);
        let off = (u - trace.x) / sqrt(trace.y * trace.y + 1.0);
        let past = smooth_between(-blend_width, blend_width, off);
        if j > i - 1 {
            let weight = before - past;
            if weight > 0.0 {
                let parts = ridge_crest_segment(texel, j - 1, below, trace.x);
                rise += weight * parts.x;
                hills += weight * parts.y;
                above += weight * parts.z;
            }
        }
        below = trace.x;
        before = past;
        let shift_before = ridge_shift(j - 1);
        let shift_after = ridge_shift(j);
        let offset_km = abs(shift_after - shift_before) * km;
        let is_transform = ridge_transform(j);
        let fz = ridge_fracture(j, offset_km, is_transform, trace.z);
        let width = 0.5 * blend_width + fz.y / km;
        deep += fz.x * (1.0 - smooth_between(TROUGH_FLOOR, 1.0, abs(off / width)));
        let ridge_at = off - fz.w * TRANSVERSE_AT * width;
        ridges += fz.z * bump(ridge_at / (TRANSVERSE_WIDTH * width));
        if is_transform {
            let nodal = min(NODAL_DEPTH * offset_km, NODAL_MAX);
            let size = blend_width + NODAL_KM / km;
            let long_size = blend_width + NODAL_ALONG_KM / km;
            for (var end = 0; end < 2; end++) {
                var crest = shift_before;
                var other = shift_after;
                if end == 1 {
                    crest = shift_after;
                    other = shift_before;
                }
                // `signum` in Rust is 1 for zero.
                let toward = select(-1.0, 1.0, other - crest >= 0.0);
                let middle = crest + toward * NODAL_IN * long_size;
                let a = off / size;
                let b = (across - middle) / long_size;
                deep += nodal * bump(sqrt(a * a + b * b));
            }
        }
    }
    let axis = level * low * taper;
    let room = ABOVE_CREST * ridge.valley * axis;
    let capped = soft_cap(hills + ridges * axis * fade, max(lift - rise, 0.0) + room);
    let up = min(rise + capped, lift + room) - deep * axis * fade + above;
    let change = at.flow * up;
    return clamp(old + floor(change + 0.5), 0.0, 65535.0);
}

// The level of a texel after a change from `old` to `next`. The steps are
// those of `Limits::apply`.
fn limited(old: f32, next: f32) -> f32 {
    let low = f32(group.limits & 0xffffu);
    let high = f32(group.limits >> 16u);
    if old < low || old > high {
        return old;
    }
    if next < old {
        return max(next, low);
    }
    return min(next, high);
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
        // The steps are those of `Limits::pair_room`.
        let held = min(old, other) >= low && max(old, other) <= high;
        if over > 0.0 && held {
            let down = max(old, other) - low;
            let up = high - min(old, other);
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
        // The lowest level is 0. A texel does not go below the floor, and a
        // texel outside the limits stays.
        let drop = floor(drop_at(at) + 0.5);
        if drop > 0.0 {
            value = limited(value, value - drop);
        }
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
        if group.mode == PUSH {
            value = limited(value, push_level(stamp, at, value));
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
