// Darkens the ground outside the extent of a map. The table holds the
// projected point at radius 1 of each texel of an equirectangular grid, and
// 1 in z if the point is on the map. The frames are `Frames::gpu_data` of
// `extent.rs`. The opacity of the shade is in `u.layer.x`. This file comes
// after `globe.wgsl`.

@group(1) @binding(10) var extent_table: texture_2d<f32>;
@group(1) @binding(11) var extent_frames: texture_2d<f32>;

// A table texel. The column wraps, and the row stays on the table.
fn table_texel(x: i32, y: i32) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(extent_table));
    let at = vec2<i32>(((x % size.x) + size.x) % size.x, clamp(y, 0, size.y - 1));
    return textureLoad(extent_table, at, 0);
}

fn frame_texel(i: i32) -> vec4<f32> {
    let width = i32(textureDimensions(extent_frames).x);
    return textureLoad(extent_frames, vec2<i32>(i % width, i / width), 0);
}

// The projected point of a world direction, and 1 in z if it is on the map.
// Across the edge of the map, the four texels are far apart, and the
// nearest one gives the point.
fn projected(d: vec3<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(extent_table));
    let lon = atan2(d.y, d.x);
    let lat = atan2(d.z, length(d.xy));
    let t = vec2<f32>((lon / (2.0 * PI) + 0.5) * size.x, (0.5 - lat / PI) * size.y) - 0.5;
    let i = vec2<i32>(floor(t));
    let f = t - floor(t);
    let a = table_texel(i.x, i.y);
    let b = table_texel(i.x + 1, i.y);
    let c = table_texel(i.x, i.y + 1);
    let e = table_texel(i.x + 1, i.y + 1);
    let lo = min(min(a.xy, b.xy), min(c.xy, e.xy));
    let hi = max(max(a.xy, b.xy), max(c.xy, e.xy));
    let valid = a.z * b.z * c.z * e.z;
    if valid < 0.5 || length(hi - lo) > 0.2 {
        let near = table_texel(i.x + i32(round(f.x)), i.y + i32(round(f.y)));
        return near.xyz;
    }
    let top = mix(a.xy, b.xy, f.x);
    let bottom = mix(c.xy, e.xy, f.x);
    return vec3<f32>(mix(top, bottom, f.y), 1.0);
}

// True if a frame shows a projected point. The fill rule is even-odd.
fn in_frames(p: vec2<f32>) -> bool {
    let count = i32(frame_texel(0).x);
    for (var k = 0; k < count; k++) {
        let h0 = frame_texel(1 + 3 * k);
        let h1 = frame_texel(2 + 3 * k);
        let box = frame_texel(3 + 3 * k);
        let q = h0.xy + h0.zw * p.x + h1.xy * p.y;
        if q.x < box.x || q.y < box.y || q.x > box.z || q.y > box.w {
            continue;
        }
        var inside = false;
        let start = i32(h1.z);
        for (var i = start; i < start + i32(h1.w); i++) {
            let edge = frame_texel(i);
            let a = edge.xy;
            let b = edge.zw;
            if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
        if inside {
            return true;
        }
    }
    return false;
}

fn extent_shade(d: vec3<f32>) -> vec4<f32> {
    let p = projected(d);
    if p.z > 0.5 && in_frames(p.xy) {
        discard;
    }
    return vec4<f32>(0.0, 0.0, 0.0, u.layer.x);
}

@fragment
fn fs_extent(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let at = disk(frag);
    if at.cover <= 0.0 {
        discard;
    }
    return extent_shade(at.d);
}

@fragment
fn fs_flat_extent(in: FlatVertex) -> @location(0) vec4<f32> {
    return extent_shade(normalize(in.dir));
}
