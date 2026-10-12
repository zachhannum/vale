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
    // 1 if the carve mode or the ridge mode has a window, and 0 if not. In
    // an erode pass, 1 if the cells of the step are the cells of a window.
    window_on: u32,
    // The first texel, the face, and the cell size of the window.
    window_origin: vec2<i32>,
    window_face: u32,
    window_cell: u32,
    // The texel of the face at the first texel of the render target.
    origin: vec2<i32>,
    // The number of cells of an erode pass along one side of a face.
    cells: u32,
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

// The distance from a texel to the river of its pair in the window, in
// channel texels of the window, and the size of that river from 0 to 255. The third number is 1 if the window covers the texel, and 0
// if not. The steps are those of `Window::channel_of_texel`,
// `Window::channel_of_dir`, `ChannelWindow::valley`, and
// `ChannelWindow::ridge`.
fn window_river_at(at: vec2<i32>) -> vec3<f32> {
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

// The change of the level of a texel from one weather stamp. Each pair of
// texels that are side by side moves a whole number of levels from the higher
// texel to the lower one. The steps are those of `Heightmap::weather_face`.
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
        let drop = old - f32(tile_level(i32(group.face), p));
        let over = abs(drop) - stamp.talus;
        if over > 0.0 {
            change -= sign(drop) * floor(weight * over * 0.125);
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
        // The lowest level is 0.
        let drop = floor(drop_at(at) + 0.5);
        value = max(value - drop, 0.0);
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
    for (var i = 0u; i < group.count; i++) {
        let stamp = group.stamps[i];
        if group.mode == WEATHER {
            // The change is a whole number, and the new level is between
            // the old levels of the pair texels.
            value += weather(stamp, at, value);
            continue;
        }
        if group.mode == WARP {
            value = warp_level(stamp, at, value);
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
    }
    // A copy moves each texel of the rectangle to the tiles, so a texel with
    // no change keeps its level here.
    return vec4<u32>(u32(value), 0u, 0u, 0u);
}
