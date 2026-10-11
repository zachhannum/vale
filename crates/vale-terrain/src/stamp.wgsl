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
    // 1 if the carve mode has a window, and 0 if not. In an erode pass, 1 if
    // the cells of the step are the cells of a window.
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

// The distance from a texel to the river with the deepest valley there, in
// channel texels, and the size of that river. The steps are those of
// `ChannelMap::valley`.
fn river_at(at: vec2<i32>) -> vec2<f32> {
    let size = f32(group.channel_size);
    let c = (vec2<f32>(at) + 0.5) * size / f32(group.size) - 0.5;
    let whole = floor(c);
    let t = c - whole;
    let last = vec2<i32>(i32(group.channel_size) - 1);
    let p0 = clamp(vec2<i32>(whole), vec2<i32>(0), last);
    let p1 = clamp(vec2<i32>(whole) + 1, vec2<i32>(0), last);
    let face = i32(group.face);
    let a = vec2<f32>(textureLoad(channels, p0, face, 0).ba);
    let b = vec2<f32>(textureLoad(channels, vec2<i32>(p1.x, p0.y), face, 0).ba);
    let c0 = vec2<f32>(textureLoad(channels, vec2<i32>(p0.x, p1.y), face, 0).ba);
    let d = vec2<f32>(textureLoad(channels, p1, face, 0).ba);
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
    return vec3<f32>((texel - vec2<f32>(group.window_origin)) / cell, 1.0);
}

// The distance from a texel to the river of the window with the deepest
// valley there, in channel texels of the window, and the size of that river
// from 0 to 255. The third number is 1 if the window covers the texel, and 0
// if not. The steps are those of `Window::channel_of_texel`,
// `Window::channel_of_dir`, and `ChannelWindow::valley`.
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
    let a = vec2<f32>(textureLoad(window_channels, p0, 0).ba);
    let b = vec2<f32>(textureLoad(window_channels, vec2<i32>(p1.x, p0.y), 0).ba);
    let c0 = vec2<f32>(textureLoad(window_channels, vec2<i32>(p0.x, p1.y), 0).ba);
    let d = vec2<f32>(textureLoad(window_channels, p1, 0).ba);
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
    let whole = floor(c);
    let t = c - whole;
    let p = vec2<i32>(whole);
    let d00 = cell_drop(p);
    let d10 = cell_drop(p + vec2<i32>(1, 0));
    let d01 = cell_drop(p + vec2<i32>(0, 1));
    let d11 = cell_drop(p + 1);
    if d00 + d11 >= d10 + d01 {
        if t.x >= t.y {
            return d00 + (d10 - d00) * t.x + (d11 - d10) * t.y;
        }
        return d00 + (d01 - d00) * t.y + (d11 - d01) * t.x;
    }
    if t.x + t.y <= 1.0 {
        return d00 + (d10 - d00) * t.x + (d01 - d00) * t.y;
    }
    return d11 + (d01 - d11) * (1.0 - t.x) + (d10 - d11) * (1.0 - t.y);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<u32> {
    let at = vec2<i32>(floor(position.xy)) + group.origin;
    let n = f32(group.size);
    let theta = ((vec2<f32>(at) + 0.5) / n * 2.0 - 1.0) * PI_4;
    let cos_theta = cos_series(theta);
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
    }
    // A copy moves each texel of the rectangle to the tiles, so a texel with
    // no change keeps its level here.
    return vec4<u32>(u32(value), 0u, 0u, 0u);
}
