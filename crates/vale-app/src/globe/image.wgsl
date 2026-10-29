// Draws an image layer: an equirectangular picture of a rectangle in
// longitude and latitude, with its colors multiplied by its alpha. The pass
// reads `u.params`: the width of the first mip in pixels, then the number of
// mips. The west, the east, the south, and the north edge of the rectangle
// are in `u.detail`, in radians. The opacity is in `u.layer.x`, and the blend
// mode in `u.turn.z`. This file comes after `globe.wgsl`.

@group(1) @binding(10) var image: texture_2d<f32>;
@group(1) @binding(11) var image_sampler: sampler;

// The color of the image at a world direction, where one pixel covers the
// angle `px`. The level of the mip comes from that angle and not from a
// derivative, because the longitude jumps at the 180 degree meridian.
// Outside the rectangle, the color is transparent.
fn image_color(d: vec3<f32>, px: f32) -> vec4<f32> {
    let lon = atan2(d.y, d.x);
    let lat = atan2(d.z, length(d.xy));
    let e = u.detail;
    let span = e.y - e.x;
    // An image around the whole globe wraps in longitude.
    let around = span >= 2.0 * PI - 1e-5;
    var x = (lon - e.x) / span;
    if !around {
        let east = lon - e.x;
        x = (east - 2.0 * PI * floor(east / (2.0 * PI))) / span;
    }
    let y = (e.w - lat) / (e.w - e.z);
    if (!around && x > 1.0) || y < 0.0 || y > 1.0 {
        return vec4<f32>(0.0);
    }
    let texels = px * u.params.x / span;
    let lod = clamp(log2(max(texels, 1e-6)), 0.0, max(u.params.y - 1.0, 0.0));
    if !around {
        // The sampler repeats, so the edge texels must not mix with the
        // other edge.
        let half = 0.5 * exp2(ceil(lod)) / u.params.x;
        x = clamp(x, half, 1.0 - half);
    }
    return textureSampleLevel(image, image_sampler, vec2<f32>(x, y), lod);
}

// The straight color of a premultiplied color.
fn straight(p: vec4<f32>) -> vec3<f32> {
    return select(vec3<f32>(0.0), p.rgb / p.a, p.a > 0.0);
}

@fragment
fn fs_image(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let at = disk(frag);
    if at.cover <= 0.0 {
        discard;
    }
    let p = image_color(at.d, at.px);
    return vec4<f32>(p.rgb * at.shade, p.a) * u.layer.x;
}

@fragment
fn fs_image_blend(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let at = disk(frag);
    if at.cover <= 0.0 {
        return textureLoad(backdrop, vec2<i32>(floor(frag.xy)), 0);
    }
    let p = image_color(at.d, at.px);
    return blended(frag.xy, straight(p) * at.shade, p.a * u.layer.x);
}

@fragment
fn fs_flat_image(in: FlatVertex) -> @location(0) vec4<f32> {
    let d = normalize(in.dir);
    let px = max(length(dpdx(d)), length(dpdy(d)));
    return image_color(d, px) * u.layer.x;
}

@fragment
fn fs_flat_image_blend(in: FlatVertex) -> @location(0) vec4<f32> {
    let d = normalize(in.dir);
    let px = max(length(dpdx(d)), length(dpdy(d)));
    let p = image_color(d, px);
    return blended(in.position.xy, straight(p), p.a * u.layer.x);
}
