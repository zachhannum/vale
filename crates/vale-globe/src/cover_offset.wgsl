// Grows or shrinks the cover of a raster mask by a distance in pixels. The
// pass along the rows finds the nearest pixel of the region in each row, and
// the pass along the columns gives the distance to the region. The cover
// has its value in the red channel and the part of the pixel on the ground
// in the green channel.

struct Offset {
    // The distance in pixels. A negative distance shrinks the cover.
    radius: f32,
    // The pixels that a pass reads on each side.
    reach: i32,
    pad: vec2<f32>,
    // The canvas in pixels: the left, the top, the right, and the bottom.
    clip: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var cover: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u: Offset;

// The distance of a row with no pixel of the region in reach.
const NONE: f32 = 1000.0;

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// The smallest part of a pixel that counts, above the noise of the cover.
const LEAST: f32 = 0.05;

// The part of pixel `p` in the region that grows: the cover, or for a
// shrink the ground outside the cover. A pixel off the canvas or not fully
// on the ground is in neither, so a shrink keeps the cover at the edge of
// the world and of the canvas.
fn part(p: vec2<i32>) -> f32 {
    let at = vec2<f32>(p) + 0.5;
    if any(at < u.clip.xy) || any(at > u.clip.zw) {
        return 0.0;
    }
    let c = textureLoad(cover, p, 0).rg;
    var a = c.r;
    if u.radius < 0.0 {
        a = select(0.0, 1.0 - c.r, c.g >= 1.0);
    }
    return select(0.0, a, a >= LEAST);
}

// The nearest pixel of the region in the row: its distance, and the
// distance from its center to the edge of the region in it, which is
// negative for a pixel that the region covers by more than half.
@fragment
fn fs_rows(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(frag.xy));
    let width = i32(textureDimensions(cover).x);
    var best = vec2<f32>(NONE, 0.0);
    for (var dx = -u.reach; dx <= u.reach; dx++) {
        let x = p.x + dx;
        if x < 0 || x >= width {
            continue;
        }
        let a = part(vec2<i32>(x, p.y));
        if a <= 0.0 {
            continue;
        }
        let d = vec2<f32>(f32(abs(dx)), 0.5 - a);
        if d.x + d.y < best.x + best.y {
            best = d;
        }
    }
    return vec4<f32>(best, 0.0, 1.0);
}

@fragment
fn fs_columns(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(frag.xy));
    let height = i32(textureDimensions(source).y);
    var d = NONE;
    for (var dy = -u.reach; dy <= u.reach; dy++) {
        let y = p.y + dy;
        if y < 0 || y >= height {
            continue;
        }
        let row = textureLoad(source, vec2<i32>(p.x, y), 0).rg;
        if row.x >= NONE {
            continue;
        }
        d = min(d, length(vec2<f32>(row.x, f32(dy))) + row.y);
    }
    var c = clamp(0.5 + abs(u.radius) - d, 0.0, 1.0);
    if u.radius < 0.0 {
        c = 1.0 - c;
    }
    let ground = textureLoad(cover, p, 0).g > 0.0;
    return vec4<f32>(select(0.0, c, ground), 0.0, 0.0, 1.0);
}
