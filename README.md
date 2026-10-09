# Vale

Vale is a GIS for fictional worlds, for desktop and iPad. It draws maps of invented planets with the right projections and with good labels.

This repository holds an app prototype and the labeling library `vale-labeler`. The prototype is a thin slice of the whole design. It runs, but it is not finished.

## Requirements

- Rust 1.95 or later.
- `cmake` and a C++ compiler.
- A GPU that wgpu supports.

The first build makes PROJ from source. It takes about one minute.

## Run the app

```sh
cargo run --release -p vale-app
```

The window opens on the globe workspace. The globe shows the heightmap of the world in greyscale. You can do these things.

- Drag the globe to rotate it. On a touch screen, use one finger.
- Scroll the wheel to zoom. On a trackpad, scroll with two fingers to rotate, and pinch to zoom.
- Hold Shift and drag to twist the globe about the middle of the view. On a trackpad, use the rotate gesture.
- On a touch screen, pinch and twist with two fingers. The place under your fingers stays under your fingers.
- Click "Brush" in the tool bar to paint the heightmap. A pen or the left mouse button paints. A finger, the other mouse buttons, and Shift with a drag move the globe.
- The brush panel opens with the brush tool. It has the four modes (raise, lower, smooth, and flatten), the radius in kilometers of the world, the hardness, and the strength. A pen sets the flow from its pressure, and the mouse paints with a fixed flow.
- The brush keeps its size on screen when you zoom. Select "Lock size" to keep its size on the ground.
- The flatten mode moves the ground to the level under the start of the stroke. To set a fixed level, click "Pick from globe", and then press on the globe.
- Click "Debug" in the tool bar to open the brush debug panel. It has "Undo". It shows the stroke delay, which is the time from the frame that read the pen to the end of the GPU work. "Run stroke test" paints four fixed strokes and shows their numbers: the largest brush and a small fast brush, each in the raise mode and in the smooth mode.

In the window, each face of the heightmap has 8,192 pixels, and the GPU holds 940 MB for it. If the computer has no GPU, start the app with `--face-size 1024`.

Click "Map" in the tool bar to open the flat map with the Natural Earth 110m sample world. You can do these things there.

- Drag the map to pan. Scroll or pinch to zoom. On a touch screen, two fingers pan and zoom.
- Double-click the map to move the center of the projection there.
- Use the left panel to set the world radius, the projection, and its center.
- Tick a layer to show it. Click "Edit" on a layer to select it. The inspector then shows its style.
- Click a feature to see its attributes. The inspector lists the labels that did not fit.
- Open a GeoJSON file with the button, or drop a file on the window.
- Export the view to PNG or PDF with the buttons in the tool bar.

## Open your own data

```sh
cargo run --release -p vale-app -- path/to/file.geojson
```

The file must use longitude and latitude. Vale copies the data into memory.

## Headless use

These commands need no window. They write files under `target/app/`.

```sh
cargo run --release -p vale-app -- --screenshot target/app/ui.png --size 1440x900 --report
cargo run --release -p vale-app -- --screenshot target/app/map.png --map-only --projection orthographic --center 20,30
cargo run --release -p vale-app -- --export target/app/map.pdf
cargo run --release -p vale-app -- --export target/app/map.png --pixel-ratio 2
cargo run --release -p vale-app -- --probe 25,25
cargo run --release -p vale-app -- --report
cargo run --release -p vale-app -- --stroke-test --face-size 8192
```

`--screenshot` draws the full UI in the globe workspace. With `--workspace map` it draws the flat map and its panels. With `--map-only` it draws the map alone. `--export` picks PNG or PDF from the file name. Text in the PDF stays text. `--probe` prints the color of one map pixel. `--report` prints the layers and the label counts. `--stroke-test` paints the fixed strokes of the debug panel and prints the stroke delay.

| Argument | Meaning |
| --- | --- |
| `--workspace <NAME>` | `globe` or `map`. The default is `globe`. |
| `--projection <ID>` | `equal-earth`, `mercator`, `lambert-azimuthal`, `orthographic`, or `stereographic` |
| `--center <LON,LAT>` | Center of the projection |
| `--radius-km <KM>` | Radius of the world. The default is 6371. |
| `--look-at <LON,LAT>` | Put this place at the middle of the view, on the globe and on the map |
| `--zoom <F>` | Zoom factor on the fitted view, on the globe and on the map |
| `--face-size <N>` | Pixels on one edge of a heightmap face. The default is 8192 in the window and 1024 without a window. |
| `--size <WxH>` | Size in points. The default is 1280x800. |
| `--pixel-ratio <F>` | Pixels per point of the output |
| `--hide <LAYER>` | Hide the layer with this name |
| `--no-labels`, `--no-graticule` | Turn the labels or the graticule off |
| `--smoke-frames <N>` | Open the window, draw N frames, and exit |

## Run on an iPad

You need a Mac with Xcode. Do the one-time steps for the Mac and for the iPad in `crates/vale-globe-proto/README.md`, in the section "Run on an iPad". Then connect the iPad, unlock it, and run this script:

```sh
scripts/app-ipad.sh
```

The script builds the app, installs it on the iPad, and starts it. It prints four steps and then "Done". The first build takes a few minutes, because it makes PROJ for iOS. If the build fails, read the full log in `target/ios/vale-app/xcodebuild.log`.

The app opens with the sample world. Drag with one finger to pan. Pinch with two fingers to zoom. The iPad app has no buttons to open or export files.

To build for the iOS simulator on a Mac with Apple silicon, run these commands:

```sh
rustup target add aarch64-apple-ios-sim
cd crates/vale-app/ios
xcodegen generate
xcodebuild -project ValeApp.xcodeproj -scheme ValeApp -sdk iphonesimulator -arch arm64 CODE_SIGNING_ALLOWED=NO build
```

The iOS target is in `crates/vale-app/ios`. `project.yml` describes the Xcode project. `build-rust.sh` builds the Rust static library, and `toolchain.cmake` tells the PROJ build which iOS SDK to use. The static PROJ library holds its own database, so the app bundle needs no PROJ data files.

## Get a PR build on an iPad

CI uploads a build to TestFlight for each PR from a branch of this repository, and for each push to `main`. The TestFlight app on the iPad then offers the build. The upload needs a membership in the Apple Developer Program.

Do these steps one time:

1. In the Apple developer account, register the bundle ID `dev.vale.app`. If the ID is not free, change `PRODUCT_BUNDLE_IDENTIFIER` in `crates/vale-app/ios/project.yml`.
2. In App Store Connect, create the app record with that bundle ID.
3. In App Store Connect, under "Users and Access", create an API key with the Admin role. Xcode can sign the build only with an Admin key. Download the `.p8` file.
4. In the GitHub repository, add these Actions secrets:
   - `ASC_KEY_P8`: the full text of the `.p8` file.
   - `ASC_KEY_ID`: the key ID.
   - `ASC_ISSUER_ID`: the issuer ID, from the same page as the key.
   - `APPLE_TEAM_ID`: the team ID, from the membership page of the developer account.
5. In App Store Connect, under "TestFlight", create an internal group, turn on automatic distribution, and add yourself.
6. On the iPad, install the TestFlight app and sign in with the same Apple account.

After a push to a PR, a comment on the PR shows the build number and the state of the upload. Apple needs some minutes to process each upload. When the comment says that the build is ready, open TestFlight on the iPad and install it.

The version of a build is `0.1.<PR>`, and `main` is `0.1.0`. TestFlight thus shows one row for each PR. The build number is the number of the workflow run and its attempt, for example `57.1`. The "What to Test" text of a build holds the PR number, the PR title, and the iPad test section of the PR description.

A PR from a fork gets no secrets. CI then makes the archive with no signature and uploads nothing.

All builds have the same bundle ID, so a new build installs over the old one and keeps the app data. A TestFlight build stops after 90 days.

The job `testflight` in `.github/workflows/ios.yml` does the work, with `scripts/testflight/testflight.py`.

## What the prototype leaves out

- Project files. There is no save and no undo. When the window closes, the session ends.
- Linked sources, file watch, raster layers, SVG and Shapefile import.
- Rule-based styles. A layer has one fill, one stroke, and one circle symbol.
- Polygon labels, label fallbacks, leader lines, and manual label changes.
- More than one map frame, the atlas, page templates, SVG export, and graticule labels. The only map furniture is a scale bar.
- Conic and transverse projections, oblique aspects, and projection suggestions.
- Editing tools, including the attribute table.
- The GPU Vello backend. The prototype draws on the CPU with `vello_cpu`.

## The labeler demo

```sh
cargo run --release -p vale-labeler-demo -- --preset world --out target/demo/world.png
```

## Tests

```sh
cargo test --workspace
```

Update the reference files of the demo with `VALE_UPDATE_REFERENCE=1`.

## License

`MIT OR Apache-2.0`. Natural Earth data is public domain. The Noto fonts use the SIL Open Font License.
