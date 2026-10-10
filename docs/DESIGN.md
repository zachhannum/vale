# Vale: Design Document

Oct 8, 2026 · Zach Hannum

## Summary

Vale is a GIS for fictional worlds, for desktop and iPad. One world dataset is the source of truth, and each regional map is a projected, styled, labeled view of that dataset.

You draw the world in the app, on a globe. You paint a heightmap (a greyscale image where brightness is elevation) with a soft brush, and the app turns it into vector topography.

The first differentiator is an automatic labeling engine at the level of the ArcGIS Maplex engine, built clean-room as a standalone library. The second differentiator is first-class projection support for a world of any size. The third differentiator is drawing on the sphere, so you never draw on a distorted flat map.

The stack is Rust, with PROJ for projections, GeoPackage for storage, Vello for screen rendering, and Krilla for PDF export.

The app and the labeling library are free and open source, under a dual MIT and Apache-2.0 license.

## Goals and non-goals

Version 1 has ten goals.

- Run on macOS, Windows, Linux, and iPad, with pen input on each. iPad is an equal platform and not a later port.
- Paint a heightmap on a globe with a pressure-sensitive brush, and see it as stepped elevation tints while you paint.
- Turn the heightmap into vector topography polygons with a tool from the toolbox.
- Import equirectangular vector and raster sources, and georeference them (assign world coordinates to them) in under a minute.
- Define a world as a sphere of any radius.
- Make regional maps, each in a projection that fits its region.
- Place labels automatically from stored attributes, at a quality that needs little manual repair.
- When the data, the projection, or the map frame changes, keep manual changes.
- Export an atlas as print-quality PDF, SVG, and raster files.
- Stay compatible with QGIS through open formats.

Version 1 does not include the items that follow.

- Procedural world generation. This is a permanent non-goal, because the app presents a world that you designed.
- A general painting or illustration tool. The app paints one thing, the heightmap. Photoshop and Illustrator keep color artwork and textures.
- Hillshade, erosion, and other terrain effects.
- Time-based data such as historical borders, and non-spherical worlds. These items come after version 1.
- Real-world GIS analysis such as routing or geocoding.
- Web and phone versions.

## Core workflow

The app has one main workflow with four steps, and every design choice in this document serves it.

1. Define the world. You set the radius, the name, and the display units.
2. Draw the world. You paint the heightmap on the globe, run the polygonize tool to get topography polygons, and draw rivers and borders as lines. You can also import equirectangular vector or raster files that you made in other tools.
3. Make regional maps. You draw a map frame over a region, select a projection, place data such as cities, apply styles, and generate labels from attributes. The app draws the graticule (the grid of latitude and longitude lines).
4. Export the atlas. You order the map frames into pages and export all of them with shared styles.

## Technology choices

The app is one Rust workspace. Rust gives one language for the engine and the app, and it has strong crates for geometry, text, and PDF output.

| Area | Choice | Reason |
| --- | --- | --- |
| Language | Rust | One language for all layers. The labeling library stays easy to embed in other tools. |
| App shell and panels | egui, through eframe on wgpu | Fast to build tool panels. It shares one GPU context with the map canvas. |
| Screen rendering | Vello | GPU vector renderer that draws paths, glyphs, and images on wgpu. |
| Text shaping and layout | Parley | Shaping, font fallback, and system font discovery in one crate. |
| Page-space geometry | kurbo | Curves, arc length, and offsets. Vello, Parley, and Krilla all use it. |
| World-space geometry | geo and rstar | Feature types, boolean operations, and an R-tree spatial index. |
| PDF export | Krilla | Text stays text, fonts are subset and embedded, vectors stay vectors. |
| SVG export | Own writer over the display list | The display list is small, so a writer is a few hundred lines. |
| Raster export | Vello, rendered in tiles | Tiles avoid the GPU texture size limit at print resolution. |
| Projections | PROJ, through the proj crate with a bundled build | Accepts a sphere of any radius and covers all common projections. |
| Spherical geometry | Own module, after the design of d3-geo | Clipping and adaptive resampling on a sphere are small, well-known algorithms. |
| Storage | GeoPackage, through rusqlite with bundled SQLite | One file, transactions, and QGIS reads it directly. |
| Vector import | Pure-Rust readers for SVG, GeoJSON, Shapefile, and GeoPackage | No native dependency to package. |
| Raster import | image, tiff, and psd crates | PNG, TIFF with 16-bit depth, and flattened PSD cover the usual tools. |
| Raster reprojection | Own warp in a wgpu shader | PROJ computes a coarse mesh on the CPU, and the GPU interpolates and samples. |
| File watch | notify crate | Detects changes to linked sources on all three platforms. |
| Heightmap storage | Cube map (six square images that wrap the sphere) in 16-bit tiles | Pixel size stays close to uniform over the sphere, and no pole has a pinch. |
| Heightmap painting | Own brush engine in wgpu shaders | The brush stamps into the cube map tiles, and the stepped preview is one shader pass. |

Three choices need a comment.

GDAL is not in version 1. Fantasy map sources are PNG, TIFF, PSD, and SVG files, and pure-Rust readers cover those. GDAL is hard to package on three platforms, and you can add it later behind the import interface.

The renderer writes a display list (a flat list of paths, glyph runs, and images in page coordinates). Each output backend reads that list. This boundary keeps screen and print output identical, and it lets you replace Vello or Krilla without a change to the map pipeline.

egui is the least certain choice. It is productive for tool panels, but complex tables and rich text entry take more work than in a web UI. The core crates have no UI dependency, so a move to Tauri or Qt later stays possible.

These choices come from memory and not from a fresh survey. Make sure of the current release status of Vello, Parley, and Krilla before you commit.

The status was checked on 2026-10-08. The current releases are Vello 0.11.0 (2026-10-02), Parley 0.11.1 (2026-08-16), and Krilla 0.8.2 (2026-06-04). All three are before 1.0 and active. The labeling prototype depends on Parley 0.11.1 and on `vello_cpu` 0.3.0, the CPU renderer of the Vello project. It does not depend on GPU Vello or on Krilla.

The app prototype depends on Krilla 0.8.2, eframe 0.36.2, and `proj` 0.31.0. It does not depend on GPU Vello. Vello 0.11.0 and eframe 0.36.2 both use `wgpu` 30, so a GPU Vello backend can share the egui device later.

## Architecture

The workspace has eight crates, and the labeling engine depends on none of the others.

```
                app (egui shell, tools, panels: calls every crate below)

import ──> store ──> sphere ──> style ─────────────────────> render
                                  │                             ^
                                  │ projected geometry          │ placed text
                                  v                             │
                                labeler (standalone library) ───┘
```

Data moves from left to right. The terrain crate is not in the diagram. It writes the heightmap and the topography polygons to the store. The labeler sits beside the main line, takes projected geometry from the style step, and returns placed text to the renderer.

| Crate | Role |
| --- | --- |
| store | Data model, GeoPackage read and write, undo log |
| sphere | Spherical geometry, PROJ wrapper, graticule generation |
| import | File readers, linked sources, file watch |
| terrain | Heightmap tiles, brush engine, heightmap tools |
| style | Style rules, expressions, symbol generation |
| labeler | Standalone labeling engine. It takes page-space geometry and returns placed text. |
| render | Display list and the Vello, Krilla, SVG, and raster backends |
| app | egui shell, tools, panels |

An unpublished ninth crate, `vale-labeler-demo`, holds the command-line tool for the labeling prototype. The binary of the app crate is named `vale-app`.

An unpublished tenth crate, `vale-globe-proto`, is a prototype that tests pen drawing on the globe. The section "Globe prototype" describes it.

The published crate names take a `vale-` prefix, for example `vale-labeler` and `vale-sphere`. The plain name `vale` is taken on crates.io by an unrelated validation library.

Each map frame runs the same pipeline, on screen and at export.

1. Query. The store returns the features that intersect the frame region.
2. Resample and clip. The sphere crate adds points along each line so that it curves correctly, and it cuts geometry at the edge of the projection.
3. Project. PROJ converts longitude and latitude to page coordinates.
4. Style. The rules turn each feature into symbols.
5. Label. The labeler places text against the projected geometry and the symbols.
6. Emit. The render crate writes the display list.
7. Output. One backend draws the list to the screen, a PDF, an SVG, or a raster file.

Labeling runs after projection, so the engine never sees longitude and latitude. That rule keeps the library usable for real-world maps too.

## Data model and file format

A project is one GeoPackage file. Features live in standard GeoPackage tables, and the app keeps its own records in extra tables in the same file.

| Record | Content |
| --- | --- |
| World | Radius, name, display units |
| Source | Path to a linked file, extents, last known file hash |
| Raster | Cube map tiles, unit, value range, ramp, band limits |
| Layer | Type (raster, points, lines, or polygons), attribute schema, link to a source, to a raster, or to an in-app table |
| Feature | Geometry in longitude and latitude, attributes, a stable ID |
| Style | Ordered rules of one layer, stored as JSON |
| Label class | Text expression, font, placement rules, priority |
| Map frame | Region, projection, scale, page size, style overrides, label classes |
| Override | One manual change, keyed by map frame, feature ID, and label class |
| Atlas | Ordered list of map frames and page templates |

QGIS opens the feature tables directly and ignores the extra tables. That gives interchange with no export step. The topography polygons are feature tables, so QGIS reads them too. The heightmap is not a standard table, and the app exports it as an equirectangular GeoTIFF.

Each feature has a UUID in addition to the integer row ID. The UUID never changes, so overrides and styles survive an edit, a reload, or a new projection.

Overrides belong to a map frame and not to the feature. A label that you move on the regional map stays where the engine put it on the world map.

Each edit is one SQLite transaction with an entry in an undo log table. Undo and redo then work across sessions, and a crash cannot leave a half-written project.

## World definition and projections

A world is a sphere with one radius, and all stored geometry is longitude and latitude in degrees on that sphere. In PROJ terms the world is `+proj=longlat +R=<radius>`.

A projection in the app is a PROJ string plus a name. A custom projection is a preset that you edit and save, so the app needs no projection math of its own.

When you draw a map frame, the app suggests a projection from the shape and latitude of the region.

| Region | Suggested projection | PROJ name |
| --- | --- | --- |
| Whole world | Equal Earth | eqearth |
| Hemisphere or continent | Lambert azimuthal equal-area, centered on the region | laea |
| Mid-latitude region, wider than tall | Lambert conformal conic | lcc |
| Mid-latitude region where area matters | Albers equal-area conic | aea |
| Region taller than wide | Transverse Mercator | tmerc |
| Region on the equator | Mercator | merc |
| Polar region | Polar stereographic | stere |
| Globe view | Orthographic | ortho |

For the two conic projections, the app sets the standard parallels at one sixth and five sixths of the latitude range. That is a common rule that keeps scale error low across the frame.

The projection editor exposes the parameters that matter: center, standard parallels, and rotation. An oblique aspect (a projection tilted so that its center line follows the region) uses the PROJ `ob_tran` wrapper.

The editor shows distortion while you edit. It draws Tissot circles (equal circles on the sphere that show local stretch after projection) and prints the scale error at the frame corners.

The sphere crate generates graticule lines on the sphere and sends them through the same pipeline as features. The interval follows the map scale, and the labels sit where each line meets the frame edge.

A scale bar is correct only where the projection keeps scale. The app measures the scale bar at the frame center and states that in the bar's properties.

## Import and linked sources

An imported file stays where it is, and the project stores a link to it plus its extents. You can then edit the file in Photoshop or Illustrator and keep the georeferencing.

A linked file is a layer. You add it from the Layers panel, and the panel of the layer has three ways to set extents.

- Full globe. This is the default, and it maps the file to longitude -180 to 180 and latitude -90 to 90.
- Four numbers. You type the west, east, south, and north edges.
- Two control points. You click two known places and type their coordinates.

If a full-globe file is not twice as wide as it is tall, the panel shows a warning. A wrong aspect ratio is the most common import error.

The app watches each linked file. When the file changes on disk, the app reloads it and draws all map frames again. If the file is missing, the layer shows a broken-link state and keeps its styles and overrides.

Stable identity is the hard part for linked vector files. For SVG, the app maps each layer by name and each feature by its element ID. If an element has no ID, the app matches features by geometry, and a large edit can then break the match. The app reports each lost match after a reload.

When the app reprojects a raster, the raster loses sharpness, so resolution matters. The rule is that the source needs at least as many pixels per kilometer as the output.

A worked example shows the size of the problem. An Earth-size world at 16,384 by 8,192 pixels has 2.4 km per pixel at the equator. A 2,000 km regional map at 3,000 pixels wide needs 0.67 km per pixel. The world raster is 3.7 times too coarse for that map.

The app has two answers. A regional source with partial extents draws over the world source inside its area. For coastlines, borders, and rivers, use vector layers, because those stay sharp at any scale.

## Drawing on the globe

The world view is a globe that you rotate, in the orthographic projection. You draw on it directly. All drawing tools work in sphere coordinates, so the same tools also work in a map frame of any projection. A switch shows the world as a flat map, and the same tools work there. The brush outline shows its true projected shape there, so you see the distortion of the flat map.

This removes the main pain of the old workflow. You no longer paint on an equirectangular image, where shapes stretch toward the poles. You no longer paint in several projections and join the parts.

### Heightmap

A heightmap is a raster layer of values with meters as its unit. A world can have other raster layers of values, for example precipitation. Each one has the same store, brush, and preview. A new world has no raster layer.

The heightmap is a 16-bit greyscale cube map. Each of the six faces splits into tiles, and the project stores only the tiles that you painted.

An equirectangular image is the wrong store. It spends most of its pixels near the poles, and a round brush becomes a wide ellipse there. On a cube map with equal-angle spacing, a pixel covers close to the same ground everywhere.

A worked example gives the resolution. With faces of 8,192 pixels, the equator has 32,768 pixels. On an Earth-size world that is 1.2 km per pixel.

### Brush

The brush is a circle on the sphere with a radius in kilometers and a soft edge. It has four modes: raise, lower, smooth, and flatten to a level.

A pen sets the flow from pressure. A mouse uses a fixed flow. The brush size follows the zoom unless you lock it.

### Stepped preview

While you paint, a shader shows the heightmap as stepped tints. It cuts the height at the band limits and colors each step from a ramp. This is the posterize and gradient overlay of the old workflow, live. A raster layer can also show as a smooth gradient or as plain greyscale.

The band limits and the ramp belong to the raster layer. The preview and the polygonize tool read the same limits, so the polygons match what you saw.

### Toolbox

The toolbox is a searchable list of tools, open from every workspace. A tool takes one or more layers and a few parameters, and it writes a new layer. Polygonize is one tool in the list and not a button of its own.

Version 1 has six tools.

| Tool | Input | Output |
| --- | --- | --- |
| Polygonize | A raster layer, band limits | Polygons, one for each band |
| Contour lines | Heightmap, interval | Lines with an elevation attribute |
| Smooth | Lines or polygons | The same layer with smoother outlines |
| Simplify | Lines or polygons | The same layer with fewer points |
| Dissolve | Polygons, an attribute | Polygons joined where the attribute is equal |
| Clip | A layer, a polygon or map frame | The part of the layer inside the shape |

The output layer keeps a record of the tool, its inputs, and its parameters. When an input changes, the layer shows that it is out of date, and you run the tool again with one action.

All tools work on the sphere. They live in the core crates, so the command line can run them too.

### Polygonize

The polygonize tool traces each band limit on each cube face, joins the outlines across face edges, and smooths them. It writes one polygon layer. It can also split the output at a value into two layers, for example Topography and Bathymetry at 0 m. Each polygon of a heightmap has an `elev_min` attribute, and a normal style rule colors it.

The polygons are an output layer of the toolbox. When the heightmap changes, the layer shows that it is out of date, and you run the tool again. Map frames and export use the polygons and not the heightmap, so topography stays vector in PDF and SVG.

### Lines

A freehand line tool draws rivers, borders, and roads on the globe. The app smooths the stroke and stores it as a normal line feature.

### Import

You can import an equirectangular heightmap that you painted before. The app copies it into the cube map. It does not link to the file, because you continue the work in the app.

### Pen and iPad

The drawing tools are designed for a pen: pressure, hover preview of the brush, and palm rejection. On desktop that is a graphics tablet. On iPad it is Apple Pencil.

iPad is an equal platform. The layout for iPad is its own design, with a full-screen canvas, floating panels, and touch-size controls. It is not the desktop layout made smaller.

The core crates have no UI dependency, so both platforms share them. The shell is egui on both platforms. The crate `vale-globe-proto` checked how well egui handles pen input and touch on iPad, and the section "Globe prototype" gives the results.

## Styling model

A style is an ordered list of rules. Each rule has a filter, a scale range, and one or more symbolizers (instructions that draw a feature). This is the QGIS rule-based model, and it lets one style serve all map frames.

A filter is an expression on attributes, for example `population > 50000 and kind = 'city'`. Each symbolizer property can also be an expression, so the line width of a river can come from a `flow` attribute.

Each layer has one style. You edit it in the panel of the layer, from each workspace. The globe, the flat view, and each map frame draw the layer with that style. A map frame can override single properties without a copy of the full style.

The symbolizers must give enough artistic range that the output does not look like a survey map.

| Symbolizer | Options in version 1 |
| --- | --- |
| Stroke | Solid, dashed, textured brush, taper along the line for rivers |
| Fill | Solid color, pattern, texture image, hatch lines |
| Edge effect | Coast lines repeated outward into water, inner shade along a border |
| Point symbol | SVG symbol with size and rotation from attributes |
| Scatter | Symbols spread inside a polygon, for forests and mountain ranges |
| Raster | Color ramp, smooth or stepped, opacity, blend mode |
| Page | Paper texture and a frame border |

Textures and blend modes become raster images in PDF and SVG output. All other symbolizers stay vector.

## Labeling engine

The labeler is a standalone library that takes page-space geometry, label classes, and obstacles, and returns placed glyph runs. It is the main differentiator, so its work starts first and it has its own tests.

The engine supports the placements that readers notice on a good map.

| Feature type | Placements |
| --- | --- |
| Point | Positions around the symbol in a preference order, with an optional leader line |
| Line | Text that curves along a smoothed copy of the line, above, below, or centered, with repeats on long lines |
| Polygon | Text that curves along the long axis of the shape and spreads its letters, or a horizontal label at the visual center |
| Polygon too small for its label | Label outside the shape with a leader line, or a smaller font step |
| Graticule and frame edge | Text at the frame edge, aligned to the line |

Placement runs in five steps.

1. Generate candidates. Each feature gets a list of possible positions from its label class.
2. Score. Each candidate gets a cost from position preference, curvature, distance from the feature, and overlap with obstacles.
3. Place by priority. The engine places the best candidate of each feature in priority order and skips candidates that collide.
4. Improve. A local search moves and swaps labels to lower the total cost. The search uses a fixed seed, so the same input always gives the same output.
5. Fall back. For each label that does not fit, the engine tries the fallbacks of its class in order: smaller font, stacked lines, abbreviation, leader line, and then removal.

The engine returns each unplaced label with a reason. The app lists them, so no label goes missing in silence.

Manual control is part of the design and not a repair tool. You can pin a label, move it, select a different candidate, or exclude it. A pinned label becomes a fixed obstacle, and the engine places all other labels around it.

The library takes its fonts through Parley and measures real glyph outlines. Collision tests use one box per glyph on curved text, so curved labels pack tightly.

The test set is real-world data, for example Natural Earth, rendered to images and compared with approved reference images. Real data has harder label problems than most fantasy maps.

Clean-room has a strict meaning here. The design comes from published research and from the public description of Maplex behavior in Esri documentation. No one on the project reads Esri code or decompiles Esri software. The starting points are Imhof's rules for name placement (1975) and the label placement study by Christensen, Marks, and Shieber (1995).

## Map frames and atlas export

A map frame is a saved view of the world: a region, a projection, a scale, a page size, style overrides, and label classes. An atlas is an ordered list of map frames with page templates.

A page template holds the map furniture (the items around the map). Version 1 has a title, a legend, a scale bar, graticule labels, a north arrow, and a locator map. The locator map is a small world map that marks the frame region, and the app generates it from the frame.

The atlas exports to three formats.

| Format | Use | Notes |
| --- | --- | --- |
| PDF | Print and books | One file with all pages. Text stays selectable, and fonts are subset and embedded. |
| SVG | More work in Illustrator or Inkscape | One file per page, with layers kept as groups. |
| PNG or TIFF | Web and virtual tabletop tools | One file per page at a set resolution, rendered in tiles. |

Export uses RGB color in version 1. CMYK output for commercial print is an open question.

A place-name index with grid references comes after version 1. The data model already supports it, because labels come from stored features.

## Editing scope

Version 1 draws the shape of the world in the app and leaves color artwork to external tools.

The app has seven editing tools in version 1.

- Heightmap painting on the globe.
- The toolbox, with polygonize and five other tools.
- Freehand lines on the globe.
- Point placement with an attribute form, for cities, landmarks, and other named places.
- An attribute table with sort, filter, and bulk edit.
- Line and polygon editing by vertex, with snapping and a smooth operation.
- Label editing on the map: pin, move, select a candidate, exclude.

Photoshop, Illustrator, and similar tools keep color artwork and textures. The linked-source design makes that round trip cheap.

A full vector drawing tool set comes after version 1.

## Phases

The labeling prototype came first, and it is done. The work after it has eleven phases, in the order of the table. The globe comes first, because you draw the world in the app. Thus the drawing tools arrive before the map tools.

| Phase | Name | At the end of the phase |
| --- | --- | --- |
| 0 | iPad test loop and CI | Each PR arrives on the iPad through TestFlight, and CI builds all four platforms. |
| 1 | Globe and heightmap painting | You paint a heightmap on the globe with Apple Pencil or a pen tablet, in the real app. |
| 2 | Projects and storage | The world that you paint is a project file that you can close, open again, and move between devices. |
| 3 | Toolbox and freehand lines | The toolbox turns the heightmap into topography polygons, and you draw rivers and borders as lines. |
| 4 | Map frames and projections | You draw a map frame on the globe, and the app shows that region in a projection that fits it. |
| 5 | Vector data editing | You place cities and other points, edit attributes, and edit lines and polygons by vertex. |
| 6 | Styling | Rule-based styles draw all symbolizers of version 1 on screen and in PDF. |
| 7 | Labeling in the app | Labels come from attributes, polygon labels and fallbacks work, and manual changes stay. |
| 8 | Atlas and export | An atlas of map frames exports as a multi-page PDF, SVG files, and raster files. |
| 9 | Import and linked sources | Files from other tools come in as linked sources and reload when they change. |
| 10 | Version 1 release | Version 1 is in the App Store and on the three desktop platforms. |

From phase 1 on, a phase is done only when it works on desktop and on iPad.

No phase is a command-line tool alone. Five command-line tools stay, and each one runs the same core crates as the app.

- `vale-labeler-demo` is the regression test of the labeler.
- In phase 2, a command exports the heightmap as an equirectangular GeoTIFF.
- In phase 3, a command runs a toolbox tool on a project file.
- In phase 4, a command renders one map frame of a custom-radius world to PDF, with clipping and a graticule.
- In phase 8, a command exports the atlas as a multi-page PDF.

The app prototype covers a thin part of phases 2, 4, 6, 8, and 9. It does not finish any of them.

### Labeling prototype

The labeling prototype differs from the design above in five ways. Each difference ends when the crate named for it exists.

1. A separate crate, `vale-labeler-demo`, holds the command-line tool for data loading, projection, and rendering. It is not published. It ends when `vale-render` and the later command-line tools take over its jobs.
2. The demo rasterizes with `vello_cpu`. It now draws through the display list of `vale-render`. This difference ends when the GPU Vello backend exists.
3. The demo has two hand-written spherical projections, Equal Earth and Lambert azimuthal equal-area. They stay in the demo as a fixed test input. The app uses PROJ through `vale-sphere`.
4. The labeler uses fonts that the caller registers as bytes. System font discovery is available but off by default, so output is the same on every machine. This ends when `vale-app` and `vale-style` choose fonts for the user.
5. Of the five fallbacks in the labeling engine, the prototype has only the last one, removal with a reason. The other four arrive in phase 7.

The labeling prototype is done. The command `cargo run --release -p vale-labeler-demo -- --preset world` writes the labeled PNG.

### App prototype

The app prototype is a vertical slice. It makes a runnable app early. It takes a thin part of phases 2, 4, 6, 8, and 9 at once. It differs from the design above in nine ways. The command `cargo run --release -p vale-app` opens the window. The command `cargo run --release -p vale-app -- --screenshot target/app/ui.png --size 1440x900 --report` draws the full UI to a PNG without a window.

The app opens on the globe workspace. The globe view and its navigation are in `vale-app`, and the globe draws the heightmap of `vale-terrain` as stepped tints with a graticule. An elevation panel edits the band limits and the ramp. A brush tool paints the heightmap, and a brush panel has the four modes, the radius in kilometers, the size lock, the hardness, the strength, and the flatten level. The size lock holds the radius as an angle on the sphere. The GPU texture holds six full faces of 8,192 pixels, which is 805 MB, and a copy of one face for the brush takes 134 MB more. A switch in the tool bar opens the flat map, and the nine items below are about the flat map.

The brush stamps in a wgpu shader of `vale-terrain`, one pass for each face that the stamp touches. Up to 32 raise, lower, or flatten stamps share one pass. A smooth stamp has its own pass, because it reads the texels around it. The pass writes only the rectangle under the brush. While you paint, the GPU texture is the working copy. At the end of a stroke the app reads the changed texels back into the heightmap of `vale-terrain`, which stays the document. The CPU stamp is the reference, and the tests of `vale-terrain` compare the two. The result of one stamp differs by at most 1 of the 65,536 levels, and by at most 2 in the smooth mode. Over a stroke of many raise or lower stamps, the tests allow a difference of 3. A debug panel shows the stroke delay, which is the time from the frame that read the pen to the end of the GPU work.

1. The prototype is a vertical slice through phases 2, 4, 6, 8, and 9. It does not finish any of them.
2. The map is drawn by `vello_cpu` on screen and in PNG files. The display list of `vale-render` exists, and the GPU Vello backend does not. The same pixels come out with and without a window, so tests need no GPU. The PDF export uses Krilla, and text in the PDF stays text.
3. `vale-store` holds the project in memory. There is no GeoPackage file, no undo log, and no UUID. A feature ID is its index in the layer.
4. `vale-import` reads GeoJSON only, and it copies the data. There are no linked sources and no file watch.
5. `vale-sphere` clips with one simple method: rotate, unwrap, and clip in a plane. A polygon that covers more than half of the sphere is not supported. A ring closes along the short longitude way. `geo` and `rstar` are not dependencies yet.
6. `vale-style` has one fixed rule for each layer, with a solid fill, a solid stroke, a circle symbol, and one label class.
7. The map pipeline (query, clip, project, style, label, emit) lives in the library part of `vale-app`, in modules that do not use egui.
8. A projection in the app is one of five presets plus a center. The app builds the PROJ string from the preset.
9. `vale-labeler-demo` stays as the regression test of the labeler. It keeps its own GeoJSON reader and its two hand-written projections, and it now draws through the display list of `vale-render`.

The prototype leaves out the following.

- Project files. There is no GeoPackage, no save, and no undo.
- Linked sources, file watch, raster layers, SVG and Shapefile import, and georeferencing with extents or control points.
- Rule-based styles, expressions, scale ranges, and all symbolizers other than a solid fill, a solid stroke, and a circle.
- Polygon labels, label fallbacks, leader lines, and manual label changes (pin, move, exclude).
- More than one map frame, the atlas, page templates, map furniture other than a scale bar, graticule labels, SVG export, and tiled raster export.
- Conic and transverse projections, oblique aspects, projection suggestions, and Tissot circles.
- All editing tools: point placement, attribute table, and vertex editing.
- The GPU Vello backend. The prototype draws with `vello_cpu`.
- Procedural world generation. This is a permanent non-goal.

### Globe prototype

The globe prototype is the crate `vale-globe-proto`. It tests two things ahead of phase 1: that a brush on a cube map stays round at every place on the sphere, and that egui can take Apple Pencil input on iPad. The heightmap and the brush come from `vale-terrain`. The command `cargo run --release -p vale-globe-proto` opens the window, and `scripts/globe-proto-ipad.sh` builds the app and starts it on a connected iPad.

The prototype has a pressure brush with the four modes, the stepped preview in one shader pass, a freehand line tool, and a panel that shows what the pen delivers. The unit tests of `vale-terrain` make sure that one stamp covers the same ground at the equator, at a pole, on a face edge, and at a cube corner.

It differs from the design in four ways.

1. The brush stamps on the CPU, and the prototype sends the changed texels to the GPU. The design stamps in wgpu shaders, and `vale-app` does.
2. The heightmap has no project file, and the GPU texture holds six full faces.
3. Pen input comes through egui touch events. A touch with a force counts as a pen, because on iPad only Apple Pencil has a force.
4. Hover, tilt, and the 240 Hz pen samples do not arrive, because winit 0.30 does not read them on iOS. If the iPad test shows that the app needs them, a UIKit gesture recognizer below egui can supply them.

A first test on an iPad with Apple Pencil on 2026-10-08 gave these results: painting and navigation are smooth, pressure works, and palm rejection works. The app gets 120 pen samples per second, and it gets no hover events. Both results match item 4. `crates/vale-globe-proto/README.md` lists all the tests.

This document sets no dates. The labeling work continues in parallel with the phases, because its quality grows with test cases and not with a deadline.

## Risks and open questions

The largest risk is the scope of the labeling engine. To contain it, the library came first, and it grows in parallel with the phases.

| Risk | Response |
| --- | --- |
| The labeling engine takes years to reach Maplex quality. | Build it first as a library. Ship strong manual control early, so that a weaker engine is still usable. |
| Vello, Parley, and Krilla are young libraries. | The display list isolates them. Skia is the fallback renderer, because it draws to screen, PDF, and SVG. |
| egui 0.36 and wgpu 30 are new, and Vello must keep the same wgpu version as eframe. | The map is a CPU-rendered texture today, so the app does not depend on that match. |
| egui limits the panels and tables. | Keep all logic in the core crates. If a real limit appears, change the shell. |
| PROJ is hard to package on Windows. | Use the bundled build and add a Windows build to CI in phase 0. |
| When you edit a linked file by hand, features lose identity. | Use element IDs first and geometry matching second, and report each lost match. |
| Painting needs a fast, steady stroke, and a slow brush makes the tool useless. | Stamp on the GPU and repaint only the tiles under the brush. Measure the stroke delay in phase 1. |
| One heightmap resolution does not fit both a world and a small region. | Store tiles in levels, so a region can hold finer tiles. This design is not done. |
| egui gives no hover, no tilt, and only 120 pen samples per second on iPad. | The iPad test of the globe prototype passed for painting, pressure, and palm rejection. Add a UIKit gesture recognizer below egui for hover, tilt, and the 240 Hz samples in phase 1. |
| Reprojected rasters look soft. | Show the resolution of each source against each map frame, and support regional sources. |

Two questions are open, and five are decided.

- [x] Decided: heightmap painting on the globe, the toolbox, and freehand lines are in version 1. A full vector drawing tool set is not.
- [x] Decided: the globe and the toolbox come before styling and labeling in the app. They are phases 1 and 3.
- [x] Decided: the shell stays egui. The globe prototype ran on an iPad with Apple Pencil, and painting, pressure, and palm rejection work. Hover and the 240 Hz pen samples need a UIKit gesture recognizer below egui.
- [x] Decided: the app and the labeler use a dual MIT and Apache-2.0 license.
- [ ] Does export need CMYK for commercial print?
- [ ] Is one project file correct, or do you want a folder that works well with version control?
- [x] Decided: the app is named Vale, and the crates have more specific names.
