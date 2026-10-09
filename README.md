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

- Drag the map to pan. Scroll or pinch to zoom.
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
