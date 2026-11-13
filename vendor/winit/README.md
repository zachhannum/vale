# winit, patched for Vale

This is a copy of `winit` 0.30.13 from crates.io, with a small change. The root `Cargo.toml` uses it through `[patch.crates-io]`. The license is the license of winit: `Apache-2.0`.

The change gives the iOS backend a scene delegate, as Apple recommends for iPhone Duo. A window takes its screen, its size, and its scale from its window scene.

## The change

To upgrade winit, apply these changes to the new version:

1. In `Cargo.toml`, add the features `NSDictionary` and `NSURL` to `objc2-foundation`, and the features `UIOpenURLContext`, `UIScene`, `UISceneDefinitions`, `UISceneOptions`, `UISceneSession`, and `UIWindowScene` to `objc2-ui-kit`.
2. Add the file `src/platform_impl/ios/scene.rs`, and declare it in `src/platform_impl/ios/mod.rs`. It has these items:
   - `WinitSceneDelegate` is a `UIResponder` that conforms to `UIWindowSceneDelegate`. An app names it in the `UISceneDelegateClassName` key of its scene manifest. When a scene connects, the delegate puts the waiting windows on the scene. It gives the URLs of the connection options and of `scene:openURLContexts:` to `application:openURL:options:` of the app delegate.
   - `current` returns the window scene of the app: the active scene, or else the first connected scene.
   - `wait_for_scene` keeps a window until a scene connects.
   - `register` registers the class of the delegate.
3. In `EventLoop::run` in `event_loop.rs`, call `scene::register` before `UIApplicationMain`. `ActiveEventLoop::primary_monitor` returns the screen of the scene.
4. In `window.rs`:
   - `Window::new` takes the bounds and the scale of the window from the scene: `coordinateSpace().bounds()` and `traitCollection().displayScale()`. If no scene is connected, it uses the main screen, and it gives the window to `wait_for_scene`. A fullscreen window on a given monitor uses the bounds and the scale of that screen.
   - `WinitUIWindow::new` takes the scene, and it makes the window with `initWithWindowScene:` and then sets the frame.
   - `current_monitor` and `primary_monitor` return the screen of the window scene.
   - `set_scale_factor` calls `WinitView::stop_scene_scale`.
5. In `view.rs`:
   - `WinitViewState` has the field `scene_scale: Cell<bool>`. It is true if the window attributes give no scale factor.
   - `WinitView::new` takes the scale of the scene, and it sets the content scale factor to the scale of the attributes or else to the scale of the scene.
   - In `layoutSubviews`, if `scene_scale` is true and the display scale of the window scene is not the content scale factor, the view calls `setContentScaleFactor` with the display scale. That call sends the events of the new scale and size. Otherwise, the size of the `Resized` event uses the content scale factor, not the scale of the screen.
