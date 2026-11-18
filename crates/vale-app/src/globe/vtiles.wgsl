// Draws a vector layer from its tiles: premultiplied RGBA pictures of parts
// of the cube faces, in one atlas that `image` and `image_sampler` bind. The
// page table of the layer gives the slot of each tile in the atlas, plus 1,
// or 0 for a tile that is not there. A pixel reads the tile of the level with
// one texel or more in the pixel, or the nearest coarser tile that is there.
// The constants match `globe/vtiles.rs`. This file comes after `image.wgsl`.

@group(1) @binding(12) var vtile_pages: texture_2d<u32>;

const VTILE: f32 = 256.0;
const VTILE_PX: f32 = 258.0;
const VTILE_COLS: u32 = 20u;
const VTILE_ATLAS: vec2<f32> = vec2<f32>(5160.0, 4644.0);
const VTILE_MAX_LEVEL: i32 = 8;
const VTILE_PAGE_WIDTH: u32 = 1024u;

// The entry of the page table for a tile.
fn vtile_page(level: u32, face: u32, col: u32, row: u32) -> u32 {
    let side = 1u << level;
    let base = 6u * ((1u << (2u * level)) - 1u) / 3u;
    let i = base + (face * side + row) * side + col;
    let at = vec2<i32>(i32(i % VTILE_PAGE_WIDTH), i32(i / VTILE_PAGE_WIDTH));
    return textureLoad(vtile_pages, at, 0).r;
}

// The color of the layer at a world direction, where one pixel covers the
// angle `px`.
fn vtile_color(d: vec3<f32>, px: f32) -> vec4<f32> {
    let p = place(d);
    // A texel of level 0 covers a quarter turn over 256 texels.
    let texels = 2.0 * FIELD_QUARTER_PI / (VTILE * max(px, 1e-9));
    let want = clamp(i32(ceil(log2(max(texels, 1.0)))), 0, VTILE_MAX_LEVEL);
    for (var level = want; level >= 0; level -= 1) {
        let side = f32(1u << u32(level));
        let t = clamp(p.at * side, vec2<f32>(0.0), vec2<f32>(side - 1e-4));
        let cell = floor(t);
        let page = vtile_page(u32(level), u32(p.face), u32(cell.x), u32(cell.y));
        if page > 0u {
            let slot = page - 1u;
            let origin = vec2<f32>(f32(slot % VTILE_COLS), f32(slot / VTILE_COLS)) * VTILE_PX;
            let at = origin + 1.0 + (t - cell) * VTILE;
            return textureSampleLevel(image, image_sampler, at / VTILE_ATLAS, 0.0);
        }
    }
    return vec4<f32>(0.0);
}

@fragment
fn fs_vtiles(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let at = disk(frag);
    if at.cover <= 0.0 {
        discard;
    }
    return vtile_color(at.d, at.px);
}

@fragment
fn fs_flat_vtiles(in: FlatVertex) -> @location(0) vec4<f32> {
    let d = normalize(in.dir);
    // The flat map stretches the world along one axis near the poles, and
    // the tile has the texels for the finer one.
    let px = min(length(dpdx(d)), length(dpdy(d)));
    return vtile_color(d, px);
}
