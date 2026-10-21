// The screen-space lines of the color stops, in passes before the layer
// pass. The first pass writes the shown height of each pixel. The second
// pass cuts each height to the range of its neighbors and finds the places
// where the height crosses the value of a stop between two pixel centers.
// The third pass gives each pixel the nearest of those places.
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

// The height of a pixel cut to the range of its 8 neighbors, so that a
// feature smaller than one pixel goes away. -1 off the ground.
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
