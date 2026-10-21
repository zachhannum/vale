// The screen-space lines of the color stops, in passes before the layer
// pass. The first pass writes the shown height of each pixel. The second
// pass cuts away the features narrower than one pixel and finds the places
// where the height crosses the value of a stop between two pixel centers.
// The third pass gives each pixel the nearest of those places. The layer
// pass then draws the band edges and the lines from the cut heights and the
// nearest places.
//
// This file comes after `globe.wgsl` in the same module. The payload of a
// place matches `edges.rs`.

// The shown height of each pixel, or -1 off the ground.
@group(3) @binding(0) var edge_raw: texture_2d<f32>;
// The place that each pixel keeps from its own 4 pairs.
@group(3) @binding(1) var edge_seeds: texture_2d<u32>;
// The height of each pixel with the features narrower than one pixel cut
// away.
@group(3) @binding(2) var edge_clean: texture_2d<f32>;
// The nearest place of each pixel.
@group(3) @binding(3) var edge_nearest: texture_2d<u32>;

const EDGE_EMPTY: u32 = 0xffffffffu;
const EDGE_T_STEPS: f32 = 64.0;
const EDGE_MAX_HALF: f32 = 29.0;

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

// The height of a pixel cut so that a feature narrower than one pixel goes
// away: a peak or a ridge is cut down to the higher of two opposite
// neighbors, and a pit or a valley is filled up to the lower of them. Each
// of the 4 pairs of opposite neighbors cuts. -1 off the ground.
fn clean_at(p: vec2<i32>) -> f32 {
    let h = raw_at(p);
    if h < 0.0 {
        return -1.0;
    }
    var low = -1.0;
    var high = 2.0;
    let pairs = array<vec2<i32>, 4>(
        vec2<i32>(1, 0),
        vec2<i32>(0, 1),
        vec2<i32>(1, 1),
        vec2<i32>(1, -1),
    );
    for (var k = 0; k < 4; k++) {
        let a = raw_at(p + pairs[k]);
        let b = raw_at(p - pairs[k]);
        if a >= 0.0 && b >= 0.0 {
            low = max(low, min(a, b));
            high = min(high, max(a, b));
        }
    }
    // On a narrow ridge, the pair along the ridge is as high as the pixel
    // and the pair across it is lower, so the pixel is below one pair and
    // above another. The pixel takes the cut of the pair that it is
    // farther from: a ridge goes down, and a valley fills up.
    if low > high {
        return select(low, high, h > 0.5 * (low + high));
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

// Each pixel takes the place that a line reaches farthest past it, from the
// places that the pixels within the radius keep. The search is exact.
@fragment
fn fs_edge_nearest(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<u32> {
    let p = vec2<i32>(floor(frag.xy));
    let size = vec2<i32>(textureDimensions(edge_seeds));
    let r = i32(u.edges.y);
    let lo = max(p - vec2<i32>(r), vec2<i32>(0));
    let hi = min(p + vec2<i32>(r), size - vec2<i32>(1));
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

// The color of the line at a pixel, and the part of the pixel that it
// covers. The band edge goes through the nearest place, on a segment in
// one of the two cells of 4 pixel centers that share its pair. Where a cell
// has the edge on all 4 sides, the mean of its corners decides how the
// edge goes through it.
fn line_cover(p: vec2<i32>) -> vec4<f32> {
    let seed = textureLoad(edge_nearest, p, 0).x;
    if seed == EDGE_EMPTY {
        return vec4<f32>(0.0);
    }
    let stop = edge_stop(seed);
    let half = edge_half(stop);
    let at = u.lines[stop].at.x;
    let step = edge_step(edge_axis(seed));
    let pa = vec2<f32>(edge_base(seed));
    let pb = pa + step;
    let ha = clean_px(p + vec2<i32>(pa));
    let hb = clean_px(p + vec2<i32>(pb));
    let sa = ha >= at;
    let sb = hb >= at;
    var x = edge_place(seed);
    var valid = ha >= 0.0 && hb >= 0.0 && sa != sb;
    if valid {
        x = crossing(pa, ha, pb, hb, at);
    }
    var dist = length(x);
    let across = vec2<f32>(step.y, step.x);
    for (var k = 0; k < 2 && valid; k++) {
        let pc = pa + across * f32(2 * k - 1);
        let pd = pc + step;
        let hc = clean_px(p + vec2<i32>(pc));
        let hd = clean_px(p + vec2<i32>(pd));
        if hc < 0.0 || hd < 0.0 {
            continue;
        }
        let sc = hc >= at;
        let sd = hd >= at;
        var y = x;
        if sa != sc && sb != sd && sc != sd {
            let middle = (ha + hb + hc + hd) * 0.25 >= at;
            if sa != middle {
                y = crossing(pa, ha, pc, hc, at);
            } else {
                y = crossing(pb, hb, pd, hd, at);
            }
        } else if sa != sc {
            y = crossing(pa, ha, pc, hc, at);
        } else if sb != sd {
            y = crossing(pb, hb, pd, hd, at);
        } else {
            y = crossing(pc, hc, pd, hd, at);
        }
        dist = min(dist, segment_distance(x, y));
    }
    let cover = clamp(max(half, 0.5) + 0.5 - dist, 0.0, 1.0) * min(2.0 * half, 1.0);
    return vec4<f32>(u.lines[stop].color.rgb, cover);
}
