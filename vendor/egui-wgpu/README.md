# egui-wgpu, patched for Vale

This is a copy of `egui-wgpu` 0.36.2 from crates.io, with a small change. The root `Cargo.toml` uses it through `[patch.crates-io]`. The license is the license of egui: `MIT OR Apache-2.0`.

The change splits the render pass of a frame at markers. At each marker, a hook can read the image that the frame drew up to that point. Vale uses this to blur the canvas behind a card.

## The change

To upgrade egui, apply these changes to the new version of `egui-wgpu`:

1. Add the file `src/backdrop.rs`, and export its items from `src/lib.rs`. These are the items:
   - `BackdropLevel(pub u32)` is a marker. It is the callback of an `epaint::PaintCallback`. Its methods `paint_callback` and `shape` make the callback and the shape for a rect.
   - `BackdropHook` is a trait with one method, `capture`. The method gets the device, the queue, the encoder, a view of the frame with the usage `TEXTURE_BINDING`, and the size of the frame.
   - `BackdropHooks(pub Box<dyn BackdropHook>)` holds the hook in `Renderer::callback_resources`.
   - `BackdropTarget` holds the color view, the resolve view, the load operation of the first pass, and the depth view.
   - `Renderer::render_with_backdrops` draws the paint jobs in one or more render passes. At a marker with a level above the level of the last capture, it ends the pass, calls the hook, and starts a new pass that loads the color and the depth. It ignores a marker at or below that level. If no hook is registered, it draws one pass.
2. In `src/renderer.rs`, move the loop of `Renderer::render` into `render_jobs`, with a `BufferCursor` that keeps the position in the index and vertex slices. `render` and `render_with_backdrops` both use `render_jobs`.
3. In `Renderer::update_buffers`, do not warn about an unknown paint callback if the callback is a `BackdropLevel`.
4. In `src/capture.rs`, split `CaptureState::copy_textures` into `copy_to_buffer` and `blit_to_surface`. `copy_textures` calls both.
5. In `Painter::paint_and_update_textures` in `src/winit.rs`, draw with `render_with_backdrops`. If a hook is registered and the frame has a marker, or if a screenshot is requested, the frame is drawn into the texture of `CaptureState`. That texture has the size and the format of the surface, and the usage `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC`. Then `blit_to_surface` draws it onto the surface. With MSAA, each pass resolves into that texture.
