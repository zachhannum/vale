// Draws the globe: one pass that reads the cube map and shows stepped tints.
// The face layout matches `cube.rs`.

struct Uniforms {
    // Rows of the world-to-view rotation.
    rot0: vec4<f32>,
    rot1: vec4<f32>,
    rot2: vec4<f32>,
    // Center x and y and radius in pixels, then pixels per point.
    globe: vec4<f32>,
    // Brush direction in world space, then the brush radius in radians.
    brush: vec4<f32>,
    // Hardness, cursor shown, stepped preview, graticule shown.
    flags: vec4<f32>,
    // Face size in texels, band count, graticule step in radians, unused.
    params: vec4<f32>,
    // The lower limit of each band, from 0 to 1, in x.
    limits: array<vec4<f32>, 16>,
    colors: array<vec4<f32>, 16>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var heights: texture_2d_array<u32>;

const PI: f32 = 3.14159265;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn texel(face: i32, x: i32, y: i32) -> f32 {
    let last = i32(u.params.x) - 1;
    let p = vec2<i32>(clamp(x, 0, last), clamp(y, 0, last));
    return f32(textureLoad(heights, p, face, 0).r) / 65535.0;
}

// The height at a world direction, from 0 to 1, with bilinear filtering.
fn height(d: vec3<f32>) -> f32 {
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
    let f = (s * 0.5 + 0.5) * u.params.x - 0.5;
    let i = vec2<i32>(floor(f));
    let t = f - floor(f);
    let h00 = texel(face, i.x, i.y);
    let h10 = texel(face, i.x + 1, i.y);
    let h01 = texel(face, i.x, i.y + 1);
    let h11 = texel(face, i.x + 1, i.y + 1);
    return mix(mix(h00, h10, t.x), mix(h01, h11, t.x), t.y);
}

fn tint(h: f32) -> vec3<f32> {
    var color = u.colors[0].rgb;
    let count = i32(u.params.y);
    for (var i = 1; i < count; i++) {
        if h >= u.limits[i].x {
            color = u.colors[i].rgb;
        }
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

    let h = height(d);
    var color = vec3<f32>(h);
    if u.flags.z > 0.5 {
        color = tint(h);
    }

    // The angle that one pixel covers at this place on the globe.
    let px = 1.0 / (radius * max(v.z, 0.05));

    if u.flags.w > 0.5 {
        let grat = u.params.z;
        let lat = asin(clamp(d.z, -1.0, 1.0));
        let lon = atan2(d.y, d.x);
        let dlat = abs(fract(lat / grat + 0.5) - 0.5) * grat;
        let coslat = max(cos(lat), 1e-4);
        let dlon = abs(fract(lon / grat + 0.5) - 0.5) * grat * coslat;
        // Meridians stop near the poles, where they crowd together.
        let keep = step(abs(lat), PI * 0.5 - grat * 0.5);
        let line = max(1.0 - smoothstep(0.0, px * 1.5, dlat),
                       keep * (1.0 - smoothstep(0.0, px * 1.5, dlon)));
        color = mix(color, vec3<f32>(0.08, 0.12, 0.18), line * 0.22);
    }

    // A little shade toward the limb, so the disk reads as a ball.
    color *= mix(0.80, 1.0, pow(v.z, 0.6));

    if u.flags.y > 0.5 {
        let ang = acos(clamp(dot(d, u.brush.xyz), -1.0, 1.0));
        let edge = abs(ang - u.brush.w);
        let dark = 1.0 - smoothstep(px * 1.5, px * 3.0, edge);
        let light = 1.0 - smoothstep(px * 0.5, px * 1.5, edge);
        color = mix(color, vec3<f32>(0.0), dark * 0.45);
        color = mix(color, vec3<f32>(1.0), light * 0.95);
        let core = abs(ang - u.brush.w * u.flags.x);
        let inner = 1.0 - smoothstep(px * 0.5, px * 1.5, core);
        color = mix(color, vec3<f32>(1.0), inner * 0.35);
    }

    return vec4<f32>(color * cover, cover);
}
