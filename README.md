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

The window opens with the Natural Earth 110m sample world. You can do these things.

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
```

`--screenshot` draws the full UI. With `--map-only` it draws the map alone. `--export` picks PNG or PDF from the file name. Text in the PDF stays text. `--probe` prints the color of one map pixel. `--report` prints the layers and the label counts.

| Argument | Meaning |
| --- | --- |
| `--projection <ID>` | `equal-earth`, `mercator`, `lambert-azimuthal`, `orthographic`, or `stereographic` |
| `--center <LON,LAT>` | Center of the projection |
| `--radius-km <KM>` | Radius of the world. The default is 6371. |
| `--look-at <LON,LAT>` | Put this place at the middle of the view |
| `--zoom <F>` | Zoom factor on the fitted view |
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

## Install a PR build on an iPad

CI builds an unsigned `.ipa` file for each PR and for `main`. SideStore is an app on the iPad that signs the file with a free Apple account and installs it. You need no Mac for each build.

Do these steps one time. They need a computer.

1. On the iPad, set a passcode and turn on Developer Mode.
2. Install SideStore and LocalDevVPN, and make the pairing file. The SideStore documentation gives the method.
3. Sign in to SideStore with an Apple account. SideStore is a third-party tool and it gets the login, so think about a separate Apple account.
4. On the iPad, open `https://zachhannum.engineer/vale/` and tap "Add the source to SideStore".

To install a build, do these steps on the iPad:

1. Connect LocalDevVPN.
2. Open the PR on GitHub and find the comment "The iPad build of commit ... is ready".
3. Tap "Install in SideStore".

The build of `main` and of each open PR is also in the source, on the page of the Vale app in SideStore.

All builds have the bundle ID `dev.vale.app`, so a new build installs over the old one and keeps the app data. The version number tells you which build is installed. It is `0.<PR>.<build>`, and PR 0 is `main`. For example, `0.112.57` is build 57, from PR 112.

Do not install the build of a PR from a fork before you read its code.

A free Apple account has these limits:

- An install stops after seven days. Open SideStore with LocalDevVPN connected to renew it.
- Only three apps can be installed at one time. SideStore is one of them.
- Only ten app IDs can be made in seven days. Vale uses one.
- Some entitlements are not available, for example iCloud containers.

Each build is a pre-release on GitHub with the tag `pr-<number>` or `main-build`. The workflow `sidestore-publish.yml` publishes it, writes the source to the `gh-pages` branch, and updates the PR comment. GitHub Pages serves that branch. When a PR closes, `sidestore-cleanup.yml` removes its build. The scripts are in `scripts/sidestore`.

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
