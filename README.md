# Vale

Vale is a GIS for fictional worlds, for desktop, iPad, and iPhone. It draws maps of invented planets with the right projections and with good labels.

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

A world opens in the globe workspace. A new world has no raster layer. A raster layer holds one value for each place, and a height layer is a raster layer with the unit `m`. A raster layer is a layer in the same draw order as the vector layers, and it has an opacity. The globe shows each visible raster layer as stepped tints, in that order. Each visible height layer shows its rivers, and its hillshade shades the slopes with a light that you set. You can do these things.

- Click "Layers" in the tool bar, and then "Add raster…", to add a raster layer. In the iPad layout, tap "+" in the Layers panel, and then "Raster…". The form has the presets "Height", "Precipitation", "Temperature", "Pressure", and "Custom". A preset fills the name, the unit, the value range, and the face size. The face size is 1,024, 2,048, 4,096, or 8,192 pixels. Next to each face size, the form shows the ground per pixel, the world map of equal detail, and the memory of a layer that is painted all over.

- Drag the globe to rotate it. On a touch screen, use one finger.
- Scroll the wheel to zoom. On a trackpad, scroll with two fingers to rotate, and pinch to zoom.
- Hold Shift and drag to twist the globe about the middle of the view. On a trackpad, use the rotate gesture.
- On a touch screen, pinch and twist with two fingers. The place under your fingers stays under your fingers.
- Click "Brush" in the tool bar to paint the selected raster layer. The brush is off if the selected layer is not a visible raster layer. The modes Carve, Ridge, Deposit, Erode, Weather, Uplift, and Ocean ridge and the rivers need a height layer. A pen or the left mouse button paints. A finger, the other mouse buttons, and Shift with a drag move the globe.
- The brush panel opens with the brush tool. It has the twelve modes (raise, lower, smooth, flatten, carve, ridge, deposit, erode, weather, uplift, warp, and ocean ridge), the radius on the ground in the display units of the world, the hardness, and the strength. In the weather mode, an Angle slider sets the steepest slope that the brush leaves. The uplift mode raises the land and cuts valleys into it while you paint. In that mode, the Uplift slider sets the rate of the rise, and the Erosion slider sets the strength of the valleys. The warp mode moves the ground sideways along a noise field, so a round coast gets bays and headlands. In that mode, the Strength slider sets the distance in kilometers, and the Scale slider sets the size of the bays. The Seed slider selects another noise field for the world. The ocean ridge mode lays a mid-ocean ridge along the stroke, with fault-block hills, fracture zones, and an axial valley or high. In that mode, the Crest slider sets the elevation of the axis, the Spreading slider and the Magma slider set the shape of the ridge, and the Segment slider and the Offset slider set the distance between the transform faults and the size of the offsets. In each mode, a Floor setting and a Ceiling setting stop the brush at an elevation. The brush changes only the ground between the floor and the ceiling, and it does not move that ground past them. A pen sets the flow from its pressure, and the mouse paints with a fixed flow.
- The globe and the flat view draw the rivers that the painted land implies, and the carve mode cuts valleys along them. The ridge mode raises the land between them, so the crests become sharp. The deposit mode fills pits to their rims, and it makes the floors of the valleys flat. While you hold the brush, the erode mode cuts valleys that branch up toward the ridges. When you zoom in, the rivers and the valleys become finer. A smaller erode brush also cuts finer valleys. The Rivers switch of a height layer hides its rivers, and the world keeps the switch of each layer. A slider below it sets the width of the lines of all rivers.
- Click "Line" in the tool bar to draw in the selected line layer. In the iPad layout, the Draw cell is the Line cell while a line layer is selected. A stroke of the pen or the mouse becomes one line feature, and Undo removes it. The Smoothing slider sets how much the app smooths the stroke. A stroke that starts near the end of a line extends that line.
- While a polygon layer is selected, the Polygon button of the tool bar is in the place of the Line button, and the Draw cell is the Polygon cell in the iPad layout. Click it to draw in the layer. A stroke becomes one polygon feature, and the short arc between its two ends closes it. Undo removes the polygon.
- While a point layer is selected, the Point button of the tool bar is in the place of the Line button, and the Draw cell is the Point cell in the iPad layout. Click it, and then tap the globe with the pen or click the globe to place a point. A form opens with a field for each field of the layer. Type the values, and then press Return or tap the place of the next point. Undo removes the point and its values.
- Click "Measure" in the tool bar, or press M, to measure a path. In the iPad layout, Measure is a cell of the tool strip. Tap or click to add points, and drag to turn the globe. Each segment shows its length, and the status bar shows the length of the path. Tap the first point or press Return to close the path, and the status bar shows the perimeter and the area. Undo removes the last point, and "Clear path" or Esc clears the path. The project does not keep the path.
- The panel of a vector layer lists its fields. Tap "Add field…" to add a field with a name, a type, and a default value. Tap a field to rename it, change its type or its default value, or delete it. In the Map workspace, the Fields part of the inspector has the same controls. Undo takes back each change.
- The brush keeps its size on screen when you zoom. Select "Lock size" to keep its size on the ground.
- The flatten mode moves the ground to the level under the start of the stroke. To set a fixed level, click "Pick from globe", and then press on the globe.
- Press Ctrl+Z to undo a stroke. Press Ctrl+Shift+Z or Ctrl+Y to redo it. On a Mac, use Command in place of Ctrl. On a touch screen, tap with two fingers to undo, and tap with three fingers to redo. The iPad layout also has an undo button and a redo button in the top row. A new stroke removes the strokes that you can redo. The undo steps use 256 MB of memory at most, and the oldest steps go first. With `--project`, the steps are in the project file, and they use 256 MB of the file at most. Undo and redo then work after you close the app and open it again. They also include each change of the world, of the vector layers, and of the color stops of a raster layer.
- Click "Flat" in the tool bar to show the whole world as one flat map. Click it again to go back to the globe. The tool and the panels stay. A drag moves the map, and the wheel zooms. You can zoom out until the map has a quarter of the size of the canvas. At each zoom, you can move each place of the map to the middle of the canvas. The brush paints the same heightmap, and its outline shows the ground that the stamp covers. In the equirectangular projection, the outline is wide near a pole.
- In the flat view, the list next to "Flat" sets the projection: equirectangular, Equal Earth, Mercator, Lambert azimuthal, orthographic, or stereographic. In the iPad layout, the button next to "Flat" opens the same list.
- "Recenter" makes the place at the middle of the canvas the center of the projection. Move the map first. "Reset" puts the center of the projection back and shows the whole map. In the iPad layout, the two actions are buttons to the right of the view switch.
- Click "New world…" in the tool bar to make a world from a name, a radius, and display units. The radius is that of Earth, Mars, or the Moon, or a value that you type. With `--project`, the new world is a new project file in the folder of the project file, and the app opens it. The file of the old world stays. With no project file, the new world has no file. Click "World…" to change the name, the radius, or the units of the world.
- The globe draws the vector layers of the world over the heightmap. The flat view of the globe workspace does not draw them.
- Click "Import heightmap…" in the tool bar to replace the height layer with an equirectangular image. If the world has no height layer, the import adds one with the name "Height". You can also drop the image on the window. The image is a PNG or TIFF file, and 16-bit greyscale gives the best result. Black is -6,000 m, and white is 6,000 m. If the image is not twice as wide as it is tall, the status bar shows a warning. In the iPad layout, the button is in the Layers panel, or in the panel of the Height layer if the world has one. "Undo" puts the old heightmap back. If the old heightmap has more painted tiles than the undo memory of 256 MB holds, the import has no undo step.
- Click "Export GeoTIFF…" in the tool bar to write the selected raster layer to an equirectangular GeoTIFF file with 16-bit depth. QGIS opens the file at the correct place and with the correct elevations. "Export PNG…" writes the same image as a 16-bit grayscale PNG, with no georeference. The status bar shows the progress, and you can paint while the export runs. In the iPad layout, the row is in the panel of the raster layer. That panel also has the name of the layer and its opacity.
- Click the arrow at the end of the row of a raster layer, or "Layer style" in the tool bar, to open the style panel of the layer. In the iPad layout, the arrow opens the panel of the layer, and its Style section has the same controls. The panel has the unit of the layer. The unit of a height layer is always `m`. The display is Bands, Gradient, or Grey. Bands shows a tint for each band. Gradient shows the color ramp with no steps. Grey shows the plain values. The gradient bar shows the colors of the layer, and the color stops are below it. Each stop is a value with a color, and each step of Bands goes from one stop to the next. Gradient blends the colors between the stops. Bands fills each step with the color of the gradient at its middle. Tap the bar to add a stop. Drag a stop to the side to move it, and pull it down to remove it. During the drag, the value of the stop shows above the bar. "Reset colors" puts back the default stops of the layer, after it asks. You cannot undo a reset. Tap a stop to select it. In the iPad layout, a card at the left of the panel then shows its value, its color, and a button that removes it. Tap the stop again, or tap another place, to close the card. The two end stops and sea level stay in place. The sea level stop of a height layer has two colors, one for the sea and one for the land. A drag up or down on the bar scrolls the panel. Each raster layer has its own unit, display, and color stops, and the project file keeps them. The switch "Band lines" in the Style section of a raster layer hides the lines at its color stops, for example while you paint. The project file keeps the switch of each layer.
- Click "Debug" in the tool bar to open the brush debug panel. It shows the stroke delay, which is the time from the frame that read the pen to the end of the GPU work. It shows the memory of the undo steps, or the size of the undo log of the project file. "Run stroke test" paints four fixed strokes and shows their numbers: the largest brush and a small fast brush, each in the raise mode and in the smooth mode. "Run zoom test" zooms the view from the whole world to a close view and back in 4 seconds, and shows the time of the frames.

The default face size is 8,192 pixels for a height layer and 2,048 pixels for each other raster layer. The selected raster layer is in memory at its full face size. Each other raster layer is in memory as a copy with a face of 2,048 pixels at most, so it looks softer in a close view until you select it. The GPU and the main memory hold only the tiles that you painted. After an import, a height layer of 8,192 pixels holds 805 MB on the GPU and 805 MB of main memory. If the computer has no GPU, start the app with `--face-size 1024`.

Click "Atlas" in the tool bar to open the pages of the atlas. On iPad, tap "Atlas" in the workspace switch. You can do these things there.

- Click "Add map frame" to add a frame to the page. The frame shows the map of the project. On iPad, tap the Add button and then "Map frame".
- Click a frame to select it. Drag the frame to move it, and drag a handle to resize it. The dock shows the tabs Frame, Projection, and Layers.
- In the Frame tab, change the shape to a polygon. Drag a vertex to move it, drag the middle of an edge to add a vertex, and double-click a vertex to remove it. "Add hole" adds a hole.
- With the Move tool, drag in the selected frame to pan its map. The key H selects the Move tool, and V selects the Select tool. Scroll in the selected frame to zoom its map. On a touch screen, two fingers in the selected frame pan, zoom, and rotate the map.
- With the Measure tool, tap or click in the map frames to measure a path. The path shows in each frame of the page, and it can cross from one frame to another.
- Drag the page outside the frames to pan it, and scroll outside the selected frame to zoom it. On a touch screen, two fingers pan and zoom the page. Double-click outside the page to fit the page in the window again.
- To delete the selected frame, click "Delete frame" in the Frame tab, or press Delete when no vertex is selected.
- Type the scale, the center, and the rotation of a frame in the Frame tab. Select a projection in the Projection tab. Each frame of the map shows the new projection.
- In the Layers tab, the eye of a layer turns the layer on or off in the map of the frame. Each frame of that map shows the change. The eye in the globe workspace does not change a map.
- In the Layers tab, open a layer to change its style in the map of the frame. Each frame of that map shows the change, and the other maps keep the style of the layer. The panel of a raster layer has the same style controls as in the World workspace, with the color stops and the hillshade. "Remove overrides" takes the changes of the map away. On the desktop, click "Style" next to a layer.
- Click the page outside the frames to set the page size, to add a page, or to delete the page. The arrows in the tool bar go to the previous page and the next page.
- In the Labels tab, tap "Add class" to add a label class to the map of the frame. A class has a text expression, a filter, a font, a size, a color, a halo, a priority, and a placement. Each frame of that map shows the change, and the other maps keep their classes.
- Tick a layer in the left panel to show it in the map. Click "Edit" on a layer to select it. The inspector then shows its name and its style.
- In the label controls of a point layer or a line layer, click the font to search the fonts of the system and select one. On iPad, open the layer and tap "Font" to open the font picker of iPadOS.
- Click a feature in a frame to see its attributes. The inspector lists the labels that did not fit.
- Export the open page to PNG or PDF, or export all pages to one PDF. A PDF keeps text selectable. The app shows the page progress, and Cancel export stops the work.

## Open your own data

```sh
cargo run --release -p vale-app -- path/to/file.geojson
```

The file must use longitude and latitude. Vale copies the data into memory.

This command keeps the world, the vector layers, and the heightmap in a project file:

```sh
cargo run --release -p vale-app -- --project path/to/world.gpkg
```

The project file is a GeoPackage, and QGIS opens its layers. If the file is not there, the app makes it from the sample world or from the GeoJSON files that you give. The app saves half a second after a change, and at once after a brush stroke. "Save" in the File menu, or Ctrl+S, saves a change that waits at once. The mark next to the File menu shows the state of the save. Each save is one undo step. A save after a brush stroke writes only the tiles that the stroke changed. The file holds the style of each layer. On iPad, each world is one project file in "On My iPad > Vale" or in "iCloud Drive > Vale" of the Files app. On iPhone, the local folder is "On My iPhone > Vale". The form of a new world picks the folder.

The start screen keeps each world in a project file in the data folder of the system. On Linux, the folder is `~/.local/share/vale`. On macOS, it is `~/Library/Application Support/Vale`. On Windows, it is `%APPDATA%\Vale`. The flag `--projects-dir` sets another folder. The flag `--cloud-dir` gives a second folder, which the app shows as the iCloud Drive folder. With `--project`, the app opens that file with no start screen, and "All worlds" shows the project files of its folder.

## Headless use

These commands need no window. They write files under `target/app/`.

```sh
cargo run --release -p vale-app -- --screenshot target/app/ui.png --size 1440x900 --report
cargo run --release -p vale-app -- --screenshot target/app/map.png --map-only --projection orthographic --center 20,30
cargo run --release -p vale-app -- --export target/app/map.pdf
cargo run --release -p vale-app -- --export target/app/map.png --pixel-ratio 2
cargo run --release -p vale-app -- --project path/to/world.gpkg --export-atlas target/app/atlas.pdf
cargo run --release -p vale-app -- --project path/to/world.gpkg --export-page 2 target/app/page-2.pdf
cargo run --release -p vale-app -- --probe 25,25
cargo run --release -p vale-app -- --report
cargo run --release -p vale-app -- --stroke-test --face-size 8192
cargo run --release -p vale-app -- --zoom-test --import-heightmap path/to/height.png
cargo run --release -p vale-app -- --project path/to/world.gpkg --export-heightmap target/app/world.tif
```

`--screenshot` draws the full UI in the globe workspace. With `--layout pad` it draws the iPad layout. In that layout, `--panel brush`, `--panel layers`, or `--panel toolbox` opens a panel, `--brush-modes` opens the list of the brush modes, and `--worlds-menu` opens the menu of the worlds. `--world-form new` or `--world-form properties` opens the form of the world. `--measure "0,0 40,10 20,40 0,0"` selects the Measure tool and adds a path of longitude and latitude pairs, and a repeated first point closes the path. `--safe-area` gives the bands of the device at the top, the right, the bottom, and the left. For example, `--size 393x852 --safe-area 59,0,34,0` draws an iPhone 16 in portrait. With `--view flat` it draws the flat view of the world, and `--flat-projection` and `--flat-center` set its projection. With `--workspace map` it draws the flat map and its panels. With `--map-only` it draws the map alone. `--export` picks PNG or PDF from the file name. `--export-atlas` writes one PDF with every page. `--export-page` writes one page as PNG or PDF. Text in a PDF stays text. `--probe` prints the color of one map pixel. `--report` prints the layers and the label counts. `--stroke-test` paints the fixed strokes of the debug panel and prints the stroke delay.

| Argument | Meaning |
| --- | --- |
| `--workspace <NAME>` | `globe` or `atlas`. The default is `globe`. |
| `--projection <ID>` | `equal-earth`, `mercator`, `lambert-azimuthal`, `orthographic`, or `stereographic` |
| `--center <LON,LAT>` | Center of the projection |
| `--radius-km <KM>` | Radius of the world. The default is 6371. A project file that exists gives its own radius. |
| `--project <GPKG>` | Project file for the world, the vector layers, and the heightmap |
| `--projects-dir <DIR>` | Folder of the project files of the start screen |
| `--start-screen` | Show the start screen. With `--screenshot`, draw it. |
| `--look-at <LON,LAT>` | Put this place at the middle of the view, on the globe and on the map |
| `--zoom <F>` | Zoom factor on the fitted view, on the globe and on the map |
| `--face-size <N>` | Pixels on one edge of a face of a raster layer that the app makes with no choice from you, for example at an import. The form of a new raster layer also offers this size and starts on it. With no flag, the window uses 8192 and a run without a window uses 1024. A raster layer of a project file keeps its own face size. |
| `--raster-form` | Open the form of a new raster layer at the start |
| `--stops-page` | With `--panel layers`, open the page of the color stops of the selected raster layer |
| `--import-heightmap <FILE>` | Import this equirectangular PNG or TIFF heightmap into the globe |
| `--export-heightmap <FILE>` | Export the first height layer of the project file of `--project` to this GeoTIFF file, or to a 16-bit PNG file if the name ends in `.png`, and exit |
| `--export-atlas <FILE>` | Export all pages of the project file to one PDF file, and exit |
| `--export-page <N> <FILE>` | Export page N of the project file to a PNG or PDF file, and exit |
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

The first time, the app opens on the start screen, which lists your worlds with a picture of each globe. After that, the app opens on the world of the last run, at its last view. The app saves with no action from you, also when it goes to the background. The mark at the left of the readout turns during a save, and it is a tick when the file holds each change. "Save" in the menu of the worlds saves at once. Tap "New world…", set the name, the radius, and the units, and tap "Create world". The world opens on the globe, in the iPad layout. On the start screen, tap a world to open it. Tap the button with three dots on a world to rename it, copy it, or delete it. Paint with Apple Pencil. Drag with one finger to turn the globe, and pinch with two fingers to zoom. To import a heightmap into a world with no height layer, tap the layers button, tap the plus button, and tap "Heightmap…" in the section "Import". The file picker of the Files app opens. After the first stroke or import, the row "Import heightmap…" is in the panel of the Height layer. The layers panel lists the layers from the bottom of the drawing to the top. Tap the plus button and tap "Lines", "Points", or "Polygons" to make a layer. Tap the eye of a layer to hide it or to show it. Hold a layer until its row lifts, and then drag it to change the order. Tap the arrow of a layer to open its panel, where you rename the layer. Swipe a row to the left to show its Delete button, and tap the button to delete the layer. Undo brings the layer back. The view switch has a button that opens the view options, with the Graticule switch. To export the heightmap, tap "Export GeoTIFF…" in that panel. After the export, the file picker of the Files app opens, and the app moves the file to the place that you pick. The globe button at the top left opens the menu of the worlds. Tap a world to open it. Tap "All worlds" to go back to the start screen. Tap "New world…" to make a world, which is a new project file. In the form, the slider sets the radius, and it stops at the marks of the Moon, Mars, and Earth. Tap the value above the slider to type a radius. The file of the old world stays. Tap "World properties" to change the name, the radius, or the units of the open world. In a narrow window, the menu of the workspaces has the row "Worlds…". To import a GeoJSON file, tap the plus button of the layers panel and tap "GeoJSON…" in the section "Import". The coordinates of the file are longitude and latitude in degrees, the same grid as an equirectangular heightmap. Each kind of geometry in the file becomes one vector layer. In the Atlas workspace, the page panel exports the open page to PNG or PDF, or all pages to one PDF.

To build for the iOS simulator on a Mac with Apple silicon, run these commands:

```sh
rustup target add aarch64-apple-ios-sim
cd crates/vale-app/ios
xcodegen generate
xcodebuild -project ValeApp.xcodeproj -scheme ValeApp -sdk iphonesimulator -arch arm64 CODE_SIGNING_ALLOWED=NO build
```

The iOS target is in `crates/vale-app/ios`. `project.yml` describes the Xcode project. `build-rust.sh` builds the Rust static library, and `toolchain.cmake` tells the PROJ build which iOS SDK to use. The static PROJ library holds its own database, so the app bundle needs no PROJ data files.

## Get a PR build on an iPad

CI uploads a build to TestFlight for each PR from a branch of this repository, and for each push to `main`. The TestFlight app on the iPad or on the iPhone then offers the build. The upload needs a membership in the Apple Developer Program.

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

The version of a build is `0.1.<PR>`, and `main` is `0.1.0`. TestFlight thus shows one row for each PR. The build number is the number of the workflow run and its attempt, for example `57.1`. The "What to Test" text of a build holds the PR number, the PR title, and the iPad test section and the iPhone test section of the PR description.

A PR from a fork gets no secrets. CI then makes the archive with no signature and uploads nothing.

All builds have the same bundle ID, so a new build installs over the old one and keeps the app data. A TestFlight build stops after 90 days.

The job `testflight` in `.github/workflows/ios.yml` does the work, with `scripts/testflight/testflight.py`.

## What the prototype leaves out

- With GeoJSON files and no `--project`, undo and redo are for brush strokes only, and the session ends when the window closes.
- Rasters and blend modes in the PNG and PDF export, regional source extents over a world source, SVG and Shapefile import, and the command Replace from file.
- Vector layers in the flat view of the globe workspace and in the picture of the globe on the start screen.
- Pattern fills, texture fills, hatch fills, and edge effects. In the style editor, only the size and the rotation of a point symbol can come from a field.
- Polygon labels, label fallbacks, leader lines, and manual label changes.
- The overview of the pages, page templates, and SVG export.
- Projection suggestions and Tissot circles.
- Editing tools other than the line tool, the polygon tool, and the point tool, including the attribute table.
- The GPU Vello backend. The prototype draws on the CPU with `vello_cpu`.

## The labeler demo

```sh
cargo run --release -p vale-labeler-demo -- --preset world --out target/demo/world.png
```

## Tests

```sh
cargo test --workspace
```

The tests that read pixels from a GPU render have the attribute `#[ignore = "gpu"]`. These commands run them. The first command skips the ignored tests with the prefix `manual_`, which save pictures or give times:

```sh
cargo test -p vale-app -- --ignored --skip manual_
cargo test -p vale-terrain --features gpu --test gpu -- --ignored
```

On Linux, the GPU tests can use Lavapipe, the software Vulkan driver of Mesa. Set `VALE_SCREENSHOTS=1` to save the pictures of the UI tests in `target/app`.

Update the reference files of the demo with `VALE_UPDATE_REFERENCE=1`.

## License

`MIT OR Apache-2.0`. Natural Earth data is public domain. The Noto fonts and the Silkscreen test font use the SIL Open Font License.
