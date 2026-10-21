// The screen-space lines of the color stops, in passes before the layer
// pass. The first pass writes the shown height of each pixel. The second
// pass cuts away the peaks and the pits smaller than one pixel, and finds
// the places where the height crosses the value of a stop between two pixel
// centers. The third pass marks the blocks of pixels with a place, and the
// fourth gives each pixel the nearest place. The layer pass then draws the
// band edges and the lines from the cut heights and the nearest places.
//
// This file comes after `globe.wgsl` in the same module. The payload of a
// place matches `edges.rs`.

// The shown height of each pixel, or -1 off the ground.
@group(3) @binding(0) var edge_raw: texture_2d<f32>;
// The place that each pixel keeps from its own 4 pairs.
@group(3) @binding(1) var edge_seeds: texture_2d<u32>;
// The height of each pixel cut to the range of its 8 neighbors.
@group(3) @binding(2) var edge_clean: texture_2d<f32>;
// The nearest place of each pixel.
@group(3) @binding(3) var edge_nearest: texture_2d<u32>;
// 1 for each block of `EDGE_BLOCK` pixels on a side with a place, 0 for one
// with none.
@group(3) @binding(4) var edge_blocks: texture_2d<u32>;

const EDGE_EMPTY: u32 = 0xffffffffu;
const EDGE_T_STEPS: f32 = 64.0;
const EDGE_MAX_HALF: f32 = 29.0;
const EDGE_BLOCK: i32 = 8;

fn edge_pack(base: vec2<i32>, axis: u32, t: u32, stop: u32) -> u32 {
    let b = bitcast<vec2<u32>>(base) & vec2<u32>(63u);
    return b.x | (b.y << 6u) | (axis << 12u) | (t << 13u) | (stop << 20u);
}

// The offset of the first pixel of the pair of a place.
fn edge_base(p: u32) -> vec2<i32> {
    let b = vec2<u32>(p & 63u, (p >> 6u) & 63u);
    return bitcast<vec2<i32>>(b << vec2<u32>(26u)) >> vec2<u32>(26u);
}

fn edge_axis(p: u32) -> u32 {
    return (p >> 12u) & 1u;
}

fn edge_t(p: u32) -> u32 {
    return (p >> 13u) & 127u;
}

fn edge_stop(p: u32) -> u32 {
    return (p >> 20u) & 63u;
}

// The step from the first pixel of a pair to the second.
fn edge_step(axis: u32) -> vec2<f32> {
    return select(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), axis == 1u);
}

// A place from the center of the pixel that keeps it, in pixels.
fn edge_place(p: u32) -> vec2<f32> {
    return vec2<f32>(edge_base(p)) + edge_step(edge_axis(p)) * f32(edge_t(p)) / EDGE_T_STEPS;
}

// Half of the weight of the line of a stop in pixels.
fn edge_half(stop: u32) -> f32 {
    return min(u.lines[stop].color.a, EDGE_MAX_HALF);
}

@fragment
fn fs_heights(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let at = disk(frag);
    if at.cover <= 0.0 {
        discard;
    }
    return vec4<f32>(shown(at.d).x, 0.0, 0.0, 0.0);
}

@fragment
fn fs_flat_heights(in: FlatVertex) -> @location(0) vec4<f32> {
    return vec4<f32>(shown(normalize(in.dir)).x, 0.0, 0.0, 0.0);
}

fn raw_at(p: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(edge_raw));
    if any(p < vec2<i32>(0)) || any(p >= size) {
        return -1.0;
    }
    return textureLoad(edge_raw, p, 0).x;
}

// The height of a pixel cut to the range of its 8 neighbors, so that a
// peak or a pit smaller than one pixel goes away. -1 off the ground.
fn clean_at(p: vec2<i32>) -> f32 {
    let h = raw_at(p);
    if h < 0.0 {
        return -1.0;
    }
    var low = 2.0;
    var high = -1.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if dx == 0 && dy == 0 {
                continue;
            }
            let n = raw_at(p + vec2<i32>(dx, dy));
            if n >= 0.0 {
                low = min(low, n);
                high = max(high, n);
            }
        }
    }
    if high < 0.0 {
        return h;
    }
    return clamp(h, low, high);
}

struct SeedOut {
    @location(0) seed: vec4<u32>,
    @location(1) clean: vec4<f32>,
}

// Each pixel keeps the place of its 4 pairs that a line reaches farthest
// past. A height at the value of a stop is on the upper side, so flat
// ground at the value has no place.
@fragment
fn fs_edge_seeds(@builtin(position) frag: vec4<f32>) -> SeedOut {
    let p = vec2<i32>(floor(frag.xy));
    let h = clean_at(p);
    var best = EDGE_EMPTY;
    var best_m = 1e9;
    if h >= 0.0 {
        let count = u32(u.params.w) + 1u;
        for (var k = 0u; k < 4u; k++) {
            let axis = k / 2u;
            let dir = select(1, -1, k % 2u == 1u);
            let off = vec2<i32>(edge_step(axis)) * dir;
            let q = clean_at(p + off);
            if q < 0.0 {
                continue;
            }
            let base = min(off, vec2<i32>(0));
            let first = select(h, q, dir < 0);
            let second = select(q, h, dir < 0);
            for (var i = 0u; i < count; i++) {
                let half = edge_half(i);
                if half <= 0.0 {
                    continue;
                }
                let at = u.lines[i].at.x;
                if (first >= at) == (second >= at) {
                    continue;
                }
                let t = clamp((at - first) / (second - first), 0.0, 1.0);
                let steps = u32(round(t * EDGE_T_STEPS));
                let place = vec2<f32>(base) + edge_step(axis) * f32(steps) / EDGE_T_STEPS;
                let m = length(place) - half;
                // On a tie, the higher stop wins.
                if m <= best_m {
                    best_m = m;
                    best = edge_pack(base, axis, steps, i);
                }
            }
        }
    }
    return SeedOut(vec4<u32>(best, 0u, 0u, 0u), vec4<f32>(h, 0.0, 0.0, 0.0));
}

// 1 for a block of pixels with a place, and 0 for one with none.
@fragment
fn fs_edge_blocks(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<u32> {
    let first = vec2<i32>(floor(frag.xy)) * EDGE_BLOCK;
    let size = vec2<i32>(textureDimensions(edge_seeds));
    var found = 0u;
    for (var y = first.y; y < min(first.y + EDGE_BLOCK, size.y); y++) {
        for (var x = first.x; x < min(first.x + EDGE_BLOCK, size.x); x++) {
            if textureLoad(edge_seeds, vec2<i32>(x, y), 0).x != EDGE_EMPTY {
                found = 1u;
            }
        }
    }
    return vec4<u32>(found, 0u, 0u, 0u);
}

// Each pixel takes the place that a line reaches farthest past it, from the
// places that the pixels within the radius keep. The search is exact.
@fragment
fn fs_edge_nearest(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<u32> {
    let p = vec2<i32>(floor(frag.xy));
    let size = vec2<i32>(textureDimensions(edge_seeds));
    let r = i32(u.edges.y);
    let lo = max(p - vec2<i32>(r), vec2<i32>(0));
    let hi = min(p + vec2<i32>(r), size - vec2<i32>(1));
    // Most pixels are far from each line, and their blocks have no place.
    var near = false;
    for (var by = lo.y / EDGE_BLOCK; by <= hi.y / EDGE_BLOCK; by++) {
        for (var bx = lo.x / EDGE_BLOCK; bx <= hi.x / EDGE_BLOCK; bx++) {
            near = near || textureLoad(edge_blocks, vec2<i32>(bx, by), 0).x != 0u;
        }
    }
    if !near {
        return vec4<u32>(EDGE_EMPTY, 0u, 0u, 0u);
    }
    var best = EDGE_EMPTY;
    var best_m = 1e9;
    for (var y = lo.y; y <= hi.y; y++) {
        for (var x = lo.x; x <= hi.x; x++) {
            let seed = textureLoad(edge_seeds, vec2<i32>(x, y), 0).x;
            if seed == EDGE_EMPTY {
                continue;
            }
            let at = vec2<i32>(x, y) - p;
            let stop = edge_stop(seed);
            let m = length(vec2<f32>(at) + edge_place(seed)) - edge_half(stop);
            if m < best_m || (m == best_m && stop > edge_stop(best)) {
                best_m = m;
                best = edge_pack(at + edge_base(seed), edge_axis(seed), edge_t(seed), stop);
            }
        }
    }
    return vec4<u32>(best, 0u, 0u, 0u);
}

// The cut height of a pixel, or -1 off the ground.
fn clean_px(p: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(edge_clean));
    if any(p < vec2<i32>(0)) || any(p >= size) {
        return -1.0;
    }
    return textureLoad(edge_clean, p, 0).x;
}

// The distance from a pixel center to the band edge at `at`, toward a
// neighbor with the height `q`, in pixels. Large if the edge is not there.
fn edge_toward(c: f32, q: f32, at: f32, side: bool) -> f32 {
    if q < 0.0 || (q >= at) == side {
        return 1e9;
    }
    return clamp((at - c) / (q - c), 0.0, 1.0);
}

// The tint of a pixel from the cut heights. Each band edge is the line
// where the height between the pixel centers crosses the limit, and it is
// one pixel soft. `h` and `width` give the tint where there is no cut
// height.
fn tint_screen(p: vec2<i32>, h: f32, width: f32, mode: i32) -> vec3<f32> {
    let c = clean_px(p);
    if c < 0.0 {
        return tint(h, width, mode);
    }
    let left = clean_px(p - vec2<i32>(1, 0));
    let right = clean_px(p + vec2<i32>(1, 0));
    let up = clean_px(p - vec2<i32>(0, 1));
    let down = clean_px(p + vec2<i32>(0, 1));
    var color = band_color(0, c, mode);
    let count = i32(u.params.w);
    for (var i = 1; i < count; i++) {
        let at = u.bands[i].bottom.a;
        // A height at the limit is in the upper band.
        let side = c >= at;
        let tx = min(edge_toward(c, left, at, side), edge_toward(c, right, at, side));
        let ty = min(edge_toward(c, up, at, side), edge_toward(c, down, at, side));
        // The distance to the line through the two crossings.
        var d = min(tx, ty);
        if tx < 1e8 && ty < 1e8 {
            d = tx * ty / max(sqrt(tx * tx + ty * ty), 1e-9);
        }
        let over = select(clamp(0.5 - d, 0.0, 1.0), clamp(0.5 + d, 0.0, 1.0), side);
        color = mix(color, band_color(i, c, mode), over);
    }
    return color;
}

// The place where the height crosses `at` on the edge from `pa` to `pb`.
fn crossing(pa: vec2<f32>, ha: f32, pb: vec2<f32>, hb: f32, at: f32) -> vec2<f32> {
    return pa + (pb - pa) * clamp((at - ha) / (hb - ha), 0.0, 1.0);
}

// The distance from the pixel center to the segment from `a` to `b`.
fn segment_distance(a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(-dot(a, ab) / max(dot(ab, ab), 1e-12), 0.0, 1.0);
    return length(a + ab * t);
}

// A cell has the 4 pixel centers from `c` to `c + (1, 1)`. Its sides are 0
// and 1 along x, at the rows of `c` and `c + (0, 1)`, and 2 and 3 along y,
// at the columns of `c` and `c + (1, 0)`.
fn side_base(c: vec2<i32>, k: i32) -> vec2<i32> {
    return c + vec2<i32>(select(0, 1, k == 3), select(0, 1, k == 1));
}

fn side_step(k: i32) -> vec2<i32> {
    return select(vec2<i32>(0, 1), vec2<i32>(1, 0), k < 2);
}

// The cell on the other side of side `k`.
fn next_cell(c: vec2<i32>, k: i32) -> vec2<i32> {
    let away = array<vec2<i32>, 4>(
        vec2<i32>(0, -1),
        vec2<i32>(0, 1),
        vec2<i32>(-1, 0),
        vec2<i32>(1, 0),
    );
    return c + away[k];
}

// The side where the band edge leaves a cell that it enters at side `k`, or
// -1 off the ground. Where the edge crosses all 4 sides, the mean of the
// corners decides how it goes through.
fn exit_side(c: vec2<i32>, k: i32, at: f32) -> i32 {
    let h00 = clean_px(c);
    let h10 = clean_px(c + vec2<i32>(1, 0));
    let h01 = clean_px(c + vec2<i32>(0, 1));
    let h11 = clean_px(c + vec2<i32>(1, 1));
    if min(min(h00, h10), min(h01, h11)) < 0.0 {
        return -1;
    }
    let s00 = h00 >= at;
    let s10 = h10 >= at;
    let s01 = h01 >= at;
    let s11 = h11 >= at;
    let crossed = array<bool, 4>(s00 != s10, s01 != s11, s00 != s01, s10 != s11);
    if crossed[0] && crossed[1] && crossed[2] && crossed[3] {
        let middle = (h00 + h10 + h01 + h11) * 0.25 >= at;
        // Each corner on the other side of the middle has the edge across
        // its two sides.
        let first_alone = array<bool, 4>(s00 != middle, s01 != middle, s00 != middle, s10 != middle);
        let pair_first = array<i32, 4>(2, 2, 0, 0);
        let pair_second = array<i32, 4>(3, 3, 1, 1);
        if first_alone[k] {
            return pair_first[k];
        }
        return pair_second[k];
    }
    for (var e = 0; e < 4; e++) {
        if e != k && crossed[e] {
            return e;
        }
    }
    return -1;
}

// The place where the band edge crosses side `k` of a cell, from the
// center of pixel `p`.
fn side_crossing(c: vec2<i32>, k: i32, at: f32, p: vec2<i32>) -> vec2<f32> {
    let a = side_base(c, k);
    let b = a + side_step(k);
    let pa = vec2<f32>(a - p);
    return crossing(pa, clean_px(a), pa + vec2<f32>(side_step(k)), clean_px(b), at);
}

// The color of the line at a pixel, and the part of the pixel that it
// covers. The band edge goes through the nearest place, and on through the
// next 3 cells on each side, as a chain of segments. Where the chain turns
// by more than 70 degrees, the segments that cut the
// corner give way to the 2 outer segments, carried on to where they meet,
// so a sharp corner keeps its point.
fn line_cover(p: vec2<i32>) -> vec4<f32> {
    let seed = textureLoad(edge_nearest, p, 0).x;
    if seed == EDGE_EMPTY {
        return vec4<f32>(0.0);
    }
    let stop = edge_stop(seed);
    let half = edge_half(stop);
    let at = u.lines[stop].at.x;
    let axis = edge_axis(seed);
    let base = p + edge_base(seed);
    let step = vec2<i32>(edge_step(axis));
    let ha = clean_px(base);
    let hb = clean_px(base + step);
    var pts: array<vec2<f32>, 7>;
    pts[3] = edge_place(seed);
    var first = 3;
    var last = 3;
    if ha >= 0.0 && hb >= 0.0 && (ha >= at) != (hb >= at) {
        let pa = vec2<f32>(edge_base(seed));
        pts[3] = crossing(pa, ha, pa + vec2<f32>(step), hb, at);
        let across = vec2<i32>(step.y, step.x);
        // The cell on each side of the pair, and the side of each cell that
        // the pair is.
        let a_side = select(2, 0, axis == 0u);
        for (var way = 0; way < 2; way++) {
            var c = select(base - across, base, way == 0);
            var k = a_side + select(1, 0, way == 0);
            for (var n = 0; n < 3; n++) {
                let leave = exit_side(c, k, at);
                if leave < 0 {
                    break;
                }
                let y = side_crossing(c, leave, at, p);
                if way == 0 {
                    last += 1;
                    pts[last] = y;
                } else {
                    first -= 1;
                    pts[first] = y;
                }
                c = next_cell(c, leave);
                k = leave ^ 1;
            }
        }
    }
    var dist = length(pts[3]);
    for (var i = first; i < last; i++) {
        dist = min(dist, segment_distance(pts[i], pts[i + 1]));
    }
    for (var i = first; i < last - 2; i++) {
        for (var j = i + 2; j < last; j++) {
            let di = pts[i + 1] - pts[i];
            let dj = pts[j + 1] - pts[j];
            let li = length(di);
            let lj = length(dj);
            if li < 1e-3 || lj < 1e-3 || dot(di, dj) > 0.34 * li * lj {
                continue;
            }
            let turn = di.x * dj.y - di.y * dj.x;
            let gap = pts[j] - pts[i + 1];
            let t = (gap.x * dj.y - gap.y * dj.x) / turn;
            let v = (di.x * gap.y - di.y * gap.x) / turn;
            if t < 0.0 || v < 0.0 || t * li > 3.0 || v * lj > 3.0 {
                continue;
            }
            let corner = pts[i + 1] + di * t;
            dist = min(dist, segment_distance(pts[i + 1], corner));
            dist = min(dist, segment_distance(corner, pts[j]));
        }
    }
    let cover = clamp(max(half, 0.5) + 0.5 - dist, 0.0, 1.0) * min(2.0 * half, 1.0);
    return vec4<f32>(u.lines[stop].color.rgb, cover);
}
