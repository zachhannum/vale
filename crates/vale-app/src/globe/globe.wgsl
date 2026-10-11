// Draws the globe: one pass that reads the cube map and shows it as stepped
// tints, as a smooth ramp, or in greyscale. The face layout matches `cube.rs`
// of `vale-terrain`.
//
// The source of `tiles.wgsl` of `vale-terrain` comes before this file, and
// each read of a level goes through its `tile_level`.

struct Band {
    // The color at the lower limit, then the lower limit, from 0 to 1.
    bottom: vec4<f32>,
    // The color at the upper limit, then the upper limit.
    top: vec4<f32>,
    // The color of the whole step.
    solid: vec4<f32>,
}

struct Uniforms {
    // Rows of the world-to-view rotation.
    rot0: vec4<f32>,
    rot1: vec4<f32>,
    rot2: vec4<f32>,
    // Center x and y and radius in pixels, then pixels per point. In the
    // flat view, the radius is the size of one unit of the projection.
    globe: vec4<f32>,
    // Face size in texels, the graticule step in radians, the preview mode,
    // and the band count. A step of 0 hides the graticule.
    params: vec4<f32>,
    // Sea level, from 0 to 1. The size of a face of the channel map in
    // texels, or 0 to hide the rivers. The point of the flat map at the
    // center, in z and w.
    flat: vec4<f32>,
    // The width and the height of the render target in pixels.
    screen: vec4<f32>,
    // The window of small rivers: 1 if the globe has one, the face that
    // gives its texel grid, and the first texel of the window on that face.
    window: vec4<f32>,
    // The number of texels along one side of a cell of the window, the
    // number of channel texels along one side of the window, and the factor
    // of the width of a river line.
    rivers: vec4<f32>,
    // The bands, from the lowest to the highest.
    bands: array<Band, 32>,
}

const MODE_GREYSCALE: i32 = 0;
const MODE_SMOOTH: i32 = 2;

@group(0) @binding(0) var<uniform> u: Uniforms;
// The channel map of `vale-terrain`. The first two bytes of a texel are the
// distance to the nearest river, and the size of that river.
@group(0) @binding(1) var channels: texture_2d_array<u32>;
// The channel window of `vale-terrain`, with the same two bytes.
@group(0) @binding(2) var window_channels: texture_2d<u32>;

const PI: f32 = 3.14159265;
const RIVER_COLOR: vec3<f32> = vec3<f32>(0.16, 0.36, 0.62);
// The distance byte of the channel map for a distance of one channel texel.
const CHANNEL_SCALE: f32 = 32.0;
// The edge of a line is not farther from the river than this number of
// channel texels. The channel map holds no distance past 255 / 32 texels.
const CHANNEL_REACH: f32 = 7.5;
// The channel texels at each side of the window with no sure distance.
const WINDOW_MARGIN: f32 = 8.0;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn load(face: i32, x: i32, y: i32) -> f32 {
    return f32(tile_level(face, vec2<i32>(x, y))) / 65535.0;
}

// The flat face coordinate at the center of a texel. A texel past the face
// gives the face edge.
fn flat_at(i: i32) -> f32 {
    let s = (f32(i) + 0.5) / u.params.x * 2.0 - 1.0;
    return tan(clamp(s, -1.0, 1.0) * (PI / 4.0));
}

// The texel of the next face that touches a texel on a face edge. `x` or `y`
// is one texel past the face.
fn across(face: i32, x: i32, y: i32) -> f32 {
    let n = i32(u.params.x);
    let axis = face / 2;
    // The point on the edge, next to the two texels.
    var p: array<f32, 3>;
    p[axis] = 1.0 - 2.0 * f32(face % 2);
    p[(axis + 1) % 3] = flat_at(x);
    p[(axis + 2) % 3] = flat_at(y);
    var next = (axis + 1) % 3;
    if y < 0 || y >= n {
        next = (axis + 2) % 3;
    }
    var to = next * 2;
    if p[next] < 0.0 {
        to += 1;
    }
    let f = vec2<f32>(p[(next + 1) % 3], p[(next + 2) % 3]) / abs(p[next]);
    let s = atan(f) * (4.0 / PI);
    let i = vec2<i32>(floor((s * 0.5 + 0.5) * u.params.x));
    let c = clamp(i, vec2<i32>(0), vec2<i32>(n - 1));
    return load(to, c.x, c.y);
}

// The height of a texel, from 0 to 1. A texel one step past the face comes
// from the next face, so the filter has no seam.
fn texel(face: i32, x: i32, y: i32) -> f32 {
    let last = i32(u.params.x) - 1;
    let cx = clamp(x, 0, last);
    let cy = clamp(y, 0, last);
    if x == cx && y == cy {
        return load(face, x, y);
    }
    if x != cx && y != cy {
        // Three faces meet at a cube corner, so no texel is there. The mean
        // of the three corner texels is the same from each face.
        return (load(face, cx, cy) + across(face, x, cy) + across(face, cx, y)) / 3.0;
    }
    return across(face, x, y);
}

struct Place {
    face: i32,
    // The place on the face, from 0 to 1 along each axis.
    at: vec2<f32>,
}

// The place of a world direction on its cube face.
fn place(d: vec3<f32>) -> Place {
    let a = abs(d);
    var axis = 2;
    if a.x >= a.y && a.x >= a.z {
        axis = 0;
    } else if a.y >= a.z {
        axis = 1;
    }
    var face = axis * 2;
    if d[axis] < 0.0 {
        face += 1;
    }
    let flat = vec2<f32>(d[(axis + 1) % 3], d[(axis + 2) % 3]) / a[axis];
    let s = atan(flat) * (4.0 / PI);
    return Place(face, s * 0.5 + 0.5);
}

// The height at a world direction, from 0 to 1, with bilinear filtering. The
// y and z parts are the change of the height across one texel.
fn height(d: vec3<f32>) -> vec3<f32> {
    let p = place(d);
    let face = p.face;
    let f = p.at * u.params.x - 0.5;
    let i = vec2<i32>(floor(f));
    let t = f - floor(f);
    let h00 = texel(face, i.x, i.y);
    let h10 = texel(face, i.x + 1, i.y);
    let h01 = texel(face, i.x, i.y + 1);
    let h11 = texel(face, i.x + 1, i.y + 1);
    let slope = vec2<f32>(mix(h10 - h00, h11 - h01, t.y), mix(h01 - h00, h11 - h10, t.x));
    return vec3<f32>(mix(mix(h00, h10, t.x), mix(h01, h11, t.x), t.y), slope);
}

// The first two bytes of a texel of the channel map. A texel past the face gives
// the texel at the face edge.
fn channel(face: i32, x: i32, y: i32) -> vec2<f32> {
    let last = i32(u.flat.y) - 1;
    let c = clamp(vec2<i32>(x, y), vec2<i32>(0), vec2<i32>(last));
    return vec2<f32>(textureLoad(channels, c, face, 0).rg);
}

// The distance from a world direction to the nearest river of the window in
// channel texels of the window, and the size of that river from 0 to 255.
// The third number is 1 if the window covers the direction, and 0 if not.
// Each of the first two is a blend of the 4 texels around the place, as in
// `stamp.wgsl` of `vale-terrain`.
fn window_river(d: vec3<f32>) -> vec3<f32> {
    if u.window.x < 0.5 {
        return vec3<f32>(0.0);
    }
    let face = i32(u.window.y);
    let axis = face / 2;
    let depth = d[axis] * (1.0 - 2.0 * f32(face % 2));
    if depth <= 0.2 {
        return vec3<f32>(0.0);
    }
    // The texel grid of the face continues past its edges.
    let flat = vec2<f32>(d[(axis + 1) % 3], d[(axis + 2) % 3]) / depth;
    let texel = (atan(flat) * (4.0 / PI) + 1.0) * 0.5 * u.params.x;
    let c = (texel - u.window.zw) * 2.0 / u.rivers.x - 0.5;
    let last = u.rivers.y - 1.0 - WINDOW_MARGIN;
    if any(c < vec2<f32>(WINDOW_MARGIN)) || any(c > vec2<f32>(last)) {
        return vec3<f32>(0.0);
    }
    let i = vec2<i32>(floor(c));
    let t = c - floor(c);
    let c00 = vec2<f32>(textureLoad(window_channels, i, 0).rg);
    let c10 = vec2<f32>(textureLoad(window_channels, i + vec2<i32>(1, 0), 0).rg);
    let c01 = vec2<f32>(textureLoad(window_channels, i + vec2<i32>(0, 1), 0).rg);
    let c11 = vec2<f32>(textureLoad(window_channels, i + vec2<i32>(1, 1), 0).rg);
    let both = mix(mix(c00, c10, t.x), mix(c01, c11, t.x), t.y);
    return vec3<f32>(both.x / CHANNEL_SCALE, both.y, 1.0);
}

// The distance from a world direction to the nearest river of the channel
// map in channel texels, and the size of that river from 0 to 255. The
// distance is a blend of the 4 texels around the place, and the size is the
// largest of the 4, as in `stamp.wgsl` of `vale-terrain`.
fn map_river(d: vec3<f32>) -> vec2<f32> {
    let p = place(d);
    let f = p.at * u.flat.y - 0.5;
    let i = vec2<i32>(floor(f));
    let t = f - floor(f);
    let c00 = channel(p.face, i.x, i.y);
    let c10 = channel(p.face, i.x + 1, i.y);
    let c01 = channel(p.face, i.x, i.y + 1);
    let c11 = channel(p.face, i.x + 1, i.y + 1);
    let flow = max(max(c00.y, c10.y), max(c01.y, c11.y));
    let distance = mix(mix(c00.x, c10.x, t.x), mix(c01.x, c11.x, t.x), t.y) / CHANNEL_SCALE;
    return vec2<f32>(distance, flow);
}

// The part of a pixel that a river covers at a world direction, from 0 to 1.
// `pixel` is the angle that one pixel covers there. The river comes from the
// window where the window covers the direction, and from the channel map on
// the other ground.
fn river(d: vec3<f32>, pixel: f32) -> f32 {
    let in_window = window_river(d);
    var found = in_window.xy;
    // The angle of one channel texel.
    var angle = u.rivers.x * PI / 4.0 / u.params.x;
    if in_window.z < 0.5 {
        found = map_river(d);
        angle = PI / 2.0 / u.flat.y;
    }
    let flow = found.y / 255.0;
    if flow <= 0.0 {
        return 0.0;
    }
    // The size of one channel texel in pixels.
    let texel = angle / pixel;
    // Half of the width of the line in pixels. A large river has a wide
    // line. A line that is thin against the texels has steps.
    let half = max((0.6 + 1.4 * flow) * u.rivers.z, 0.7 * texel);
    let edge = min(half + 0.5, CHANNEL_REACH * texel);
    return clamp(edge - found.x * texel, 0.0, 1.0);
}

fn band_color(i: i32, h: f32, mode: i32) -> vec3<f32> {
    let band = u.bands[i];
    if mode != MODE_SMOOTH {
        return band.solid.rgb;
    }
    let t = (h - band.bottom.a) / max(band.top.a - band.bottom.a, 1e-6);
    return mix(band.bottom.rgb, band.top.rgb, clamp(t, 0.0, 1.0));
}

// The color of a height. `width` is the change of the height across one
// pixel. It makes a band limit one pixel soft.
fn tint(h: f32, width: f32, mode: i32) -> vec3<f32> {
    let w = max(width, 1e-6);
    var color = band_color(0, h, mode);
    let count = i32(u.params.w);
    for (var i = 1; i < count; i++) {
        // A height at the limit is in the upper band.
        let over = clamp((h - u.bands[i].bottom.a) / w + 1.0, 0.0, 1.0);
        color = mix(color, band_color(i, h, mode), over);
    }
    return color;
}

// The color of the ground in a world direction: the tint of the height, the
// rivers, and the graticule. `width` is the change of the height across one
// pixel. `wlat` and `wlon` are the latitude and the longitude that one pixel
// covers there. `pixel` is the angle that one pixel covers there.
fn ground(h: f32, width: f32, d: vec3<f32>, wlat: f32, wlon: f32, pixel: f32) -> vec3<f32> {
    let mode = i32(u.params.z);
    var color = vec3<f32>(h);
    if mode != MODE_GREYSCALE {
        color = tint(h, width, mode);
    }
    if u.flat.y > 0.5 {
        // A river stops at the coast.
        let land = clamp((h - u.flat.x) / max(width, 1e-6) + 1.0, 0.0, 1.0);
        color = mix(color, RIVER_COLOR, 0.9 * land * river(d, pixel));
    }
    let grat = u.params.y;
    if grat > 0.0 {
        let lat = asin(clamp(d.z, -1.0, 1.0));
        let lon = atan2(d.y, d.x);
        let dlat = abs(fract(lat / grat + 0.5) - 0.5) * grat;
        let dlon = abs(fract(lon / grat + 0.5) - 0.5) * grat;
        // Meridians stop near the poles, where they crowd together.
        let keep = step(abs(lat), PI * 0.5 - grat * 0.5);
        let line = max(1.0 - smoothstep(0.0, wlat * 1.5, dlat),
                       keep * (1.0 - smoothstep(0.0, wlon * 1.5, dlon)));
        color = mix(color, vec3<f32>(0.08, 0.12, 0.18), line * 0.22);
    }
    return color;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let radius = u.globe.z;
    let p = vec2<f32>(frag.x - u.globe.x, u.globe.y - frag.y) / radius;
    let r = length(p);
    // One pixel of soft edge at the limb.
    let cover = clamp((1.0 - r) * radius + 0.5, 0.0, 1.0);
    if cover <= 0.0 {
        discard;
    }
    let rr = min(r, 1.0);
    let v = vec3<f32>(p / max(r, 1e-6) * rr, sqrt(max(1.0 - rr * rr, 0.0)));
    let d = v.x * u.rot0.xyz + v.y * u.rot1.xyz + v.z * u.rot2.xyz;

    // The angle that one pixel covers at this place on the globe.
    let px = 1.0 / (radius * max(v.z, 0.05));
    let coslat = max(sqrt(max(1.0 - d.z * d.z, 0.0)), 1e-4);
    let h = height(d).x;
    var color = ground(h, fwidth(h), d, px, px / coslat, px);

    // A little shade toward the limb, so the disk reads as a ball.
    color *= mix(0.80, 1.0, pow(v.z, 0.6));

    return vec4<f32>(color * cover, cover);
}

struct FlatVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) dir: vec3<f32>,
}

// One corner of the mesh of the flat view: a point of the map, and the world
// direction of the place there.
@vertex
fn vs_flat(@location(0) point: vec2<f32>, @location(1) dir: vec3<f32>) -> FlatVertex {
    let offset = vec2<f32>(point.x - u.flat.z, u.flat.w - point.y);
    let pixel = u.globe.xy + offset * u.globe.z;
    let ndc = vec2<f32>(pixel.x / u.screen.x * 2.0 - 1.0, 1.0 - pixel.y / u.screen.y * 2.0);
    return FlatVertex(vec4<f32>(ndc, 0.0, 1.0), dir);
}

@fragment
fn fs_flat(in: FlatVertex) -> @location(0) vec4<f32> {
    let d = normalize(in.dir);
    let lat = asin(clamp(d.z, -1.0, 1.0));
    // The longitude jumps at the 180 degree meridian. Its sine and its
    // cosine do not, and they change by the same angle.
    let east = normalize(vec2<f32>(d.x, d.y) + vec2<f32>(1e-9, 0.0));
    let wlon = length(fwidth(east));
    // A derivative of the height is wrong at an edge of a triangle of the
    // mesh, so the width comes from the slope of the texels. A face has
    // 2 / PI of its texels in one radian.
    let h = height(d);
    let angle = max(length(dpdx(d)), length(dpdy(d)));
    let width = length(h.yz) * u.params.x * (2.0 / PI) * angle;
    return vec4<f32>(ground(h.x, width, d, fwidth(lat), wlon, angle), 1.0);
}
