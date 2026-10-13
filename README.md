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

The first time, the window opens on the start screen, which lists your worlds. After that, the window opens on the world of the last run, at its last view. Click "New world…" to make a world, or click the picture of a world to open it. Each world has buttons that rename it, copy it, and delete it. The File menu of the tool bar has the same actions and a list of the recent worlds. "All worlds" in the File menu goes back to the start screen.

A world opens in the globe workspace. The globe shows the heightmap of the world as stepped elevation tints. You can do these things.

- Drag the globe to rotate it. On a touch screen, use one finger.
- Scroll the wheel to zoom. On a trackpad, scroll with two fingers to rotate, and pinch to zoom.
- Hold Shift and drag to twist the globe about the middle of the view. On a trackpad, use the rotate gesture.
- On a touch screen, pinch and twist with two fingers. The place under your fingers stays under your fingers.
- Click "Brush" in the tool bar to paint the heightmap. A pen or the left mouse button paints. A finger, the other mouse buttons, and Shift with a drag move the globe.
- The brush panel opens with the brush tool. It has the ten modes (raise, lower, smooth, flatten, carve, ridge, erode, weather, uplift, and warp), the radius on the ground in the display units of the world, the hardness, and the strength. In the weather mode, an Angle slider sets the steepest slope that the brush leaves. The uplift mode raises the land and cuts valleys into it while you paint. In that mode, the Uplift slider sets the rate of the rise, and the Erosion slider sets the strength of the valleys. The warp mode moves the ground sideways along a noise field, so a round coast gets bays and headlands. In that mode, the Strength slider sets the distance in kilometers, and the Scale slider sets the size of the bays. The Seed slider selects another noise field for the world. In each mode, a Floor setting and a Ceiling setting stop the brush at an elevation. The brush does not move ground down across the floor or up across the ceiling. A pen sets the flow from its pressure, and the mouse paints with a fixed flow.
- The globe and the flat view draw the rivers that the painted land implies, and the carve mode cuts valleys along them. The ridge mode raises the land between them, so the crests become sharp. While you hold the brush, the erode mode cuts valleys that branch up toward the ridges. When you zoom in, the rivers and the valleys become finer. A smaller erode brush also cuts finer valleys. A Rivers switch in the elevation controls hides the rivers. A slider below it sets the width of the lines.
- The brush keeps its size on screen when you zoom. Select "Lock size" to keep its size on the ground.
- The flatten mode moves the ground to the level under the start of the stroke. To set a fixed level, click "Pick from globe", and then press on the globe.
- Press Ctrl+Z to undo a stroke. Press Ctrl+Shift+Z or Ctrl+Y to redo it. On a Mac, use Command in place of Ctrl. On a touch screen, tap with two fingers to undo, and tap with three fingers to redo. The iPad layout also has an undo button and a redo button in the top row. A new stroke removes the strokes that you can redo. The undo steps use 256 MB of memory at most, and the oldest steps go first. With `--project`, the steps are in the project file, and they use 256 MB of the file at most. Undo and redo then work after you close the app and open it again. They also include each change of the world, of the vector layers, and of the elevation bands.
- Click "Flat" in the tool bar to show the whole world as one flat map. Click it again to go back to the globe. The tool and the panels stay. A drag moves the map, and the wheel zooms. You can zoom out until the map has a quarter of the size of the canvas. At each zoom, you can move each place of the map to the middle of the canvas. The brush paints the same heightmap, and its outline shows the ground that the stamp covers. In the equirectangular projection, the outline is wide near a pole.
- In the flat view, the list next to "Flat" sets the projection: equirectangular, Equal Earth, Mercator, Lambert azimuthal, orthographic, or stereographic. In the iPad layout, the button next to "Flat" opens the same list.
- "Recenter" makes the place at the middle of the canvas the center of the projection. Move the map first. "Reset" puts the center of the projection back and shows the whole map. In the iPad layout, the two actions are buttons to the right of the view switch.
- Click "New world…" in the tool bar to make a world from a name, a radius, and display units. The radius is that of Earth, Mars, or the Moon, or a value that you type. With `--project`, the new world is a new project file in the folder of the project file, and the app opens it. The file of the old world stays. With no project file, the new world has no file. Click "World…" to change the name, the radius, or the units of the world.
- Click "Greyscale" in the tool bar to show the plain heightmap.
- Click "Import heightmap…" in the tool bar to replace the heightmap with an equirectangular image. You can also drop the image on the window. The image is a PNG or TIFF file, and 16-bit greyscale gives the best result. Black is -6,000 m, and white is 6,000 m. If the image is not twice as wide as it is tall, the status bar shows a warning. In the iPad layout, the button is in the Layers panel, or in the panel of the Height layer if the world has one. "Undo" puts the old heightmap back. If the old heightmap has more painted tiles than the undo memory of 256 MB holds, the import has no undo step.
- Click "Export GeoTIFF…" in the tool bar to write the heightmap to an equirectangular GeoTIFF file with 16-bit depth. QGIS opens the file at the correct place and with the correct elevations. The status bar shows the progress, and you can paint while the export runs. In the iPad layout, the row is in the panel of the Height layer.
- Click "Elevation" in the tool bar to open the elevation panel. The bar shows the band limits, which are the elevations where the tints change. Tap a free place on the bar to add a limit. Drag a limit to move it. Tap a limit and click "Remove" to remove it. Sea level is a limit that stays. Clear "Levels" to show the color ramp with no steps. The land colors and the sea colors are separate.
- Click "Debug" in the tool bar to open the brush debug panel. It shows the stroke delay, which is the time from the frame that read the pen to the end of the GPU work. It shows the memory of the undo steps, or the size of the undo log of the project file. "Run stroke test" paints four fixed strokes and shows their numbers: the largest brush and a small fast brush, each in the raise mode and in the smooth mode.

In the window, each face of the heightmap has 8,192 pixels. The GPU and the main memory hold only the tiles that you painted. After an import, the heightmap holds 805 MB on the GPU and 805 MB of main memory. If the computer has no GPU, start the app with `--face-size 1024`.

Click "Map" in the tool bar to open the flat map with the Natural Earth 110m sample world. You can do these things there.

- Drag the map to pan. Scroll or pinch to zoom. On a touch screen, two fingers pan and zoom.
- Double-click the map to move the center of the projection there.
- Use the left panel to set the projection and its center.
- Tick a layer to show it. Click "Edit" on a layer to select it. The inspector then shows its style.
- Click a feature to see its attributes. The inspector lists the labels that did not fit.
- Open a GeoJSON file with the button, or drop a GeoJSON file on the window.
- Export the view to PNG or PDF with the buttons in the tool bar.

## Open your own data

```sh
cargo run --release -p vale-app -- path/to/file.geojson
```

The file must use longitude and latitude. Vale copies the data into memory.

This command keeps the world, the vector layers, and the heightmap in a project file:

```sh
cargo run --release -p vale-app -- --project path/to/world.gpkg
```

The project file is a GeoPackage, and QGIS opens its layers. If the file is not there, the app makes it from the sample world or from the GeoJSON files that you give. The app saves half a second after a change, and at once after a brush stroke. "Save" in the File menu, or Ctrl+S, saves a change that waits at once. The mark next to the File menu shows the state of the save. Each save is one undo step. A save after a brush stroke writes only the tiles that the stroke changed. The file does not hold the styles. On iPad, each world is one project file in "On My iPad > Vale" or in "iCloud Drive > Vale" of the Files app. The form of a new world picks the folder.

The start screen keeps each world in a project file in the data folder of the system. On Linux, the folder is `~/.local/share/vale`. On macOS, it is `~/Library/Application Support/Vale`. On Windows, it is `%APPDATA%\Vale`. The flag `--projects-dir` sets another folder. The flag `--cloud-dir` gives a second folder, which the app shows as the iCloud Drive folder. With `--project`, the app opens that file with no start screen, and "All worlds" shows the project files of its folder.

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
cargo run --release -p vale-app -- --project path/to/world.gpkg --export-heightmap target/app/world.tif
```

`--screenshot` draws the full UI in the globe workspace. With `--layout pad` it draws the iPad layout. In that layout, `--panel brush`, `--panel layers`, or `--panel toolbox` opens a panel, `--more-tools` opens the menu of the More cell, and `--worlds-menu` opens the menu of the worlds. `--world-form new` or `--world-form properties` opens the form of the world. With `--view flat` it draws the flat view of the world, and `--flat-projection` and `--flat-center` set its projection. With `--workspace map` it draws the flat map and its panels. With `--map-only` it draws the map alone. `--export` picks PNG or PDF from the file name. Text in the PDF stays text. `--probe` prints the color of one map pixel. `--report` prints the layers and the label counts. `--stroke-test` paints the fixed strokes of the debug panel and prints the stroke delay.

| Argument | Meaning |
| --- | --- |
| `--workspace <NAME>` | `globe` or `map`. The default is `globe`. |
| `--projection <ID>` | `equal-earth`, `mercator`, `lambert-azimuthal`, `orthographic`, or `stereographic` |
| `--center <LON,LAT>` | Center of the projection |
| `--radius-km <KM>` | Radius of the world. The default is 6371. A project file that exists gives its own radius. |
| `--project <GPKG>` | Project file for the world, the vector layers, and the heightmap |
| `--projects-dir <DIR>` | Folder of the project files of the start screen |
| `--start-screen` | Show the start screen. With `--screenshot`, draw it. |
| `--look-at <LON,LAT>` | Put this place at the middle of the view, on the globe and on the map |
| `--zoom <F>` | Zoom factor on the fitted view, on the globe and on the map |
| `--face-size <N>` | Pixels on one edge of a heightmap face. The default is 8192 in the window and 1024 without a window. A project file that holds a heightmap gives its own face size. |
| `--import-heightmap <FILE>` | Import this equirectangular PNG or TIFF heightmap into the globe |
| `--export-heightmap <FILE>` | Export the heightmap of the project file of `--project` to this GeoTIFF file, and exit |
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

The first time, the app opens on the start screen, which lists your worlds with a picture of each globe. After that, the app opens on the world of the last run, at its last view. The app saves with no action from you, also when it goes to the background. The mark at the left of the readout turns during a save, and it is a tick when the file holds each change. "Save" in the menu of the worlds saves at once. Tap "New world…", set the name, the radius, and the units, and tap "Create world". The world opens on the globe, in the iPad layout. On the start screen, tap a world to open it. Tap the button with three dots on a world to rename it, copy it, or delete it. Paint with Apple Pencil. Drag with one finger to turn the globe, and pinch with two fingers to zoom. To import a heightmap, tap the layers button and tap "Import heightmap…". The file picker of the Files app opens. After the first stroke or import, the row is in the panel of the Height layer. To export the heightmap, tap "Export GeoTIFF…" in that panel. After the export, the file picker of the Files app opens, and the app moves the file to the place that you pick. The globe button at the top left opens the menu of the worlds. Tap a world to open it. Tap "All worlds" to go back to the start screen. Tap "New world…" to make a world, which is a new project file. In the form, the slider sets the radius, and it stops at the marks of the Moon, Mars, and Earth. Tap the value above the slider to type a radius. The file of the old world stays. Tap "World properties" to change the name, the radius, or the units of the open world. In a narrow window, the menu of the workspaces has the row "Worlds…". The iPad app has no other buttons to open or export files. It does not open GeoJSON files, and it does not export a map to PNG or PDF.

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

- The styles in the project file. With GeoJSON files and no `--project`, undo and redo are for brush strokes only, and the session ends when the window closes.
- Raster layers, SVG and Shapefile import, and the command Replace from file.
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
