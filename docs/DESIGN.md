# Vale: Design Document

Oct 8, 2026 · Zach Hannum

## Summary

Vale is a GIS for fictional worlds, for desktop, iPad, and iPhone. One world dataset is the source of truth, and each regional map is a projected, styled, labeled view of that dataset.

You draw the world in the app, on a globe. You paint a heightmap (a greyscale image where brightness is elevation) with a soft brush, and the app turns it into vector topography.

The first differentiator is an automatic labeling engine at the level of the ArcGIS Maplex engine, built clean-room as a standalone library. The second differentiator is first-class projection support for a world of any size. The third differentiator is drawing on the sphere, so you never draw on a distorted flat map.

The stack is Rust, with PROJ for projections, GeoPackage for storage, Vello for screen rendering, and Krilla for PDF export.

The app and the labeling library are free and open source, under a dual MIT and Apache-2.0 license.

## Goals and non-goals

Version 1 has ten goals.

- Run on macOS, Windows, Linux, iPad, and iPhone, with pen input on each platform but iPhone. iPad is an equal platform and not a later port. On iPhone, a finger paints.
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

- Wholesale procedural world generation. This is a permanent non-goal, because the app presents a world that you designed. The Warp mode and the Noise mode of the brush use noise under the brush only, and the Ocean ridge mode and the Range mode use a pattern from a seed near the stroke only.
- A general painting or illustration tool. The app paints one thing, the heightmap. Photoshop and Illustrator keep color artwork and textures.
- Erosion and other terrain effects. The app prototype tests live rivers, a Carve mode, an Erode mode, a Weather mode, and an Uplift mode of the brush. Those modes erode only where you paint. The prototype also tests a Ridge mode, which raises the land between the rivers, and a Deposit mode, which fills pits and valley floors. The prototype also tests a Warp mode, which moves the ground under the brush by a noise field, a Push mode, which moves the ground in the direction of the stroke, and a Noise mode, which adds fractal noise under the brush.
- Time-based data such as historical borders, and non-spherical worlds. These items come after version 1.
- Real-world GIS analysis such as routing or geocoding.
- A web version.

## Core workflow

The app has one main workflow with four steps, and every design choice in this document serves it.

1. Define the world. You set the radius, the name, and the display units.
2. Draw the world. You paint the heightmap on the globe, run the polygonize tool to get topography polygons, draw rivers and borders as lines, and place data such as cities. You can also import equirectangular vector or raster files that you made in other tools.
3. Make maps. A map shows the layers of the world in one projection. You put a map into a map frame on a page. There you select the projection, turn layers on and off, change the styles for that map, and generate labels from attributes.
4. Make the atlas. You set the scale and the extent of each frame, and you add the title, the legend, and the scale bar. A frame draws the graticule (the grid of latitude and longitude lines). You order the pages and export all of them.

The first start of the app opens on a start screen that lists the projects. A later start opens the project of the last run at its last view. If the last run ended on the start screen, the app opens there. Each project shows a small picture of its globe, the name of its world, and the date of its last change. From the start screen you make a new world, open a project, rename it, make a copy, and delete it. The app asks before it deletes a project. On the desktop, the File menu has the same actions and a list of the recent projects.

The projects are the project files in the projects folders. On iPad, the app has a local folder and an iCloud Drive folder, and the Files app shows both. On the desktop, the folder is in the data folder of the system. The desktop app also opens a project file from another folder. The name of a project is the name of its world, and a rename does not change the name of the file.

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
| import | File readers, GeoTIFF writer, georeferencing |
| terrain | Heightmap tiles, brush engine, heightmap tools |
| style | Style rules, expressions, symbol generation |
| labeler | Standalone labeling engine. It takes page-space geometry and returns placed text. |
| render | Display list and the Vello, Krilla, SVG, and raster backends |
| app | egui shell, tools, panels |

An unpublished ninth crate, `vale-labeler-demo`, holds the command-line tool for the labeling prototype. The binary of the app crate is named `vale-app`.

An unpublished tenth crate, `vale-globe-proto`, is a prototype that tests pen drawing on the globe. The section "Globe prototype" describes it.

The published crate names take a `vale-` prefix, for example `vale-labeler` and `vale-sphere`. The plain name `vale` is taken on crates.io by an unrelated validation library.

Each map frame runs the same pipeline, on screen and at export.

1. Query. The store returns the features of the visible layers that intersect the extent of the frame.
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
| World | Radius, name, display units, seed, wind, rain shadow |
| Source | Name of the imported file, extents, and the pixels of an imported image |
| Raster | Face size, unit, value range, base level, smoothing, river switch, band line switch, and a copy of the colors of the style of its layer |
| Raster tile | Raster, level, face, column, row, and the pixels of one tile |
| Image | Width and height in pixels, number of levels, and a copy of the opacity and the blend mode of the style of its layer |
| Image tile | Image, level, column, row, and the RGBA pixels of one tile as a PNG |
| Thumbnail | A small picture of the globe, for the list of projects |
| Symbol | A point symbol that the user adds: a unique name and the text of an SVG file. The symbols are in the order of addition, and they are not in the undo log. |
| Layer | Name, type (raster, image, points, lines, or polygons), visibility, attribute schema (fields with a name, a type, and a default value), link to a source, to a raster, to an image, or to an in-app table |
| Feature | Geometry in longitude and latitude, attributes, a stable ID |
| Style | Ordered rules of one layer, stored as JSON. The style of a raster layer holds its colors: the color stops with their lines, the band colors, the display, the opacity, and the blend mode. |
| Label class | Name, layer, text expression, filter, font family, weight, italic, size, case, letter spacing, color, opacity, halo color and width, placement, priority. The map holds its classes as JSON for each layer. |
| Map | Name, projection preset and center, the layers that are off in the map, style overrides, label classes |
| Page | Name, page size in millimeters, units of the page size, paper color, page template, furniture |
| Map frame | Page, map, shape on the page, center, scale, rotation, fill, neatline, border, reference grid, graticule |
| Furniture | Page, kind (text, legend, scale bar, or north arrow), rectangle on the page, link to a map frame of the page, and the properties of the kind as JSON. A legend keeps a changed name or a hidden flag for each entry, keyed by layer and rule. |
| Override | One manual change, keyed by map frame, feature ID, and label class |
| Layout grid | Switch, spacing in millimeters, color, opacity. One grid for all pages of the atlas. |
| Page template | Page size, margins, positions of the furniture |
| Atlas | Ordered list of pages |

The order of the layer records is the draw order. The first layer is at the bottom.

The shape of a map frame is a list of rings in millimeters from the top-left corner of the page. The first ring is the outer ring, and the other rings are holes. The frames and the furniture of a page are in draw order. The scale of a frame is a ratio 1:N on the standard lines of the projection. The projection uses the radius of the world, so the ratio is true for any radius. The rotation is in degrees clockwise.

QGIS opens the feature tables directly and ignores the extra tables. The extra tables have the prefix `vale_`. Each one has a row in `gpkg_contents` with the data type `vale`, and QGIS lists only the data types that it knows. That gives interchange with no export step. The topography polygons are feature tables, so QGIS reads them too. A raster layer is not a standard table, and the app exports it as an equirectangular GeoTIFF. The image is four times the face size wide and two times the face size tall. The samples are the 16-bit levels. The file gives the offset and the scale that turn a level into a value in the unit of the layer. The CRS is the sphere of the world, the same as in the feature tables.

Each feature has a UUID in addition to the integer row ID. The UUID never changes, so overrides and styles survive an edit, a replaced file, or a new projection. The UUID is a text column of the feature table.

The world is a row in `gpkg_spatial_ref_sys`: a geographic coordinate system on a sphere of the world radius. Each feature table uses it. Each attribute field is one column. Its type is text, whole number, decimal number, or true or false, and the column type is TEXT, INTEGER, DOUBLE, or BOOLEAN. The default value of a field is the DEFAULT of its column. Each value is null or a value of the type of its field. An imported field with values of more than one type is a text field.

The display units of the world are a text column of the world table, with the value `metric` or `imperial`.

The file has a format version for the extra tables, in the table `vale_meta`. The store has one migration step for each version. When the app opens an older file, it runs the later steps in one transaction. The app does not open a file with a newer version.

The key of a raster tile is the raster, the level, the face, the column, and the row. The face is a number from 0 to 5. The column and the row count tiles of 256 pixels from the corner of the face. Level 0 has the face size of the raster, and the level is always 0 in version 1. A higher level is for finer tiles in a region, which come after version 1. The level field lets those tiles arrive with no change of the file format.

The base level of a raster is the pixel of each place that has no tile. A tile holds 256 by 256 pixels. A tile at the far side of a face can go past the face, and those pixels have the base level.

The pixels of a tile are one compressed blob, and the compression loses nothing. The first byte of the blob gives the codec. Codec 1 takes the difference of each pixel from the pixel to its left. It then compresses the low bytes and the high bytes of the differences with deflate.

A save writes the tiles that changed and removes the tiles that have no pixels now. The tiles and the Raster record go to the file in one transaction. If a raster gets a new face size, the save removes its old tiles.

The tiles of an image never change after the import, and they are not in the undo log. The import writes them while it runs, and the new layer and its Image record are one undo step after that. An undo step that holds an Image record keeps its tiles. When the file opens, when the undo log is emptied, and when an undo step goes, the store removes the tiles of each image that no record holds.

Overrides belong to a map frame and not to the feature. A label that you move in one frame stays where the engine put it in each other frame, also in another frame of the same map.

Each edit is one SQLite transaction with an entry in an undo log table. Undo and redo then work across sessions, and a crash cannot leave a half-written project.

The log is three tables. A step is one edit. A step holds an image of each record and each raster tile that the edit changed, from before the edit. A record is the world, one raster, one vector layer with its features, or the atlas. An image is one compressed blob. The image is empty if the record or the tile was not there. An undo swaps each image of the step with the record or the tile in the file, in one transaction. The step then holds the images from after the edit, and a redo swaps them back. A new edit removes the redo steps. An undo does not put back the next ID of a layer or of a raster, so an ID is not used again.

The log has a size limit, which is 256 MB of images. If the log goes over the limit, the oldest undo steps go first. A step that is larger than the limit goes at once.

The project file holds all the data of the project. The app does not edit the file in place. When you open a project, the app copies the file into its private storage and edits that working copy. Each edit is on disk in the working copy when the edit ends.

The app saves with no action from you. It writes the working copy back as one whole file, in one atomic step. It does this when you close the project, when the app goes to the background, and every few minutes while there are changes. The app also has a Save command, because people expect one. The command writes the working copy back at once. A small mark shows the state of the save. The mark turns during a save, and it is a tick when the file holds each change. On Apple platforms, the write uses file coordination. A sync service such as iCloud Drive then sees only complete files.

If the app stops before it writes back, the working copy keeps each edit, and the app writes it back at the next start.

To move a project between devices, you move or sync the file. The app does not merge. If two devices change the same project, the app keeps both versions, and you pick one.

On iPad, the app keeps each project in one of two folders. The local folder is the Documents folder of the app, and the Files app shows it as "On My iPad > Vale". The iCloud folder is the Documents folder of the iCloud container of the app. The Files app and the Finder show it as "iCloud Drive > Vale". The app owns both folders, so it needs no saved permission to open them again. If the device has no iCloud account, or the build has no iCloud entitlement, the app has the local folder only. The recent list and the working copies are in the Application Support folder of the app, which the Files app does not show.

The app declares the GeoPackage file type to iPadOS, so the Files app opens a project in Vale. A project in one of the two folders opens in place. For a project in another place, the app copies the file into the local folder and opens the copy.

The versions of a project come in two ways. If two devices save the same project while they are offline, iCloud Drive keeps each version, and the start screen asks which one stays. If the project file changes while the project is open, the write-back goes to a new file beside it, with " conflict" after the name.

## World definition and projections

A world is a sphere with one radius, and all stored geometry is longitude and latitude in degrees on that sphere. In PROJ terms the world is `+proj=longlat +R=<radius>`.

A new world starts from a form with a name, a radius, and display units. The form has the radius presets Earth (6,371 km), Mars (3,389.5 km), and the Moon (1,737.4 km), and it takes a free value from 100 km to 100,000 km. The world properties show the same fields, and a change there applies at once. A change of the radius does not change the longitude and the latitude of a feature.

The display units are kilometers or miles. They apply to a distance on the ground: the brush size, the unit of a new scale bar, and the radius. A distance below one kilometer shows in meters, and a distance below one mile shows in feet. The project file holds the radius in kilometers for each choice. A value of a raster layer shows in the unit of that layer.

A projection in the app is a PROJ string plus a name. A custom projection is a preset that you edit and save, so the app needs no projection math of its own.

When you make a map, you can draw a region on the globe. The app then suggests a projection from the shape and latitude of the region, and it puts the map in a new frame on the open page, with the region in view. The map and the frame do not keep the region.

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

For the two conic projections, the app sets the standard parallels at one sixth and five sixths of the latitude range. That is a common rule that keeps scale error low across the region.

The projection editor exposes the parameters that matter: center, standard parallels, and rotation. An oblique aspect (a projection tilted so that its center line follows the region) uses the PROJ `ob_tran` wrapper.

The editor shows distortion while you edit. It draws Tissot circles (equal circles on the sphere that show local stretch after projection) and prints the scale error at the corners of the frame.

The sphere crate generates graticule lines on the sphere and sends them through the same pipeline as features. The graticule belongs to the map frame. The frame turns it on or off, and it holds its interval, its color, and its width. The interval follows the scale of the frame, or you set it by hand. The labels sit where each line meets the frame edge. The frame sets their format: degrees, minutes, and seconds, or decimal degrees. The meridians and the parallels each have a set of labels, and each set has its own switch, text, and placement. The text is the font, the style, the size, the color, the opacity, and the halo. The placement is inside or outside the edge, the offset from the edge, the orientation, and the sides that have labels. A label lies along its line or is level on the page, and it reads from left to right. An edge of a polygon frame is on the side that it faces. A label that overlaps another label, or is not on its side of the edge, does not show, so the labels at a corner do not overlap. The map labels keep clear of the edge labels inside the frame. The globe and the flat view show a graticule as a view option, and that graticule is not in an export.

A scale bar is correct only where the projection keeps scale. The app measures the scale bar at the frame center.

## Import

An import copies the file into the project, and the project stores the extents with the copy. The project then opens complete on each device.

An imported file is a layer. You add it from the Layers panel, and the panel of the layer has three ways to set extents.

- Full globe. This is the default, and it maps the file to longitude -180 to 180 and latitude -90 to 90.
- Four numbers. You type the west, east, south, and north edges.
- Two control points. You click two known places and type their coordinates.

If a full-globe file is not twice as wide as it is tall, the panel shows a warning. A wrong aspect ratio is the most common import error.

The form of a new raster layer can take its levels from an image file, for each kind of raster layer. The form maps the darkest level and the brightest level of the image to two values in the layer.

An image layer keeps the colors and the transparency of an image file. Each pixel holds red, green, blue, and alpha (the opacity of the pixel), and not a value. An example is a drawing of the plate boundaries that you make in another tool. The source is an equirectangular PNG, TIFF, or flattened PSD file, and the extents are the full globe. A PSD file gives its composite image, which has an alpha channel only if the file has no Background layer and was saved with "Maximize compatibility". Photoshop mixes the transparent parts of the composite with white, and the app removes that white. The app reads the file in strips of 256 rows and writes tiles of 256 pixels, so an image of 16,384 by 8,192 pixels loads with a small amount of memory. The file holds the image at the full size and at each half size, down to a width of 256 pixels. The globe draws the first of those levels that is at most four times the display face size wide, which is 8,192 pixels for a face of 2,048 pixels. An image layer is in the one draw order, and a pixel with an alpha of 0 shows the layers below it. The opacity and the blend mode of the layer apply on top of the alpha of each pixel. An image layer has no color stops and no value range, and you cannot paint on it. If the image has no alpha channel or is not twice as wide as it is tall, the panel shows a warning.

After you edit the file in Photoshop or Illustrator, the command Replace from file on the layer loads the file again. The layer keeps its extents, styles, and overrides, and the app draws all maps again.

Stable identity is the hard part when you replace a vector file. For SVG, the app maps each layer by name and each feature by its element ID. If an element has no ID, the app matches features by geometry, and a large edit can then break the match. The app reports each lost match after a replace.

When the app reprojects a raster, the raster loses sharpness, so resolution matters. The rule is that the source needs at least as many pixels per kilometer as the output.

A worked example shows the size of the problem. An Earth-size world at 16,384 by 8,192 pixels has 2.4 km per pixel at the equator. A 2,000 km regional map at 3,000 pixels wide needs 0.67 km per pixel. The world raster is 3.7 times too coarse for that map.

The app has two answers. A regional source with partial extents draws over the world source inside its area. For coastlines, borders, and rivers, use vector layers, because those stay sharp at any scale.

## Drawing on the globe

The world view is a globe that you rotate, in the orthographic projection. You draw on it directly. All drawing tools work in sphere coordinates, so the same tools also work in a map of any projection. A switch shows the world as a flat map, and the same tools work there.

This removes the main pain of the old workflow. You no longer paint on an equirectangular image, where shapes stretch toward the poles. You no longer paint in several projections and join the parts.

### Heightmap

A heightmap is a raster layer of values with meters as its unit. A world can have other raster layers of values, for example precipitation. Each one has the same store, brush, and preview. A new world has no raster layer.

The world does not mark one raster layer as its height. Each raster layer with the unit `m` is a height layer. Sea level, the rivers, and the brush modes that move ground work on a height layer only. A tool that needs a height takes a layer as its input. Nothing else in the app needs a height layer.

A raster layer is a layer in the one draw order, with the vector layers. It has an opacity. The globe and the flat view draw each visible raster layer in that order, and a layer shows the layers below it through itself as its opacity says. Each raster layer has a blend mode, one of the 12 W3C modes, and the default is Normal. A layer with another mode draws in an offscreen group, which the view blends with the layers below. The flat view, the globe, and the map frames use the same modes. A map frame draws each raster layer that the map of the frame shows, in the projection of the frame and clipped to the shape of the frame. It smooths the layer by the minimum feature size at the scale of the frame, where 1 pt is 1/72 inch. Below the lowest layer is a ground of one plain color. A new raster layer goes above the top raster layer.

A raster layer holds a value that changes smoothly from place to place. A layer of classes, such as a number for each biome, is not in version 1.

A raster layer is a 16-bit greyscale cube map. Each of the six faces splits into tiles, and the project stores only the tiles that you painted.

An equirectangular image is the wrong store. It spends most of its pixels near the poles, and a round brush becomes a wide ellipse there. On a cube map with equal-angle spacing, a pixel covers close to the same ground everywhere.

A raster layer has one face size, which is the number of pixels along the edge of a face. For a height layer, the size is high enough that a regional map uses the same raster layer as the world map. A layer such as precipitation or pressure needs less. You pick the face size when you add a raster layer. The app offers 1,024, 2,048, 4,096, and 8,192 pixels. The default is 8,192 for a height layer and 2,048 for each other layer. The Raster record stores the face size.

The app shows three values next to each size:

- The ground per pixel at the equator, for the radius of the world.
- The size of an equirectangular world map of equal detail, which is four times the face size by two times the face size.
- The memory of a layer that is painted all over, before compression.

These numbers are for an Earth-size world.

| Face size | World map of equal detail | Ground per pixel | Pixels across a 1,000 km region | Memory when painted all over |
| --- | --- | --- | --- | --- |
| 1,024 | 4,096 × 2,048 | 9.8 km | 100 | 13 MB |
| 2,048 | 8,192 × 4,096 | 4.9 km | 200 | 50 MB |
| 4,096 | 16,384 × 8,192 | 2.4 km | 410 | 201 MB |
| 8,192 | 32,768 × 16,384 | 1.2 km | 820 | 805 MB |

The app keeps the memory of many layers low in the way of other paint programs. The tiles that you do not paint are on disk, and the screen uses a smaller copy. The selected raster layer has its tiles at the full face size, in main memory and on the GPU. Each other raster layer has a copy with a face of 2,048 pixels at most, and the app draws that copy. Such a layer looks softer when you zoom in, until you select it. A layer with a face of 2,048 pixels or less always shows in full detail. The copies of the visible layers on the GPU use 256 MB at most, with their pyramids. The copy of each visible image layer is in the same 256 MB. The pyramid of a layer uses a third of the memory of a dense copy of the layer. If they need more, each copy gets a smaller face. A hidden layer has no copy on the GPU. When you select a raster layer, the app loads its tiles from the working copy of the project, and the copy shows until the tiles are there.

You can change the face size of a raster layer later. The app then resamples each painted tile. For a smaller size, each new pixel is the mean of the pixels that it covers. For a larger size, the app interpolates between the pixels, which adds no detail. The change is one edit, so undo brings the old tiles back.

### Brush

The brush is a circle on the sphere with a radius in kilometers and a soft edge. It has four modes: raise, lower, smooth, and flatten to a level.

A pen sets the flow from pressure. A mouse uses a fixed flow. The brush size follows the zoom unless you lock it.

One stroke is one undo step. The step holds only the tiles that the stroke changed. With a project file, the step is in the undo log of the file. With no project file, the steps are in memory, and they use 256 MB at most. If a new step goes over that limit, the oldest steps go.

### Stepped preview

While you paint, a shader shows the heightmap as stepped tints. Each color stop of the layer is a value with a line. A band goes from one stop to the next, and each band has one color. A band owns the stop at its lower edge. This is the posterize and gradient overlay of the old workflow, live. Each raster layer has one of three displays. Bands fills each band with its color. Gradient blends from the middle of one band to the middle of the next. From an end of the range to the middle of the band at that end, the color does not change. Grey is the plain greyscale. On a height layer, sea level is a fixed stop, and it is a hard edge between the two bands next to it in the Gradient display.

The color stops and the display belong to the raster layer. The Style section of the panel of the layer shows them on a bar. A tap on the bar opens a page of the Layers panel with all the controls of the stops. The page has a vertical bar at the left, with the highest value at the top and a handle at each stop. At the right of the bar, each band has a row, with the highest band at the top. A row shows the color of the band, the value of its lower edge, and the line at that edge. All rows have the same height, the bar is as tall as the rows, and a line joins the handle of the lower edge of each band to its row. Thus many stops near each other on the bar still have large rows. The preview and the polygonize tool read the same stops, so the polygons match what you saw.

The app has a library of color ramps, and each world can use them. The row "Color ramps" of the page of the color stops opens a page with the ramps. The row "Import ramp…" of the page adds a ramp from a legend image. A legend image is a row of blocks of color, from the lowest band to the highest, or a column of blocks from the bottom up. Borders, gaps, a margin, and a label above or below the blocks are not blocks. The import does not change a layer. The page lists the ramps, each with a bar of its colors. On a layer with a sea level, a ramp with no colors that are mostly blue is a land ramp, and it changes only the bands above sea level. A ramp with only such colors is a sea ramp, and it changes only the bands below sea level. Thus a land ramp and a sea ramp together color one layer. Each other ramp changes all bands. A tap on a ramp opens a card. On a layer with a sea level, the card has a choice of the bands that the ramp changes: all, the sea, or the land. It starts on the part that the colors give. If the ramp has as many colors as it has bands to change, the card applies the colors to the bands. Otherwise, the card offers two ways. "Fit" keeps the stops, and each band takes the color at its middle along the ramp. "Make" makes one band for each color, with even spacing. A ramp for all bands puts its first colors that are mostly blue below sea level. The card also reverses the order of the colors of the ramp, and it deletes the ramp. The library is a text file in the folder of the app state.

Each color stop can also have a line, with a weight in points and a color. The color is a color that you pick, or the color of the band above the stop made darker by an amount that you set. The stops at the ends of the range have no line. The card of a band has a switch for the line at its lower edge, the weight, and the color. The globe and the flat map draw the line where the value crosses the stop, in the Bands display only. The line has the same weight on the screen at each zoom and on any shape of ground. The middle of the line is on the edge of the step, and the line goes around a sharp corner with a round end. An island or a pit that is smaller than one pixel in each direction has no line and no step color. Where the lines of two stops touch, each pixel shows the line that goes farthest past it. On a height layer, a line at sea level is the coast, and a line at another stop is a contour. Thus a height layer shows the coast and the contours with no vector step. The switch "Band lines" in the Style section of each raster layer hides all the lines of that layer, for example while you paint. The project keeps the switch of each layer.

The display smooths the field of a raster layer by the scale, so the detail follows the zoom. Each raster layer has a minimum feature size in points, from 0 to 8, and the default is 2. The app turns the size into a distance on the ground at the current zoom, and it smooths the field over that distance. A size of 0 shows the full detail. The fills, the lines at the color stops, and the snap target of a color stop all come from the same smoothed field, so they always agree.

The GPU keeps a pyramid of the field. A pyramid is a stack of levels, and each texel of a level is the mean of 2 by 2 texels of the level below. The shader reads the two levels that are nearest to the distance and blends them, so the detail changes with no jump while you zoom. A read near a face edge takes the texels past the edge from the next face, so the field has no seam. A brush stroke updates the levels under its texels in the same frame. For the snap target, the CPU makes the same levels with the same integer math, only for the blocks of texels near the pen.

The heightmap does not change, and the readout and the brush use the full detail. Each raster layer has a switch that turns its smoothing on and off on the globe and in the flat view, so you can paint one layer with the full detail in view. The project keeps the switch and the size of each layer. A map, a map frame, and an export always use the smoothing.

A height layer can show a hillshade over its colors. A hillshade is a shade of the slopes, with a light from one direction. North is the north of the world at each place, so the shade stays on the ground while you turn the globe. The shade blends with the colors in the soft-light mode. Flat ground keeps its color, slopes toward the light get lighter, and slopes away from the light get darker. The hillshade comes from the same smoothed field as the fills, in each display, on the globe and in the flat view. The rivers and the lines at the stops go over it. When you zoom out, the app makes the slopes steeper, so the relief shows at each zoom.

The hillshade has its own page in the Layers panel. The Hillshade row of the Style section opens the page, and the row shows if the hillshade is on. The project keeps each value of the page:

- A switch turns the hillshade on or off. The other controls show while it is on.
- The opacity is 0 to 100 percent. The default is 50.
- A switch turns the highlights on or off. Without the highlights, only the slopes away from the light change.
- A compass dial sets the direction of the light. North is 0 degrees, and east is 90. You drag the handle, or you tap a place on the dial. Near the 8 compass points, the direction snaps to the point. The default is 315, from the northwest.
- The light height is 5 to 85 degrees above the ground. The default is 45. A lower light gives a stronger shade.
- The relief makes the slopes 0.25 to 4 times as steep. The default is 1. It multiplies the change by the zoom.
- The slopes away from the light blend toward the shade color. The highlights stay grey. The default is black.

A final map can use the stepped raster with its lines as the topography, in place of the topography polygons. Polygonize is then optional.

### Toolbox

The toolbox is a searchable list of tools, open from every workspace. A tool takes one or more layers and a few parameters, and it writes a new layer. Polygonize is one tool in the list and not a button of its own.

Version 1 has six tools.

| Tool | Input | Output |
| --- | --- | --- |
| Polygonize | A raster layer, its color stops | Polygons, one for each step |
| Contour lines | Heightmap, interval | Lines with an elevation attribute |
| Smooth | Lines or polygons | The same layer with smoother outlines |
| Simplify | Lines or polygons | The same layer with fewer points |
| Dissolve | Polygons, an attribute | Polygons joined where the attribute is equal |
| Clip | A layer, a polygon or the extent of a map frame | The part of the layer inside the shape |

The output layer keeps a record of the tool, its inputs, and its parameters. When an input changes, the layer shows that it is out of date, and you run the tool again with one action.

All tools work on the sphere. They live in the core crates, so the command line can run them too.

### Polygonize

The polygonize tool traces the value of each color stop on each cube face, joins the outlines across face edges, and smooths them. It writes one polygon layer. It can also split the output at a value into two layers, for example Topography and Bathymetry at 0 m. Each polygon of a heightmap has an `elev_min` attribute, and a normal style rule colors it.

The polygons are an output layer of the toolbox. When the heightmap changes, the layer shows that it is out of date, and you run the tool again. A map can use the polygons, so topography stays vector in PDF and SVG. It can also use the stepped raster with the lines of its stops.

### Lines

A freehand line tool draws rivers, borders, and roads on the globe. The app smooths the stroke and stores it as a normal line feature. The tool draws in the selected line layer, and each stroke is one undo step.

The path of the stroke is a curve through the pen samples, so a fast stroke has no corners. A Gaussian filter then takes away the shake of the hand. One Smoothing control sets the width of the filter, from 0 to 8 points on the screen. The default is 35 percent. A slow stroke gives a sample at each point, so its detail stays. The width is on the screen, so you zoom in to draw finer detail. The ends of the stroke stay where the pen was.

A stroke that starts within 12 points of the end of a line in the layer extends that line. The line keeps its UUID and its attributes, and the filter reads the end of the line, so the joint has no kink. Each other stroke makes a new feature.

In a free part of the stroke, the app removes each vertex that is less than 0.1 points from the simpler line. Where the line crosses the 180 degree meridian, it has two parts. One part ends at 180 and the next part starts at -180, at the same latitude. Two vertices of a part are at most 45 degrees of longitude apart, so a renderer in longitude and latitude draws each step the short way. Where a line goes over a pole, it has two vertices at the pole, with the longitude of each side. The globe draws each step as a great-circle arc.

### Terrain fit

The line tool has a terrain fit for roads. You draw a rough road between two places, and the app moves the line onto the path of least effort over the ground. You choose the ends and the route, and the app only refines your line. The fit runs when the pen lifts. The result is a normal line feature in the selected line layer, with the smoothing of the line tool, and the stroke is one undo step. A later change to the terrain does not move a road.

The fit uses the first height layer of the project. With no height layer, the switch is disabled. The search is A* (a shortest-path search) on the texels of the cube map, in a corridor around the stroke. The corridor keeps the search fast, and it keeps the road near the route that you drew. Each texel has 16 neighbors: the 8 texels around it and the 8 texels at a knight move. A neighbor across a face edge is on the next face, so a road has no gap at a face edge or at 180 degrees.

The cost of a step is its length times a slope penalty, plus a penalty for the distance from the stroke. A step with a grade above 10 percent is not allowed. If no path below the limit exists in the corridor, the search runs again without the limit. A step from land into the sea or into a river has a fixed cost, so the road crosses water at a narrow place. A step along a line of the same layer costs half as much, so a new road joins the roads that exist. After the search, a pass replaces parts of the path with straight lines that cost almost the same, so the road does not zigzag.

The Balance slider sets the balance between the sketch and the terrain. Toward the terrain end, the corridor is wider and the distance from the stroke costs less. At the sketch end, the result is the same as the line tool with the fit off. The ends of the stroke use the snap service, so a road can start at a city or at the end of another road. A stroke that starts at the end of a line in the layer extends that line. The settings are not saved in the project.

### Polygons

A freehand polygon tool draws forests, deserts, lakes, and other areas on the globe. The tool draws in the selected polygon layer. Each stroke makes one new feature, and each stroke is one undo step. The stroke has the smoothing of the line tool. It uses the snap service, so a part of the outline can trace a coast or a border.

The short arc of a great circle between the two ends of the stroke closes the ring. Each new edge gets a vertex at each degree of arc, so the flat map draws the same shape as the globe. Where the ring crosses itself, the polygon holds each area that the ring goes around. The area outside is the largest area between the parts of the ring, so the polygon has no holes. Two areas that touch at one point are two polygons of the feature.

Where the ring crosses the 180 degree meridian, the feature has a polygon on each side, with an edge along the meridian. A ring around a pole goes along the meridian to the pole and then along the pole. Each ring is closed and goes counterclockwise in longitude and latitude, and no two of its edges cross.

### Points

The point tool places cities, landmarks, and other named places in the selected point layer. A tap of the pen or a click of the mouse places one point, and a finger turns the globe. The point snaps to the targets of the snap service. A point that snaps to a vertex gets the stored position of the vertex.

The tap opens the attribute form of the point. The form has a text field for each field of the layer, with the default value of the field. A text that is not a value of the type of the field has the warning color, and its value is null. An empty field is null. A layer with no fields opens no form.

The first field has the keyboard focus. Return goes to the next field, and on the last field it closes the form. The next tap also closes the form and places the next point, so ten points take ten taps and ten names. The point and its values are one undo step. Undo, a change of the tool, and a change of the layer close the form first. A ring in the accent color marks the point of the open form.

On the iPad, the form is a card near the top of the screen. The card is not modal, so the pen can place the next point. The app does not know the height of the on-screen keyboard. It takes the top of the keyboard at 45 percent of the screen height in landscape, and at 62 percent in portrait. If the new point is under the card or under that line, the globe turns, so the point is between them.

### Measure

The Measure tool measures a path on the world. It works on the globe, in the flat view, and in the map frames of the Atlas, and it does not change a layer. A tap of the pen, a click of the mouse, or a tap of one finger adds a point to the path, and a drag turns the globe or moves the page. A point snaps to the targets of the snap service when Snap is on. In a map frame, the targets are the vector layers that the map shows, and the color stops are not targets.

Each segment is the short arc of a great circle, with a vertex at each degree of arc, as in the polygon tool. A label at the middle of each segment shows its length. The readout shows the length of an open path. A tap on the first point of a path with three or more points closes the path, and so does Return on the desktop. The readout then shows the perimeter and the area, on a sphere with the radius of the world. The area is the smaller of the two areas that the path divides the sphere into. A closed path that crosses itself shows its perimeter and no area. The lengths and the areas use the display units of the world. A tap after the path is closed starts a new path.

In the Atlas, the path shows in each map frame of the page, cut at the frame shape. A path can cross from one frame to another, because all the points are in longitude and latitude.

A measurement is temporary, and the project file and the undo log do not hold it. While Measure is the tool, undo and redo act on the path and not on the project. A new point, the close of the path, and "Clear path" are each one step. A new change empties the redo steps. A change of the tool, a change of the workspace, a change of the page, and "Clear path" clear the path. A change of the tool, the workspace, or the page also clears the undo steps of the path. A change of the view, of the layer, or of the radius does not clear it, and the values follow the radius and the units.

The math is in `vale-sphere`, and the area and length expressions use the same functions.

### Attribute schema

The panel of a vector layer lists the fields of the layer, each with its type. "Add field…" opens a page with the name, the type, and the default value of a new field. The name has the keyboard focus, so the on-screen keyboard opens. "Add", or Return in the name, adds the field. A tap on a field opens its page. There you change the name, the type, and the default value, and "Delete field" removes the field.

A field name is not blank, and it is not `fid`, `geom`, or `uuid`. No two fields of a layer have the same name, and the check ignores the difference between capital and small letters. A name that breaks these rules has the warning color, and "Add" is off. A default value that is not a value of the type also has the warning color, and "Add" is off.

A new field is null in each feature, and each new feature gets the default value. A change of the type converts each value and the default value. A value that does not convert is null. A new point layer has a `name` text field, and its points get labels from it. A field with a new name stays the label field and the priority field of the layer style. Each change of the schema is one undo step, and the project saves at once.

### Snapping and tracing

The vector tools share one snap service. A snap moves a position to the nearest target within the snap distance. A trace makes a freehand stroke follow a target. The targets are the vertices and the edges of the features in each visible vector layer. The color stops of each raster layer that the globe shows in the Bands display are targets too. A vertex wins over an edge or a color stop. Of an edge and a color stop, the nearer one wins. Later targets are the ridges and valleys.

The snap distance is in points on the screen, from 4 to 32, and the default is 12. It is the same at each zoom. The start and the end of a stroke snap to a target.

When the pen comes within the snap distance of an edge, the stroke locks on and follows the line or the ring of the target. The trace only moves forward along the target, and it can go on past a cut at the 180 degree meridian. When the pen moves more than two times the snap distance away, the stroke is free again. A trace that is shorter than 8 points becomes free stroke again, so a stroke that crosses a line does not follow it.

The traced part goes from the first to the last pen sample within a quarter of the snap distance of the target. It has a copy of each stored vertex of the target, so the two features share the border exactly. The app does not smooth the traced part. Before and after the traced part, the stroke blends from the pen to the target, so it meets the target at the angle of the pen. Each free part has its own smoothing, and its ends stay on the target.

A stroke that starts at the end of a line in the same layer extends that line, and it does not trace that line. The target shows a highlight in the accent color while a stroke snaps or traces. Before a stroke, a pen that hovers or a mouse shows the point where the stroke will start: the end of the line that it extends, or the snapped position.

The target of a color stop is the line where the field of the layer crosses the value of the stop. Each stop between the two ends of the range is a target, with a line or without one, because it is the edge of a step. The target comes from the same smoothed field at the current zoom as the steps and the lines. The app reads the field only near the pen.

A position snaps to the nearest point where the field crosses a stop. A trace walks along the line in steps of half a point, and each step goes back onto the line. The field has no seam, so the trace has no gap at a face edge or at a cube corner. The trace of a color stop has no stored vertices, and the app removes each vertex that is less than 0.1 points from the simpler line.

The line tool, the polygon tool, and the point tool have a Snap switch on the brush card. The line tool also has the Fit switch of the terrain fit. The sheet has the Snapping switch, a switch for each target type, a switch for the color stops of each raster layer, and the snap distance. The settings are not saved in the project.

The service has two parts. A position snap gives the snapped point, the feature, and the stored vertex. A tracer takes the pen samples of a stroke and gives free parts and traced parts. Point placement, vertex editing, and the polygon tool use the same two parts.

### Import

You can import an equirectangular heightmap that you painted before. The app copies it into the cube map.

### Pen and iPad

The drawing tools are designed for a pen: pressure, hover preview of the brush, and palm rejection. On desktop that is a graphics tablet. On iPad it is Apple Pencil.

On iPad, winit 0.30 gives 120 pen samples per second, no hover, and no tilt. A UIKit gesture recognizer on the view of the app reads the pen below egui. It reads the coalesced touches (the extra pen samples that iOS collects in each frame, 240 per second), the tilt, and the hover. It sends them to the canvas, and it does not take the touches from winit. The canvas reads the pen from that recognizer, and egui handles the pen only on the panels. Fingers come through egui. While the pen is down, each other touch is a palm.

The path of a stroke is a curve through the pen samples, so a fast stroke has no corners.

iPad is an equal platform. The layout for iPad is its own design, with a full-screen canvas, floating panels, and touch-size controls. It is not the desktop layout made smaller.

The app also installs on iPhone, with the same layout. In portrait, an iPhone screen is compact. The safe area of an iPhone holds the Dynamic Island and the status bar at the top, and the home indicator at the bottom. In landscape, it also holds a band at each side. The Files app shows the local folder as "On My iPhone > Vale", and the app names it "On this iPhone".

The layout has four places. The top row holds the workspace switch at the left, the view switch in the center, and the actions at the right. The left edge holds the tool strip, and below it a card with the Size slider, the Flow slider, and the Brush button. Layers and Toolbox open a panel in one shared place at the right, and Brush opens a panel next to the left edge. The bottom holds the position readout, with the save mark at its left.

These rules apply to the layout:

- Each control is at least 44 by 44 points.
- No control is in the top 24 points or in the bottom 20 points, because the system uses those bands. If the safe area of the device is larger, the layout uses the safe area.
- A panel stays open while you paint. Its close button closes it, and the button that opened it closes it.
- At a width of 1,000 points or more, the workspace switch shows all workspaces. Below 1,000 points, it is one menu button.
- Below 600 points, the view switch is in the workspace menu, and a panel is a sheet at the bottom. You can pull the sheet down to its title row. One sheet shows at a time.
- If the Brush panel and the right panel do not fit side by side, one panel shows at a time.
- The two sliders are 140 points high. In a short window they become shorter, down to 110 points, which fits an iPad that is 744 points high. In a window that is shorter than that, the card holds the Brush button alone, and the Brush panel holds the two sliders. If the tool strip and the Brush button do not fit, the tool strip scrolls.
- The tool strip has five cells: Sculpt, Terrain, Draw, Move, and Measure. The strip has the same cells for each layer selection.
- The brush modes are in two groups, and each group has a cell. The Sculpt cell holds the modes that shape the ground directly: Raise, Lower, Smooth, Flatten, Warp, Push, and Noise. The Terrain cell holds the modes that need heights in meters: Carve, Ridge, Deposit, Erode, Weather, Uplift, Ocean ridge, and Range. Each list fits next to the strip on each iPad, and each group has room for new modes.
- Each of the two cells shows the icon and the name of a mode, with a small triangle in its corner. That mode is the brush mode if it is in the group of the cell, and otherwise it is the last mode that you chose in the group. A tap chooses the brush in that mode. A hold of 0.4 seconds, or a tap on the cell while the brush is in its group, opens the list of the modes of the group next to the cell. A tap on a mode chooses it and closes the list, and a tap outside the list closes it.
- The Measure cell is always enabled. With Measure, the card below the strip holds Snap, Clear path, and the Brush button, which opens the snap settings.
- The brush paints the selected raster layer, and the selection does not change the picture or the draw order. If the world has no layer, or the selected layer is not a raster layer, or the selected raster layer is hidden, the Sculpt cell, the Terrain cell, and the card below the strip are disabled, and the tool is Move. If the selected raster layer is not a height layer, the Terrain cell is disabled.
- The Draw cell draws in the selected vector layer. With no visible vector layer selected, it is disabled and shows the Line icon. If the selected layer is a visible line layer, the Draw cell is the Line cell. The Smoothing slider of the line tool is in the place of the Size slider, and below it are the Snap button and the Fit button. The Brush panel holds the Smoothing slider, the Terrain fit switch, and the Balance slider. If the selected layer is a visible polygon layer, the Draw cell is the Polygon cell, with the same controls and no terrain fit. A selection does not change the tool, so you tap the cell to draw. If the selected layer is a visible point layer, the Draw cell is the Point cell. The card then has the Snap button and no Smoothing slider. If the tool of one vector layer kind is the tool and you select a vector layer of another kind, the tool changes to the tool of that kind.
- On a raster layer with a unit other than `m`, the modes that move ground are disabled.
- The pen paints. One finger turns the globe, and two fingers zoom it.
- A tap of two fingers is undo, and a tap of three fingers is redo. The actions in the top row have an undo button and a redo button.
- A long press on a text field opens a menu with Copy and Paste, and no keyboard. Paste replaces the whole text. On iPadOS, the system can ask for permission before a paste.
- The value field and the hex field of the card of a band have a keypad in the card, in place of the keyboard of the system. A tap on the field opens the keypad in the place of the color picker, so the card does not move. The first key replaces the whole text. Done or a second tap on the field closes the keypad.

The core crates have no UI dependency, so both platforms share them. The shell is egui on both platforms. The crate `vale-globe-proto` checked how well egui handles pen input and touch on iPad, and the section "Globe prototype" gives the results.

### iPhone layout

The iPhone layout is the compact layout of the iPad, below 600 points, with these changes:

- The tool strip is a column at the left edge, as on the iPad, with the card below it.
- The phone has no pen. With a brush tool, one finger paints, and two fingers turn and zoom the globe. With Move, one finger turns the globe.
- The Brush panel has no pressure settings.
- A sheet opens at half height. You pull it up to the full height, or down to its title row.
- In the Atlas workspace, the page controls are below the top row.
- The form of a new point is a card below the top row, above the keyboard of the system.
- In landscape, the card holds the Brush button alone, at the right of the strip. The width is 600 points or more, so the view switch is in the top row, and a panel floats at the right.

The safe area of the iPhone is larger than the bands of the iPad. In portrait, it leaves out the top 59 points and the bottom 34 points.

### Desktop layout

The desktop layout has the workspaces, switches, tools, and panels of the iPad layout. The panels are docked, and the controls are for a mouse and a keyboard. macOS is the primary desktop platform, and the app uses its materials and conventions. The app prototype does not have this layout yet.

The window has five places, from top to bottom:

- The title bar and the tool bar are one bar. At the left, it holds the window buttons, the name of the world, the name of the file with the save mark, and the workspace switch. At the center, it holds the view switch in the World workspace, and the page controls, "Add", and "Export…" in the Atlas workspace. At the right, it holds undo, redo, and the button that shows or hides the dock. These actions are the same in each workspace.
- The options bar holds the settings of the active tool. For a brush mode, these are the Size slider, the Flow slider, the Hardness slider, a pressure switch, and the button "Brush…". That button opens the Brush panel as a popover.
- The tool rail at the left edge holds the tools and the More button.
- The canvas is between the tool rail and the dock.
- The status bar holds the position readout, the scale, and the zoom.

Each workspace has a tool rail and an options bar, so the canvas keeps its place when the mode changes. The menus are in the menu bar of the system.

The dock is a column at the right edge. Its tabs keep the same order in a workspace. The World workspace has the tabs Layers and Toolbox. In the Atlas workspace, the dock holds the panel of the selection, and the tabs are the tabs of that panel. With no selection, the dock does not show. Below the tabs is the list, and below the list is the inspector. The inspector is the panel of the selected layer or tool. The inspector of a vector layer lists its fields. With no selection, it holds the properties of the world. The list and the inspector show at the same time, so a panel has no back button. In the World workspace, the attribute table of a layer is in a dock below the canvas.

The title bar, the tool rail, the dock, the popovers, and the menus are translucent, with the blur of the macOS materials. The background of the canvas is the same material with a darker tint. The globe, the flat map, and the map pages are opaque, so their colors stay true.

These rules apply to the layout:

- A row is 26 pixels high, and a tool button is 28 by 28 pixels.
- The eye of a layer always shows. A hidden layer shows a crossed eye.
- Selected tools, segments, tabs, and rows have the same tint of the accent color.
- Each tool and each action shows its name and its key in a tooltip, and the menus show the keys too.
- The keys of the World tools are R for Raise, F for Lower, S for Smooth, E for Flatten, L for Line, P for Point, V for Select, A for Vertex, and H for the hand. B opens the Brush panel. The keys of the Atlas tools are V for Select, H for the hand, and T for Label. In both workspaces, M selects Measure. While Measure is on, Return closes the path, and Esc clears the path. In a map frame of the Atlas workspace, ] and [ go to the next and the previous label, Return pins a label, and Delete hides it.
- A form, for example "New world" or "Export", is a sheet of the window. The attribute form of a new point is a window at the top of the canvas, and the canvas takes the next click.
- The start screen has a sidebar with the places and the recent worlds, and a grid of cards. The tool bar has a search field, the sort order, a switch between a grid and a list, and "New world…". The menu of a card holds "Open", "Rename…", "Duplicate", "Show in Finder", and "Delete…".
- The left mouse button and the pen paint. The middle button, or Space with the left button, turns the globe, and the wheel zooms. The brush outline follows the pointer and a hovering pen.

On Windows and Linux, the menus are a row at the top of the window, and the window has no translucency. The shortcuts use Ctrl in place of Command.

## Styling model

A style is an ordered list of rules. Each rule has a filter, a scale range, and one or more symbolizers (instructions that draw a feature). This is the QGIS rule-based model, and it lets one style serve all maps.

A filter is an expression on attributes, for example `population > 50000 and kind = 'city'`. Each symbolizer property can also be an expression, so the line width of a river can come from a `flow` attribute.

The same expression language serves filters, style properties, label text, and the attribute table. `docs/EXPRESSIONS.md` has its grammar and its functions.

A rule draws a feature when its filter matches and the scale denominator is in its range. The lower limit is in the range, and the upper limit is not. Each rule that matches draws the feature, in the order of the list. The project stores the style of each layer as JSON with a version number, together with the label settings of the layer.

Each layer has one style. You edit it in the panel of the layer in the World workspace. The globe, the flat view, and each map draw the layer with that style. A map can override single properties without a copy of the full style. A change to the style of the layer shows in each map, and each override stays. In the Layers tab of a selected frame, the panel of a layer changes the map of the frame only. An override is one property of the style, for example the width of the stroke of one rule. It names its rule by the name of the rule, so it stays on its rule when the rules move. An override of a rule that is gone does nothing.

The Style section of the panel of a vector layer lists the rules in draw order, with the first rule at the top. Each row shows the filter of its rule. "Add rule" adds a rule at the end and opens its page. The new rule has the symbolizers of the last rule, with the next default color of the layer kind. A tap on a rule also opens its page. The page has the filter, the scale range, the stroke, and the fill. The stroke has a color, a width, and a line type. The line type is solid, dashed, brush, or taper. A dashed line has a dash length and a gap length. A tapered line is narrow at its first vertex and has the full width at its last vertex, and its start width is a percentage of the full width. Only a line layer has the taper. In a PDF, a brush line is a raster image, and the other line types stay vector. The page of a point layer has the Marker choice: a circle or an SVG symbol. A circle has a size, which is its diameter in points. An SVG symbol has a Symbol row, a size, a rotation, a field for the size, a field for the rotation, and a tint. The Symbol row shows the current symbol, and a tap on it opens the symbol page. The symbol page has a tile for each symbol of the project and the "Add symbol…" button, which adds an SVG file. A tap on a tile picks the symbol and goes back to the page of the rule. If the project has no symbols, the symbol page shows a hint and the button. A tap on "SVG symbol" opens the symbol page, and the circle stays until you pick a symbol. A field makes the property an expression on that field, and the value of the slider is the fallback. A tint replaces the colors of the symbol. A new symbol has no tint. The page of a polygon layer has a Scatter switch below the fill. The switch opens the symbol page, and the scatter starts when you pick a symbol. A scatter has a Symbol row, a size, a spacing, a size variation, a rotation variation, and a tint. The spacing is never less than the size. The filter shows as a list of conditions that `and` joins, and each condition has a button that removes it. "Add condition" opens a page with a chip for each field. After you pick a field, the page has the operators and the value. A number has the keypad, a text has a chip for each common value, and a true or false field has two chips. The Expression field below the conditions holds the whole filter as text, for a filter that conditions cannot make. A blank scale limit means no limit. "Move rule up", "Move rule down", and "Delete rule" change the list. The map redraws during each drag of a color, a width, a length, a size, or an angle, and one drag is one undo step. In the desktop layout, the same controls are in the panel of the layer in the globe workspace and in the dock of the Atlas workspace. In the desktop layout, a list holds the symbols, and the "Add symbol…" button is next to it. If the project has no symbols, the list is off.

The symbolizers must give enough artistic range that the output does not look like a survey map.

| Symbolizer | Options in version 1 |
| --- | --- |
| Stroke | Solid, dashed, textured brush, taper along the line for rivers |
| Fill | Solid color, pattern, texture image, hatch lines |
| Edge effect | Coast lines repeated outward into water, inner shade along a border |
| Point symbol | SVG symbol with size and rotation from attributes |
| Scatter | Symbols spread inside a polygon, for forests and mountain ranges |
| Raster | Color ramp, smooth or stepped, opacity, blend mode, band lines, smoothing, hillshade |
| Image | Opacity, blend mode |

Textures and blend modes become raster images in PDF and SVG output. All other symbolizers stay vector.

A symbol is an SVG file that the user adds. The project file holds the SVG files, and a new project has no symbols. If a style names a symbol that the project does not have, the app draws a small dot. The size of a symbol is its longer side in points. The rotation is in degrees, clockwise from the top of the page. A symbol keeps the colors of its SVG file, and a tint replaces them. A label does not overlap a point symbol. Labels can overlap scatter symbols at a cost, so a city in a forest still gets its label.

The spacing of a scatter is the minimum distance between two symbol centers, in points. The scatter puts its symbols on a fixed grid on the ground. The grid has levels, and each level has cells of half the size of the cells of the level above it. Each cell has one candidate point at a random position, and the feature ID and the cell give the random value. A candidate becomes a symbol if no symbol of a coarser level is near it. The scale of the map selects the level. A pan does not move a symbol, and a zoom adds symbols or removes symbols but does not move one. The same polygon gives the same symbols after a reload, on the globe, on each map, and in each export.

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

The library takes its fonts through Parley and measures real glyph outlines. Collision tests use one box per glyph on curved text, so curved labels pack tightly. A line label tries positions along the whole line, and its letters follow a parallel copy of the line, so the letter spacing stays the same in a bend. Above the curve limit, the turn between two letters adds a cost, and the hard limit is 1.6 times the curve limit. A position where two letters overlap, or where a gap between letters grows or shrinks too much, is not used.

The labels use the fonts of the system. The app also ships Noto Sans, which is the default font. A label style names a font family, and the project file holds only that name. The font picker has a search field. On iPad, it is the font picker of the system. A project looks the same on two devices if both devices have its fonts. If the device has no font of that family, the labels use Noto Sans, and the panel of the layer shows a warning that names both fonts. PDF export embeds a subset of each font.

The test set is real-world data, for example Natural Earth, rendered to images and compared with approved reference images. Real data has harder label problems than most fantasy maps.

Clean-room has a strict meaning here. The design comes from published research and from the public description of Maplex behavior in Esri documentation. No one on the project reads Esri code or decompiles Esri software. The starting points are Imhof's rules for name placement (1975) and the label placement study by Christensen, Marks, and Shieber (1995).

## Maps and the atlas

The atlas has three levels: maps, pages, and map frames.

A map shows the layers of the world in one projection. It has a name, a projection, a switch for each layer, style overrides, and label classes. A map has no extent, no scale, and no page size. A map frame shows one map. The Add button makes a map frame and its map together, and a delete of the frame deletes its map. The Frame tab of a map frame has the name of its map, and you rename the map there. Each map has one projection center. For another center, you add another map frame.

The app has two workspaces, World and Atlas. In the World workspace you draw and edit the features, and its attribute table edits their attributes. The Atlas workspace holds the maps, the pages, and the map frames. You edit a map in a frame, at the scale of the page, so that the labels and the styles show as they export.

A page is one page of the atlas. It has a page size, its map frames, and its furniture (the items around the maps). The page size has presets and a free value, with units. A page has a paper color. Version 1 has text boxes, legends, scale bars, and north arrows. A page can have any number of each.

A map frame is an area on a page that shows one map. Its shape is a rectangle or a polygon. A polygon can have notches and holes, for example a cut-out for an inset. The frame holds its shape on the page, and the center, the scale, and the rotation of the map in the shape. The map draws only inside the shape, and the neatline follows the shape. The extent of the frame comes from the shape and the view. A frame also holds its fill, its neatline, its border, and its graticule. The fill is a color behind the map, and with the fill off, the paper shows through. One map can be in no frame, or in many frames on one or many pages. Thus an inset or an island chain is many frames on one page. A locator map is a map in a small frame, for example in the orthographic projection with a fill for the region of the main map.

Each scale bar, north arrow, and legend links to one frame of its page. A new item links to the largest frame. When a frame is deleted, its items link to the largest frame that stays.

A scale bar has one of six styles: alternating, double alternating, hollow, single division, stepped line, or line. It shows one or two units. The second unit is on the other side of the line and has the same zero. The bar sets its divisions, equal or doubling, the subdivisions of the first division, a division before zero, the division value, the fills, the marks, the numbers, and the unit label. The fill of a division is solid, hatch, cross-hatch, or none. The division value can follow the width of the box. Each text part of an item of furniture, for example the title of a legend or the numbers of a scale bar, has the text settings of a label class.

The screen shows what the export holds. A frame on the screen has the line widths, the sizes of the symbols, and the sizes of the text of the printed page at the zoom of the view, so a 1 pt line is 1 pt on paper and grows and shrinks with the page. The Atlas workspace shows one page, and each panel opens from the selection. With no selection, no panel shows. A tap on a frame opens the panel of the frame. A tap on the page outside the frames opens the panel of the page, with the page size, the paper color, the template, and the layout grid. A tap on an item of furniture opens the panel of the item. The page controls in the top row go to the previous page and the next page. A tap on the name of the page opens the overview of all pages, where you open, reorder, add, and delete pages. The Add button in the top row adds a map frame, text, a legend, a scale bar, or a north arrow to the page.

In the Atlas workspace, you move and resize a frame on the page, you edit the vertices of its shape, and you pan, zoom, and rotate the map in it. When you select a frame, its panel has the tabs Frame, Projection, Layers, and Labels. The Frame tab holds the map of the frame, its shape, center, scale, rotation, neatline, border, and graticule. The neatline has a switch, a color, and a width. It is a line on the edge of the shape, and it follows each notch and each hole. It belongs to the frame, so two frames of one map can have different neatlines. It is a vector line in a PDF. The border is a band around the frame, at a set distance outside the bounding box of the shape. It has a switch, a width in millimeters, a style, colors, and a stroke. It also has a distance in millimeters for each side. "Border distance" sets all four sides, and the switch "Distance by side" shows the distance of the top, the right, the bottom, and the left. A negative distance puts that side of the band inside the bounding box. A part of the shape that crosses the band covers it there. The band, its stroke, and its grid labels stop at the gap, which is a set distance around the shape. Thus a polygon frame can reach past the band at one place, as a peninsula reaches past the page margin of a printed atlas. The space between the shape and the band is not filled, so another frame can show there or cross the band. A solid band has one color. A graduated band has two colors, and the color changes at each place where a meridian or a parallel meets the middle of the band. The lines are at the graticule interval of the frame, also while the graticule is off. The stroke is a line on the inner and the outer edge of the band, with a switch, a color, and a width. On a graduated band, the stroke also goes across the band at each change of color. The reference grid follows the graticule. Each interval of the meridians along the top and the bottom of the band is a column, and each interval of the parallels along the left and the right of the band is a row. The choice "Columns" gives the columns the letters A, B, C and the rows the numbers 1, 2, 3, or the reverse. The columns count from the westmost cell of the shape, and the rows count from the northmost cell. Thus on a conic map, the top and the bottom of the band can show different columns. A label sits at the middle of the band, at the middle of the part of its interval that is beside the bounding box. An interval that is too short for its label, that is off the sphere, or that is under the shape has no label. The grid has its own switch below the border, and the row "Grid labels", which opens the Text section of the labels. The labels sit in the band, so the grid turns the border on, and the border turns the grid off. The border can be on without the grid. The band and the labels are vector items in a PDF. While the graticule is on, the tab shows its interval, its color, its width, and the row "Edge labels". The row opens the Edge labels page. The page has the label format, "DMS" or "Decimal", and the segments "Meridians" and "Parallels". Each segment has the switch "Show labels", the Text section, the Placement section, and the switches of the four edges. The interval slider shows the interval that the frame draws. A drag on the slider sets the interval by hand, and the switch "Auto interval" makes the interval follow the scale again. The Projection tab edits the projection of the map and shows the distortion in the frame. In the Layers tab, the eye of a row is the switch of that layer in the map. The arrow of a row opens the panel of the layer. The panel edits the style overrides of the map, and the style of the layer stays. You cannot add, move, or delete a rule there. "Remove overrides" removes the overrides of the layer in the map. Below the rows, the section "Style for this map" lists each layer that has overrides in the map, and a tap opens the panel of that layer. The panel of a raster layer has the opacity and the Style section of the World workspace: the display, the blend mode, the smoothing, the color stops with their pages and cards, the band lines, and the hillshade. The panel of an image layer has its opacity and its blend mode. A map can override each of them. The rivers belong to the globe, and a map frame draws no rivers. The eye in the World workspace has its own state and does not change a map switch. A new layer is on in each map. The Labels tab edits the label classes of the map. The tab lists the classes of the point layers and the line layers in priority order. The top class places its labels first. You drag a row by its handle to change the order, and the page of a class also has "Move up" and "Move down". "Add class" adds a class. The page of a class has three segments: Text, Placement, and Conflicts. The Text segment holds the name, the layer, the text expression, the filter, the font, the style, the size, the case, the letter spacing, the color, the opacity, and the halo. The style row lists the faces of the font family, for example Regular, Italic, and Bold. The case is normal, uppercase, lowercase, title case, small caps, or all small caps. Small caps use the small caps of the font. If the font has none, the labeler makes them from capitals at 70 % of the size. The letter spacing is a percentage of the size. The halo is a stroke around the text, with a switch, a color, and a width. The filter has the condition builder of the style rules, and the Expression field holds the whole filter as text. The placement choices come from the feature type of the layer. A point label goes around the point, or to one side of it. A line label goes above or below the line, above it, below it, or on it. The Placement segment holds the placement and the offset, which is the distance from the feature. A point class also has the Positions page. It lists the eight positions around the point in rank order, and the labeler tries them in that order. You drag a row by its handle to change the rank, and the switch of a row blocks or allows the position. A line class also has a repeat switch and a repeat interval, a curve limit in degrees per letter, and a smoothing of the line. The Conflicts segment holds the buffer, which is a margin around the label that other labels keep free. It also holds "Remove duplicates" with its distance, the smallest distance between two labels with the same text. "Remove duplicates" is off in a new class, and it is for a feature whose name repeats because the data is in pieces. "Move up", "Move down", and "Delete class" are at the bottom of each segment. A map can have more than one class for a layer, and each class whose filter matches a feature labels that feature. A layer with no class of its own in the map uses the label settings of the layer, and a layer with its classes deleted has no labels in the map. A change in these three tabs changes the map, so it shows in each frame of that map. You pin, move, and hide a single label in a frame, because each manual change belongs to one frame.

The Atlas workspace has the Select tool, the Move tool, and the Measure tool. With the Select tool, a drag on a frame moves the frame. A selected rectangle has eight handles that resize it. A selected polygon has a handle on each vertex and a small handle at the middle of each edge. A drag on a vertex moves it, and a drag on the middle of an edge adds a vertex. A double tap on a vertex removes it. The row "Add hole" of the Frame tab adds a hole to the polygon. The row "Delete frame" at the bottom of the Frame tab deletes the frame.

Only the selected frame pans, zooms, and rotates its map. With the Move tool, a drag in the selected frame pans the map. With the Measure tool, a drag pans the page, and two fingers and the mouse wheel pan and zoom the page. With each tool, two fingers in the selected frame pan, zoom, and rotate the map, and the mouse wheel zooms it. Elsewhere, two fingers pan and zoom the page, and the mouse wheel zooms the page. A drag that starts outside the frames pans the page. With the Move tool, a drag outside the selected frame also pans the page. A double tap outside the page fits the page to the screen again, and a page fits when it opens.

When you move or resize a frame, or drag a vertex, the frame snaps to the page edges, the center lines of the page, the margins of the page template, and the edges, the center lines, and the vertices of the other frames. A move snaps the left edge, the center, or the right edge of the box of the frame, and the same on the y axis. A resize snaps only the edges that its handle moves. A vertex snaps to a vertex of another frame first. If no vertex is near, it snaps to an edge of another frame or to the lines of the page, whichever is nearer. A vertex also lines up horizontally or vertically with each other vertex of its own frame and of the other frames, and the guide goes from that vertex to the dragged vertex. A frame that you move or resize lines up with the vertices of the other frames in the same way. The snap distance is the snap distance of the World workspace. It is in points on the screen, so it is the same at each zoom of the page. While a snap holds, a magenta guide line shows its target.

An edge of a polygon frame locks to horizontal, to vertical, or to 45 degrees, with Shift on the desktop and with a second finger held on the canvas on iPad. The lock uses the edge to the vertex before or after, whichever is nearer to the angle of the pen. The locked vertex then slides along the edge to the nearest crossing with a snap target or with a locked line of the other edge. A second finger that holds a lock does not pinch the page.

The switch "Snap to layout" turns snapping on and off. It shows only while the Select tool has a map frame selected. On iPad, it is the Snap cell in a card below the tool strip. On the desktop, it is in the tool bar. The project does not save the switch.

The layout grid is a grid of lines on all pages of the atlas, at a set spacing from the top-left corner of the page. The grid button in the top row, next to the Add button, shows and hides the grid. On the desktop, the button "Layout grid" in the tool bar does the same. The panel of the page holds the switch "Layout grid", the spacing, the color, and the opacity. While the grid shows, its lines are snap targets too. A line of the page or of a frame wins over a grid line at the same distance. The grid shows only on the screen, and an export leaves it out. The project saves the grid, and a change of the grid is one undo step.

A new frame on an empty page fills the page inside a margin of 10 mm. The panel of the page does not delete the last page.

A page template holds the page size, the margins, and the positions of the furniture. Many pages share one template, so the atlas has one look. The atlas is the ordered list of the pages.

The atlas exports to three formats.

| Format | Use | Notes |
| --- | --- | --- |
| PDF | Print and books | One file with all pages, or one file for one page. Text stays selectable, and fonts are subset and embedded. |
| SVG | More work in Illustrator or Inkscape | One file per page, with layers kept as groups. |
| PNG or TIFF | Web and virtual tabletop tools | One file per page at a set resolution, rendered in tiles. |

Export uses RGB color in version 1. CMYK output for commercial print is an open question.

A place-name index with grid references comes after version 1. The data model already supports it, because labels come from stored features.

## Editing scope

Version 1 draws the shape of the world in the app and leaves color artwork to external tools.

The app has seven editing tools in version 1.

- Heightmap painting on the globe.
- The toolbox, with polygonize and five other tools.
- Freehand lines and polygons on the globe.
- Point placement with an attribute form, for cities, landmarks, and other named places.
- An attribute table with sort, filter, and bulk edit.
- Line and polygon editing by vertex, with snapping and a smooth operation.
- Label editing on the map: pin, move, select a candidate, exclude.

Photoshop, Illustrator, and similar tools keep color artwork and textures. The command Replace from file makes that round trip cheap.

A full vector drawing tool set comes after version 1.

## Phases

The labeling prototype came first, and it is done. The work after it has eleven phases, in the order of the table. The globe comes first, because you draw the world in the app. Thus the drawing tools arrive before the map tools.

| Phase | Name | At the end of the phase |
| --- | --- | --- |
| 0 | iPad test loop and CI | Each PR arrives on the iPad through TestFlight, and CI builds all four platforms. |
| 1 | Globe and heightmap painting | You paint a heightmap on the globe with Apple Pencil or a pen tablet, in the real app. |
| 2 | Projects and storage | The world that you paint is a project file that you can close, open again, and move between devices. |
| 3 | Toolbox and freehand lines | The toolbox turns the heightmap into topography polygons, and you draw rivers and borders as lines. |
| 4 | Maps, frames, and projections | You make a map in a projection that fits its region, and you place it in frames on a page. |
| 5 | Vector data editing | You place cities and other points, edit attributes, and edit lines and polygons by vertex. |
| 6 | Styling | Rule-based styles draw all symbolizers of version 1 on screen and in PDF. |
| 7 | Labeling in the app | Labels come from attributes, polygon labels and fallbacks work, and manual changes stay. |
| 8 | Atlas and export | An atlas of pages exports as a multi-page PDF, SVG files, and raster files. |
| 9 | Import | Files from other tools come in as layers, and a replaced file keeps its extents, styles, and overrides. |
| 10 | Derived values | Styles, labels, and fields use the measures of features, the values of raster layers, and the flow of rivers. |
| 11 | iPhone | The app installs on iPhone, a finger paints the globe, and each screen has an iPhone layout. |
| 12 | Version 1 release | Version 1 is in the App Store and on the three desktop platforms. |

From phase 1 on, a phase is done only when it works on desktop and on iPad.

No phase is a command-line tool alone. Five command-line tools stay, and each one runs the same core crates as the app.

- `vale-labeler-demo` is the regression test of the labeler.
- In phase 2, the flag `--export-heightmap` of `vale-app` exports the first height layer of a project file as an equirectangular GeoTIFF.
- In phase 3, a command runs a toolbox tool on a project file.
- In phase 4, a command renders one page of a custom-radius world to PDF, with its frames clipped and a graticule.
- In phase 8, a command exports the atlas as a multi-page PDF.

The app prototype covers a thin part of phases 2, 4, 6, 8, and 9. It does not finish any of them.

### Labeling prototype

The labeling prototype differs from the design above in four ways. Each difference ends when the crate named for it exists.

1. A separate crate, `vale-labeler-demo`, holds the command-line tool for data loading, projection, and rendering. It is not published. It ends when `vale-render` and the later command-line tools take over its jobs.
2. The demo rasterizes with `vello_cpu`. It now draws through the display list of `vale-render`. This difference ends when the GPU Vello backend exists.
3. The demo has two hand-written spherical projections, Equal Earth and Lambert azimuthal equal-area. They stay in the demo as a fixed test input. The app uses PROJ through `vale-sphere`.
4. Of the five fallbacks in the labeling engine, the prototype has only the last one, removal with a reason. The other four arrive in phase 7.

The labeling prototype is done. The command `cargo run --release -p vale-labeler-demo -- --preset world` writes the labeled PNG.

### App prototype

The app prototype is a vertical slice. It makes a runnable app early. It takes a thin part of phases 2, 4, 6, 8, and 9 at once. It differs from the design above in nine ways. The command `cargo run --release -p vale-app` opens the window. The command `cargo run --release -p vale-app -- --screenshot target/app/ui.png --size 1440x900 --report` draws the full UI to a PNG without a window.

The app opens on the globe workspace. The globe view and its navigation are in `vale-app`, and the globe draws the heightmap of `vale-terrain` as stepped tints with a graticule. The style panel of a raster layer edits its display and its color stops. A brush tool paints the heightmap, and a brush panel has the fifteen modes, the radius in kilometers, the size lock, the hardness, the flow, the strength, the flatten level, the angle of the Weather mode, the erosion of the Uplift mode and the Range mode, the wind and the rain shadow of the Erode mode, the Uplift mode, and the Range mode, the scale and the seed of the Warp mode and the Noise mode, the ridged variant, the invert switch, and the relief of the Noise mode, the spacing and the seed of the Range mode, the channel width of the Carve mode and the Erode mode, the floodplain of the Carve mode, and the floor and the ceiling. The size lock holds the radius as an angle on the sphere. The GPU holds the heightmap in tiles of 256 pixels, as the CPU does. A lookup table has one entry for each tile of each face, and it takes 25 KB at faces of 8,192 pixels. A tile at the base level has no entry and takes no memory, so an empty world takes memory for the table only. A tile with memory is in a pool. The pool grows in textures of 64 tiles or more, and each new texture is as large as all textures before it. The first one takes 8 MB. Thus the pool takes at most two times the memory of its tiles, plus 8 MB. With memory in each tile, the pool takes 805 MB. The button "Import heightmap…" in the tool bar copies an equirectangular PNG or TIFF image with 16-bit depth into the heightmap. You can also drop the file on the window, or give it with `--import-heightmap`. Level 0 of the image is -6,000 m, and level 65,535 is 6,000 m. A worker thread reads the file and makes the six faces, and the status bar shows the progress. The import is one undo step. The button "Export GeoTIFF…" writes the heightmap to an equirectangular GeoTIFF file. The button "Export PNG…" writes the same image as a 16-bit grayscale PNG file with no georeference. A worker thread writes the file, and the status bar shows the progress. You can paint while the export runs, and the file holds the heightmap of the time that the export started. If the tiles that the import replaces are larger than the undo limit, the import has no undo step. If the image is not twice as wide as it is tall, the status bar shows a warning, and the app stretches the image over the globe. After an import, each tile of the heightmap holds memory, which is 805 MB at faces of 8,192 pixels. The app sends a large change to the GPU in strips of 8 MB. A switch shows the world as a flat view. The flat view has one of the ten projection presets, and it starts in the equirectangular projection. PROJ computes a mesh of the map with cells of one degree, and the GPU interpolates the place on the sphere between the corners. The flat view has its own camera, and it pans and zooms. It opens with the whole map on the canvas, and you can zoom out to a quarter of that size. At each zoom you can move each place of the map to the middle of the canvas, and the middle of the canvas does not leave the map. The brush stamps on the sphere there too, and the brush outline is the projected circle of the stamp. The workspace switch opens the Atlas workspace, with the pages and the map frames of the section "Maps and the atlas". The nine items below are about the map in a frame.

On iPad, the globe workspace has the layout of the section "Pen and iPad". The flag `--layout pad` shows that layout on the desktop. With that flag, `--panel` opens one of its panels, `--brush-modes` opens the list of the brush modes, and `--worlds-menu` opens the menu of the worlds. The view switch changes between the globe and the flat view. In the flat view, the switch has a button that opens the list of the projections. In the compact layout, the workspace menu opens that list. In the flat view, the card of the view switch also has two buttons to the right of the switch. "Recenter" makes the place at the middle of the canvas the center of the projection. "Reset" puts the center of the projection back and shows the whole map. In the compact layout, the two buttons have a card below the top row. In the iPad layout, the highlight of a control is 2 points smaller than its touch area on each side, so the highlights of two controls do not touch. A panel that is too long for its card scrolls. A drag that starts up or down on a slider of a panel scrolls the panel, and it does not move the slider. The Layers panel lists the raster layers and the vector layers in draw order, with the bottom of the drawing in the first row. A row has an eye that shows or hides the layer, and a hold and a drag move each row. A tap on a row selects the layer. If you hold a row for 0.4 seconds, the row lifts, and a drag then moves it to a new place in the order. The arrow of a row opens the panel of that layer. The panel of a vector layer has the name, the type, the number of features, the Style section, and the fields. A swipe to the left on a row shows its Delete button, and a tap on the button deletes the layer. The view switch has a button that opens the view options, with the Graticule switch. The plus button of the Layers panel opens a menu with two sections. The section "New" has the rows "Lines", "Points", "Polygons", and "Raster…", which make a layer. The section "Import" has the rows "Image…", "GeoJSON…", and "Heightmap…", which read a file. The row "Heightmap…" shows only in a world with no height layer. The row "GeoJSON…" opens the file picker for a GeoJSON file. Its coordinates are longitude and latitude in degrees, the grid of an equirectangular raster. The file gives one vector layer for each kind of geometry, and the panel of the top new layer opens. The error of the import shows below the list of the layers. The desktop Layers menu has the same two sections, with the buttons "Add raster…", "Add image…", and "Add GeoJSON…". The row "Image…" opens the file picker for a PNG, TIFF, or PSD file. The new image layer goes on top of the draw order and has the name of the file. The progress and the error of the import show below the list of the layers, and the panel of the new layer shows its warning. The panel of an image layer has the name, the size in pixels, the opacity, and the blend mode. The row "Raster…" opens the form of a new raster layer, and the flag `--raster-form` opens that form at the start. The form has the presets "Height", "Precipitation", "Temperature", "Pressure", and "Custom", and it opens on "Custom". A preset fills the name, the unit, the value range, and the face size, and you can change each of them. Only the preset "Height" changes what the app does, because its unit is `m`. The form offers the face sizes that the device can hold, and the size of `--face-size` if the flag gives another one. Next to each size, it shows the ground per pixel, the world map of equal detail, and the memory. A new layer that is not a height layer starts at the lowest value of its range. The form has the source "Empty" or "Image file". For an image file, the form reads a PNG or TIFF image and shows its darkest level and its brightest level, 0 to 255 for an 8-bit image and 0 to 65,535 for a 16-bit image. Each of the two levels has a field with its value in the layer. The defaults are the minimum and the maximum of the layer. The values of the other levels are on a straight line between the two, and a value outside the range of the layer gets the nearest end. The new layer and its levels are one undo step. The panel of a raster layer has the name, the unit of a layer that is not a height layer, the value range, the pixel size on the ground, the opacity, the export row, and the Style section. The Style section has the display, the bar of the color stops, and the river switches of a height layer. On the page of the color stops, a tap on the bar away from the handles adds a stop there. If the layer has 32 bands, the page says so. A vertical drag on a handle moves its stop, and the value of the stop shows at the left of the bar. Each other drag on the page scrolls it. The rows "Reset colors" and "Color ramps" are below the rows of the bands. The flag `--stops-page` opens the page of the selected raster layer. A tap on the row of a band, or on the handle of its lower edge, opens the card of the band at the left of the panel, next to the row. A tap on the handle at the top of the range does nothing. The card has the color of the band, the value of its lower edge, the line at that edge, and the button that removes that stop. The card of the lowest band has only the color. The card of the band above sea level shows the value of sea level, and you cannot change it. A new stop splits a band, and the two parts keep the color of the band. If you remove a stop, the band below it and the band above it join, and the joined band keeps the color of the lower band. Above a sheet, the card is above the list. A second tap on the band, or a tap outside the page, closes the card. In the desktop layout, a list of the bands is below the gradient bar, with one row for each band. A click on a row selects the band and shows the controls of its card. The row "Reset colors" puts back the default stops, after a card asks. On the page of the color ramps, the row "Import ramp…" opens the file picker for a PNG or JPEG legend image, and the text below the row shows the new ramp or the error. The flag `--import-ramp` adds a ramp and gives the selected raster layer one band for each of its colors. The readout and the brush settings show values in the unit of the selected raster layer. The strength is the same share of the value range on each layer. Only a height layer has a sea level. Each other row makes an empty layer on top, with a name such as "Lines 1", and opens its panel. On a wide screen, the menu is at the left of the panel. On a narrow screen, it is below the plus button. The Toolbox panel holds no tools. The panel of the Height layer has the row "Import heightmap…". A world with no raster layer has no Height layer, and the Layers panel then holds the import row. On iPad, the row opens the file picker of the Files app, and the app gets a copy of the file that you pick. The panel of a height layer has the import row too. The text below the row shows the progress of the export, and you can paint while the export runs. On iPad, the app then opens the file picker of the Files app, and it moves the file to the place that you pick. The progress and the warning of the import show below the row. Only the panel of the layer of an import or an export shows its result. A result with no warning goes away after six seconds, and a warning goes away when you leave the panel.

The tool bar has the buttons "New world…" and "World…", which open the form of a new world and the world properties. In the iPad layout, the workspace card has the Worlds button at its left. The button opens the menu of the worlds. The menu lists the world of each project file in the folder of the open project file, and the open world has a tick. A tap on a world saves the open world and opens that world. Below the list, the menu has the rows "New world…", "World properties", and "Save". In the compact layout, the workspace menu has the row "Worlds…" in place of the button. In the iPad layout, the form is a card near the top of the screen, so that the keyboard does not cover it. There a slider sets the radius, on a logarithmic scale from 1,000 km to 20,000 km. The slider has a mark at each preset, and a place near a mark gives the preset. A tap on the value above the slider starts an edit, and a typed radius can be outside the limits of the slider. A new world is a new project file, and the app opens it. The file of the old world stays as it is. The name of the file is the name of the world, with a number if a file has that name. The file goes to the folder of the open project file. With no project file, a new world has no file. The heightmap of a new world has the default face size of the device. If the app has two projects folders, the form of a new world has the row "Location", with the choices "iCloud Drive" and "On this iPad". The file goes to the folder of that choice, and the form keeps the last choice. A device with iCloud Drive starts with it. In the iPad layout, the row also shows with one folder, and it then has one choice. The flag `--world-form` opens the form in a screenshot.

The prototype edits a project of a plain folder in place. A project in an iCloud Drive folder has the working copy of the section "Data model and file format". The app writes the copy back when the project closes, when the app goes to the background, and when a change is two minutes old. If the app stops with a change in the copy, the next start writes the copy back. On iPad, the read and the write of the project file use file coordination. An older version of the iPad app kept its projects in the Application Support folder, and the app moves those files to the local folder at the start.

The start screen lists the project files of the projects folders. The flag `--projects-dir` sets the local folder, and the flag `--cloud-dir` gives a second folder that the app shows as the iCloud Drive folder. On macOS, the app uses the folder "iCloud Drive > Vale" as the second folder if the Mac has it. Each card shows the location of its project. A project in iCloud Drive that is not on the device has the name of its file and no picture. A tap downloads it and then opens it. While a download runs, the app reads the folders again each second. If a project has more than one version, its card shows the count, and a tap opens a dialog with one button for each version. The version of that button stays, and the other versions go away. On iPad, the system gives the iCloud folder on a worker thread, each time the app comes to the front. With `--project`, the folder is that of the project file. With no `--project` and no GeoJSON file, the window opens on the project of the last run, and the iPad app does the same. The window opens on the start screen at the first start, and after a run that ended there. It also opens there if the file of the last project is not there. The flag `--start-screen` always shows the start screen, also in a screenshot. The file `resume.txt` holds the path of the open project and its view. The file is in the projects folder, and on iPad it is in the Application Support folder. The view is the workspace, the view switch, the place and the zoom of the globe and of the flat view, the projection of the flat view, and the open page of the atlas. While the view changes, the app writes the file one time in a second at most. "New world…" on the start screen opens the form of a new world. The store reads the name and the picture of a project with no read of its features. The CPU makes the picture from the heightmap, 256 pixels wide, when the project closes and after each stroke. The picture is not an undo step. The date in the list is the UTC date of the last change of the file. In the iPad layout, the start screen is a grid of cards with the theme of that layout. A narrow screen has two columns. A tap on a card opens the world. The button at the corner of a card opens a menu with the rows "Rename…", "Duplicate", and "Delete…". The rename dialog is a card near the top of the screen, so that the keyboard does not cover it. In the desktop layout, each project has the buttons "Rename", "Duplicate", and "Delete". A rename is one undo step of the project. The name of the copy is the name of its source with " copy". The File menu of the tool bar shows if the app has a projects folder. In the iPad layout, the row "All worlds" of the menu of the worlds closes the project and shows the start screen. The File menu has the same row.

A card of that layout shows a blurred copy of the canvas below a dark fill. The globe goes into a texture of the size of the screen, and egui copies that texture to the screen. A second texture holds the canvas at a quarter of the size, and two shader passes blur it with a standard deviation of 20 points. Each card draws its part of the blurred texture through a paint callback, at 70 percent of the brightness, so that grey text has a contrast of 4.5 to 1 over a white canvas. In the Atlas workspace, the CPU draws the canvas into the second texture: the background, the page, and the textures of the frames. That copy has no raster layers of the frames. The debug controls in the Toolbox panel show the frame time, and a checkbox there turns the blur off.

The brush stamps in a wgpu shader of `vale-terrain`, one pass for each face that the stamp touches. Up to 32 raise, lower, flatten, carve, ridge, deposit, or noise stamps share one pass. A smooth stamp has its own pass, because it reads the texels around it. A weather stamp, a warp stamp, and a push stamp have their own passes for the same reason. Each read of a level goes through the lookup table, and so does a read past a tile edge or a face edge. The globe shader reads through the same table. A pass cannot read the texture that it writes. Thus the pass draws the rectangle under the brush into a render target of about that size, and a copy moves the texels into the tiles. The render target takes about 20 MB for one stamp of the largest brush, and 134 MB at most. A warp stamp or a push stamp reads the texels of each face that it touches, so it draws each of its faces before the first copy. It uses one render target for each face, and at most three. While you paint, the pool is the working copy. At the end of a stroke the app reads the changed texels back into the heightmap of `vale-terrain`, which stays the document. The CPU stamp is the reference, and the tests of `vale-terrain` compare the two. The result of one stamp differs by at most 1 of the 65,536 levels, and by at most 2 in the smooth mode and in the weather mode. Over a stroke of many raise or lower stamps, the tests allow a difference of 3. Over a stroke of warp stamps or push stamps, the tests allow a difference of 2. A debug panel shows the stroke delay, which is the time from the frame that read the pen to the end of the GPU work.

The globe draws the visible vector layers over the heightmap, in draw order, with the fill, the stroke, and the circle of the layer style. An edge between two positions is the short arc of a great circle. The app adds points to an arc until the drawn line is at most a quarter of a pixel from the arc. The edge of the globe cuts each line at the exact place where the arc goes to the far side. A polygon that crosses the edge of the globe gets a fill that follows the edge between the two crossings. The outer ring of a polygon goes around the smaller of its two sides. The order of the positions in a file has no effect. A map frame draws the same edges as straight lines in longitude and latitude.

The globe and the flat view draw the rivers that the heightmap implies, and the Rivers switch of each height layer hides its rivers. The project keeps the switch of each layer. A layer with its rivers off has no channel map on the GPU. `vale-terrain` keeps a coarse copy of the heightmap with at most 512 cells along a face. It fills each pit to its rim, and each land cell drains to a steep lower neighbor. A fixed scatter of that choice lets the streams on an even slope join. A cell is a river if at least 0.0005 steradians of land drain through it. The line of a river is a curve through its cells, with no corner at a cell. Four passes of smoothing move each cell of a river toward the cells above and below it, so a river on a slanted slope has no 45-degree zigzags. A channel map with two times as many texels along a face holds two rivers for each texel, each with its distance and its size. The first is the nearest river, for the lines. The second is the river with the deepest valley at the texel. At faces of 8,192 pixels, the channel map takes 25.2 MB on the GPU. The channel map also holds one fill level for each cell of the coarse copy, for the Deposit mode. Those levels take 3.1 MB. After each stroke and each undo, a worker thread computes the channel map again, and painting does not wait for it. The rivers are a preview, and they are not features of a layer.

When you zoom in, a window of 512 by 512 fine cells covers the region in view. The cells line up with the texels of one cube face. At faces of 8,192 pixels, one cell is 1, 2, 4, or 8 texels wide. The window has its own flow and its own channel map of 4 MB, and a river of the coarse map that enters the window keeps its size. In the window, the rivers are closer together. A small pan keeps the window, and a large pan or a zoom makes a new one on the worker thread. A River width slider below the Rivers switch sets the width of the lines.

The fifth brush mode, Carve, cuts a channel along each river under the brush. The coarse copy or the window gives only the flow: the drainage area and the smooth line of each river. The channel is cut at the full resolution of the heightmap. The width of a channel grows with the square root of its drainage area, and it does not depend on the size of a cell or on the zoom. The Channel width setting is the width of a river that drains 1,000,000 km². It goes from 0.05 km to 10 km, and it starts at 0.6 km. Thus a river that drains 3,000,000 km² has a channel about 1 km wide. A channel is at least one texel wide, so the flow of the heightmap can follow it. The bed of a channel is below the filled level of the coarse copy along the river, and that level does not go up along the flow. Thus the bed goes down along the flow, and the flow after the cut follows the channels. In this mode, the Strength slider sets the depth of the bed, and a larger river gets a deeper bed. A river with a large drainage area and a low slope also gets a flat floodplain at its filled level, on both sides of the channel. The Floodplain setting is the full width of that plain. It goes from 0 km to 100 km, and it starts at 10 km. The plain grows from no width at 100,000 km² to the full width at 1,000,000 km², and it fades away as the slope goes from 0.0002 to 0.001. Carve changes no ground farther than one texel past the channel or the plain. Where two channels meet, the ground takes the deeper cut. The distance to a river is measured on the sphere, and each face holds the rivers of the faces next to it, so a channel across a face edge has no seam. Carve reads the window where the window covers the ground, and the coarse copy on the other ground. Carve does not lower land below sea level, and it does not change the sea floor. The two settings are session settings, and they are not in the project file. In the iPad layout, they are rows of the Brush panel. One Carve stamp on the GPU differs from the CPU stamp by at most 1 level. Carve is a test of a brush that erodes, and the design of version 1 does not include it.

The sixth brush mode, Erode, uses stream power. Stream power is a rule that lowers a cell in proportion to the slope to the cell below it and to the square root of the area that drains through it. While you hold the brush, a worker thread repeats a step, and each step computes the flow again from the lowered cells. The valleys then branch up toward the ridges. A stroke works on the cells of the view or on the cells of a window of the brush. The cells of the view are the cells of the window in a near view, and the cells of the coarse copy in a far view. The window of the brush has 512 by 512 cells around the pen, and its cell is the largest power of two of texels that keeps 16 cells in the radius of the brush. If that cell is smaller than a cell of the view, the stroke works on the window of the brush. Thus a small brush cuts narrow valleys in a wide view. The water of the ground of the view comes into the window of the brush from outside, so a river keeps its size there. That ground is the ground of the view at the start of the stroke, and the worker computes its flow one time. When the brush comes within 3 radii of the side of its window, the worker makes a new window around the pen. A cell of the old window keeps its height in the new window, and the water still comes in. A stroke on the cells of the view keeps the cells that it had at its start. In a window, the erosion fades to zero at the window side.

A step moves each cell to a weighted mean of its height and the new height of the cell that takes its water. Thus a cell does not go below that cell at any strength. The strength of the brush and the time since the last step set the weight. The floor of a filled pit stays until its rim comes down to it. Erode does not treat sea level as special. The flow of a step continues below the coast to the lowest ground, so a river mouth becomes an inlet, a coast under the brush moves back, and a slope of the sea floor gets canyons. A flat sea floor has no slope, so it stays. The rivers on the screen and the Carve mode use the flow of the land alone. On the cells of the coarse copy, a river cell lowers only its channel. The channel has the width of the Carve channel and no floodplain, and it gets the drop of the river cell. The other texels of a river cell get the mean drop of the cells around it that are not rivers. Thus the slopes between the rivers go down much less than the rivers. On the cells of a window, each texel takes a smooth blend of the drops of the nine cells around it, with the weights of a quadratic B-spline. Thus no cell edge shows, and a channel that crosses the cells on a diagonal has no row of dots. The blend makes a channel that is one cell wide shallower and wider. If a cell is one texel wide, the texel takes the drop of its cell. Painting does not wait for a step, and one undo step puts back the ground from before the stroke. One Erode step on the GPU differs from the CPU step by at most 1 level. Erode is a second test of a brush that erodes, and the design of version 1 does not include it.

The Erode mode and the Uplift mode use rain. The water of a cell in the flow is its area times its rain, and the rain is a value from 0 to 1. A wind brings the rain from one side. The world has a Wind setting, which is the compass direction that the wind comes from, and a Rain shadow setting from 0 to 100 percent. A new world has a wind from the west and a rain shadow of 0 percent. The brush panel of the Erode mode and of the Uplift mode shows the two settings. In the iPad layout, the wind is a compass dial. The two settings are in the project file, and a change of them is an edit of the world with an undo step. All strokes use the one wind of the world.

To find the rain of a place, the worker follows the wind back from the place for 48 steps of 2 cells each. Then the air moves forward along that line to the place. The air starts full of water. Over the sea, the air takes back a quarter of its missing water at each step. Where the ground rises along the wind, the air loses its water: a rise of 1,500 m leaves `1 / e` of the water. Where the ground goes down, the air keeps its water. The rain of the place is the water of the air times a part that grows with the elevation, from 0.15 at sea level to 1 at 3,000 m. A scale sets the largest rain on a windward slope to 1. Thus the windward side of a range gets more rain than the lee side, and higher ground on the windward side gets more rain than lower ground. The lee side is the rain shadow. A wet slope cuts deeper, denser valleys, and the lee side stays smoother.

At a rain shadow of `s`, the rain of a cell is `1 - s + s` times the rain of the wind. At 0 percent, each cell gets a rain of 1, and the result is the same as without the rain. The worker computes the rain at the corners of squares of 8 cells, and a cell takes a blend of the 4 corners around it. A corner on a face edge is one place on the sphere, so the rain is continuous across the edge. An Erode stroke computes the rain one time, from the ground of its first step, and again for a new window. The first step of a stroke takes about 1.4 times as long as before. An Uplift stroke computes the rain of the rectangle of each step, because the stroke raises the ground. The water that comes into a window from outside has no rain.

The seventh brush mode, Weather, limits the slope angle. A stamp compares each texel under the brush with its 4 neighbors on the face. Where the difference between two texels is more than the limit, ground moves from the higher texel to the lower one. One stamp moves an eighth of the part of the difference that is above the limit. The weight of a pair of texels is the smaller of their two brush weights. No ground moves across a face edge. Each pair moves a whole number of levels, so the sum of the levels stays. The limit is the tangent of the angle times the size of a texel. Each face uses one texel size, which is a quarter of the circumference of the world divided by the face size. A texel near the middle of a face edge is smaller, so the true angle there is steeper by a factor of up to 1.4 in the tangent. The Angle slider of the brush panel goes from 0.02 to 20 degrees, and it starts at 0.5 degrees. At faces of 1,024 pixels on a world of the size of the Earth, a texel is 10 km wide, and painted land has slopes of 3 degrees or less. Weather is a test of a brush that erodes, and the design of version 1 does not include it.

The eighth brush mode, Uplift, raises the land and erodes it in the same stroke. Each stamp raises the ground as a Raise stamp does. In this mode, the Strength slider has the name Uplift, and it sets the rate of the uplift. The rise of a stamp grows with the Erosion setting, up to 2 times at 100 percent. Thus one stroke gives a range of about the height of a Raise stroke at each setting. More strokes on the same range do not add their heights, because the erosion grows with the relief. A worker thread keeps the cells of the erosion for the stroke. The size of a cell follows the brush: it is the largest power of two of texels that keeps 16 cells in the radius of the brush. Thus the valleys are narrow against the range in a view of a continent and in a near view. The cell is not larger than a cell of the coarse copy. The cells are a window of 512 by 512 cells around the pen. When the brush comes within 3 radii of the side of the window, the worker makes a new window around the pen. A cell of the old window keeps its height in the new window. For each set of new stamps, the worker raises its cells under those stamps, and then it runs 4 Erode steps there. Each step computes the flow again, so the valleys of one step collect the water of the next step, and the valleys branch. The flow is that of a rectangle of cells around the stamps of the set. The water leaves at the sides of the rectangle, and no river comes in. Thus the time of a step follows the size of the brush, and not the size of the window or the amount of land. The rate of the 4 steps together is 12 times the Erosion setting times the number of stamps in the set, for a brush radius of 16 cells. The rate grows with the square root of the number of cells in the radius, because a slope with no valley erodes less on smaller cells. Thus the erosion follows the amount of the uplift, and not the speed of the pen or of the worker. After the steps, the app adds one Weather stamp for each 4 stamps of the set. The weight of that stamp is the Erosion setting, and its limit is the angle of the Angle slider. The Erosion slider goes from 0 to 100 percent, and it starts at 50 percent. At 0 percent, the stroke makes no steps and no Weather stamps, so the result is that of the Raise mode. A cell is one texel or more, so a brush with a radius of a few texels gives a smooth hill. A pen that holds still adds no stamps, so it changes nothing. The cells of the worker do not get the changes of the Weather stamps. Painting does not wait for a step, and one undo step puts back the ground from before the stroke.

Uplift is not procedural world generation. The mode has no noise and no random numbers. It changes the ground under the brush only, and the stroke sets the line, the width, and the height of the range. The valleys come from the flow of water over the land that you painted. If each step is ready in the frame of its request, two equal strokes on equal ground give equal results. A slower worker makes other sets of stamps, so the results differ in small ways. Uplift is a test of a brush that builds relief, and the design of version 1 does not include it.

The ninth brush mode, Warp, moves the ground sideways and adds no height. Round stamps leave a coast in smooth arcs and a ridge in a straight line. A Warp stroke gives the coast bays and headlands, and it bends the line of the ridge. Each texel under the brush takes the height of the ground at an offset place. A noise function of the 3D point of the texel on the sphere gives the direction and the size of the offset. The noise is value noise: each point of a 3D lattice has a fixed vector, and the noise changes smoothly between the lattice points. The noise has one size of features. The world has a seed, which is a number in the project file that selects the noise field. A new world has the seed 0. In this mode, a Seed slider of the brush panel sets the seed of the world, from 0 to 999. A change of the seed is an edit of the world, so it has an undo step in the project file. The place and the seed fix the field, so each stamp of a stroke moves the ground along the same field. Each stamp reads the ground that the stamp before it left. Two equal strokes on equal ground with the same seed give equal results.

In this mode, the Strength slider sets the largest offset of one pass, in kilometers. It goes from 1 to 500 km, and it starts at 50 km. The mean offset is about half of that distance. The weight of the brush and the flow scale the offset. A Scale slider sets the size of the features of the noise. It goes from 5 to 2,000 km, and it starts at 200 km. The scale is not smaller than 2 texels. The offset is not larger than the distance to the brush edge minus 3 texels, so it goes to zero at the edge. Thus each texel that a stamp reads is under the brush, and each new height is between the lowest height and the highest height there. Flat ground stays flat. A brush with a radius of less than 3 texels changes nothing. The height at the offset place is a blend of the 4 texels around it. The place is a point on the sphere, and a read past a face edge blends the texels of the next face. Thus a stroke across a face edge has no seam. The blend softens sharp detail under a long stroke. An offset far below one texel changes steep ground only. One Warp stamp on the GPU differs from the CPU stamp by at most 1 level.

Warp is not wholesale procedural world generation. The stroke sets the place and the amount of the warp, and the ground outside the brush does not change. Warp is a test of a brush that breaks up round shapes, and the design of version 1 does not include it.

The tenth brush mode, Ridge, is the inverse of Carve. It raises the land under the brush by an amount that grows with the distance from the nearest river, so the crest between two valleys becomes sharp. The amount grows in a straight line from zero on a river to the full strength of the brush at the largest distance, and it stays at the full strength past that distance. The largest distance is about 4 cells of the map in use, so the crests are closer together in the window. Ridge reads the window where the window covers the ground, and the channel map on the other ground. With no channel map, each place is at the largest distance. Ridge does not change the sea floor. One Ridge stamp on the GPU differs from the CPU stamp by at most 1 level. Ridge is a test of a brush that follows the rivers, and the design of version 1 does not include it.

The eleventh brush mode, Deposit, fills pits and valley floors, as sediment does. Each cell of the map in use has a fill level. A cell in a pit has the level of the rim of the pit, which is the level where the water leaves the pit. A river cell has the level halfway from its bed to its lower bank. A bank is the highest ground at one side of the river. The search for a bank goes 1 to 3 cells from the river, and a larger river has a longer search. Thus the lower bank is the level where the water leaves the valley. A cell also takes the fill level of the cell that takes its water, if that level is higher. Thus the fill levels do not go down against the flow, and the floor of a river continues into the mouths of its streams. A texel takes a blend of the fill levels of the 4 cells around it. A Deposit stamp moves each texel below its fill level toward that level, by the part that the flow of the brush gives. The lowest ground is near the river, so it rises the most, and the floor becomes flat. Ground at or above its fill level does not change. Thus the stamp lowers no ground, and no ground goes above the rim of the pit or the lower bank of the valley. Deposit does not change the sea floor, and the Strength slider has no effect in this mode. The fill levels are from the rivers at the start of the stroke, so a second stroke fills a valley further. Deposit reads the window where the window covers the ground, and the channel map on the other ground. With no channel map, Deposit changes nothing. One Deposit stamp on the GPU differs from the CPU stamp by at most 1 level. Deposit is a test of a brush that adds sediment, and the design of version 1 does not include it.

The twelfth brush mode, Ocean ridge, lays a mid-ocean ridge along the stroke. The stroke is the axis of the ridge, where new sea floor forms. A ridge is not a sum of stamps. Each texel near the line gets an age, and its shape follows from that age and from the history of the axis. The model uses published geophysics.

The age of a texel is its distance from the axis divided by half of the spreading rate. The depth follows the age curve GDH1 of Stein and Stein (1992). The brush scales that curve, so the axis is at the Crest elevation and the ground at the brush radius keeps its level.

The axis has segments. Transform faults divide the axis into groups of segments, and small offsets divide each group. Each boundary leaves a trace on the flanks. One seeded history of the spreading direction bends all the traces of a side together, so no trace is straight. An offset that moves along the axis leaves an oblique trace or a V. The age steps at a trace, so the depth also steps there. That step is a fracture zone. A large offset leaves a deep, wide trough, and a small offset leaves a faint line. A nodal deep is at each corner of the axis and a transform fault, and the segment ends are deeper than the segment middles.

The abyssal hills are fault blocks that form at the axis and move away from it. The magma supply sets the spacing and the size of the faults, from the tables of Behn and Ito (2008). Low supply gives large faults far apart, and high supply gives small faults close together. Each fault has short pieces that step past each other, and each block has a steep face toward the axis. The supply changes along each segment and with age, so the roughness changes in bands along the axis. Sediment fills the low places more on old floor. Each fault is averaged over the area of a texel, so faults smaller than a texel show their mean roughness.

A slow ridge has an axial valley, and a fast ridge has an axial high, as Chen and Morgan (1990) describe. The walls of the valley are steps of large faults that face the axis, and their tops are the rift mountains. The floor of the valley has lava hills and a volcanic ridge with gaps.

The Crest slider goes from -6,000 to 6,000 m, and it starts at -700 m. The Spreading slider sets the full spreading rate from 10 to 150 mm/yr, and it starts at 25 mm/yr. The Magma slider moves the magma supply from -25 to +15 percent of its normal value for that rate, and it starts at 0. The Segment slider sets the mean distance between transform faults from 100 to 1,500 km, and it starts at 400 km. The Offset slider sets the largest offset from 0 to 500 km, and it starts at 100 km. An offset is at most half of the brush radius. A line below the sliders shows the age of the floor at the brush radius.

The ridge raises the ground. Only the fracture zones, the nodal deeps, and the axial valley can go below the old ground. The ground farther from the line than 1.5 brush radii does not change. The highest relief bends below the Crest elevation, and the ridge gets lower and narrower toward each end of the line.

While you paint, the app sends the new points of the line to the GPU in plans. A plan holds at most 16 new segments, the older segments near them, and one rectangle for each face. For each plan, the app writes the ground from before the stroke into the rectangles, and one pass for each face draws the ridge there. The CPU heightmap holds that ground until the end of the stroke. A plan holds at most 128 segments. If more segments are near, as in a dense scribble, the plan leaves out the farthest ones, and a part of the older ridge can go flat. The GPU result of a stroke differs from the CPU result by at most 2 levels. Two equal strokes on equal ground with the same seed give equal results, and one undo step puts back the ground from before the stroke.

Ocean ridge is not wholesale procedural world generation. The stroke sets the line, the width, and the height of the ridge. The seed sets the pattern near the stroke only.

The thirteenth brush mode, Range, makes a mountain belt from one stroke. A belt has a high core along its axis, lower foothills at its sides, and many long ridges side by side. As for Ocean ridge, a belt is not a sum of stamps. Each texel near the line gets a place on the line: the distance along the line from its start, and the signed distance from the line.

The cross-section is the sum of two curves of the distance from the line. The core is flat out to 0.15 of the brush radius and falls to zero at 0.55 of the radius. The foothills fall in a smooth curve from the line to zero at the brush radius. The core has 55 percent of the height, and the foothills have 45 percent. Thus the brush radius is the half-width of the belt. The belt gets lower and narrower over 1.2 brush radii at each end of the line.

The ridges are pieces in bands across the line. The Spacing setting is the mean distance between two bands. A piece is about 8 spacings long, and its ends are seeded. The ends of the next band are at another seeded place. Each piece is a little oblique to the line, so its crest moves half a spacing sideways along its length. At an end, one piece goes down while the next piece, half a spacing to the side, comes up. Thus the ridges end and start again beside each other, and a low saddle is between them. Each piece has a seeded height and a small seeded shift across. The ridges take 80 percent of the height at each place, and the ground between them takes 20 percent. Thus the ridges are high in the core and low in the foothills.

The Strength slider has the name Height in this mode, and it sets the height of the core. The pen pressure scales the height, and the mouse gives the full height. The Spacing slider goes from 5 to 300 km, and it starts at 30 km. The spacing is not less than 6 texels or more than half of the brush radius. The Seed slider sets the seed of the world, as in the Warp mode. The mode has no Hardness slider.

The app sends the line to the GPU in plans, as for Ocean ridge, and the belt shows while you paint. The GPU result of a stroke differs from the CPU result by at most 2 levels. While you paint, the worker of the Uplift mode erodes the belt. A request covers a part of the line that the next points do not change. The worker raises its cells under that part to the level of the belt one time, and then it runs 4 Erode steps there. The rate is the Erosion setting times the square root of the number of cells in the brush radius, divided by 4. The Erosion, Wind, and Rain shadow sliders are the same as for Uplift. At 0 percent, the stroke makes no steps. The app keeps the results of the steps, and it applies them after the last plan of the stroke. The valleys between the ridges collect the water, and the rivers run along the belt. A river leaves the belt through a saddle between two pieces. Painting does not wait for a step, and one undo step puts back the ground from before the stroke.

Range is not wholesale procedural world generation. The stroke sets the axis, the width, and the height of the belt. The seed sets the pattern near the stroke only.

The fourteenth brush mode, Push, moves the ground under the brush in the direction of the stroke, as the smudge tool of a paint program moves color. With it, you move a coast or bend a range, and you do not lower the old ground and paint the new ground. Each stamp has a motion, which is the move of the pen from the stamp before it. The first stamp of a stroke has no motion and changes nothing. Each texel under the brush takes the height of the ground behind it, at the motion times the weight of the brush. The weight is the falloff of the brush times the flow, so the pressure of a pen sets the distance. At full weight, the ground moves with the pen. The mode has no Strength slider.

Push uses the steps of Warp after the offset. The offset is not larger than the distance to the brush edge minus 3 texels, so the motion goes to zero at the edge and the edge shows no step. Each new height is between the lowest height and the highest height under the brush, so Push adds no height, and flat ground stays flat. The height at the offset place is a blend of the 4 texels around it, also past a face edge, so a stroke across a face edge has no seam. One Push stamp on the GPU differs from the CPU stamp by at most 1 level. A stroke is one undo step.

The fifteenth brush mode, Noise, adds fractal noise under the brush. Painted ground is smooth at each scale below the brush radius, and the noise gives it detail. Fractal noise is a sum of noises of several sizes. Each of these noises is an octave. An octave has features of half the size and half the weight of the octave before it. The first octave has features of the size of the Scale setting, and the smallest octave has features of 2 texels or more. The sum has 8 octaves at most. Each octave is the value noise of Warp, with a seed that comes from the seed of the world. The noise takes the 3D point of the texel on the sphere. Thus a face edge shows no seam, and a feature has the same size at each place on the globe.

Each Noise stamp adds the value of the noise to each texel, times the weight of the brush and the strength. The value goes from -1 to 1, so the noise raises some ground and lowers other ground. The Strength slider sets the largest change of one pass, as in Raise. The Scale slider sets the size of the largest features, from 5 to 2,000 km, and Warp uses the same setting. The Seed slider sets the seed of the world, as in Warp. Two equal strokes on equal ground with the same seed give equal results.

The Ridged switch selects the ridged variant of the noise. That variant folds each octave at zero: its value is 1 minus 2 times the absolute value of the noise. The fold gives sharp crests where the noise crosses zero. The mean of the ridged noise is above zero, so a ridged stroke raises the ground. While the Ridged switch is on, an Invert switch shows. It turns the ridged noise upside down, which gives sharp valleys, and a stroke then lowers the ground. The Relief slider makes the noise grow with the height of the ground above sea level, so plains stay calm. It goes from 0 to 100%, and it starts at 50%. At 100%, the noise has no effect at sea level and below, and it has its full effect at 2,000 m above sea level. At 0%, the height has no effect. The relief needs heights in meters, so the slider shows on a height layer only. On another layer, the noise has its full effect at each value.

One Noise stamp on the GPU differs from the CPU stamp by at most 1 level. In the part of the brush with full effect, each stamp of a stroke gives a texel the same change. If that change is near a half level, the GPU can round it the other way at each of those stamps, so a stroke differs by a few levels.

Noise is not wholesale procedural world generation. The stroke sets the place and the amount of the noise, and the ground outside the brush does not change. Noise is a test of a brush that adds detail, and the design of version 1 does not include it.

The brush panel has a Floor setting and a Ceiling setting. Each one has a switch and an elevation in meters. Both switches are off at the start, and the elevations start at 0 m and 1,000 m. In the iPad layout, the elevation is a slider with steps of 50 m, and it shows while its switch is on. The two limits apply to all fifteen modes. The brush changes only the ground from the floor to the ceiling, and that ground does not go below the floor or above the ceiling. Ground below the floor or above the ceiling does not change. Thus an Erode stroke with a floor of 0 m keeps the land above the sea, and an Uplift stroke with a ceiling gives a plateau. A Smooth stroke with a ceiling does not lower the peaks above it. One pair of texels of a Weather stamp moves at most a quarter of the room to each limit, and a pair with a texel outside the limits does not move. A texel is in 4 pairs at most, so no texel crosses a limit, and the sum of the levels stays. A texel within 3 levels of a limit does not move toward it. In the Erode mode and in the Uplift mode, the cells of the worker keep the limits, and so do the texels. One GPU pass has one pair of limits, so a group of stamps ends where the limits change. The two settings are not coupled. If the floor is above the ceiling, no ground moves.

1. The prototype is a vertical slice through phases 2, 4, 6, 8, and 9. It does not finish any of them.
2. The map is drawn by `vello_cpu` on screen and in PNG files. The display list of `vale-render` exists, and the GPU Vello backend does not. The same pixels come out with and without a window, so tests need no GPU. The PDF export uses Krilla, and text in the PDF stays text.
3. `vale-store` holds the project in memory, and it reads and writes the world, the vector layers, and the raster records as a GeoPackage file. One save writes all the vector layers again, and it writes only the raster tiles that changed. Each save that changes the file is one step of the undo log. A step holds a changed vector layer as a whole. In memory, a feature ID is its index in the layer. With `--project`, the app loads the project file at the start and saves half a second after a change. When a stroke or an import ends, the app saves at once, so each of them is one undo step. While you hold a control, the save waits, so one drag is one undo step. On iPad, the app saves each change that waits when it stops being active, and again when it goes to the background. A stroke that still waits for its texels at that time stays open, and it is not in the file. The row "Save" of the menu of the worlds saves each change that waits at once, and the File menu has the same row with the shortcut Ctrl+S. The save mark turns while a change waits, and for 0.9 seconds after a save. After that it is a tick. After a save that failed, it is a warning sign. In the desktop layout, the mark is next to the File menu. A layer record holds the name, the visibility, the style, and the place of the layer in the draw order, so undo and redo include them. An imported layer goes above the layers of its kind, with polygons below lines and lines below points. The globe draws one pass for each visible raster layer, between the shapes of the vector layers. The drawing starts at the top raster layer that is visible and has an opacity of 100 %, because that layer hides each layer below it. An image layer never hides the layers below it. The blend mode of a raster layer or an image layer mixes it with the raster layers and the image layers below it, up to the next vector layer. The graticule and the soft edge of the globe are above the top raster layer. Each visible height layer shows its own rivers, and they cost 25 MB of GPU memory for each such layer, outside the 256 MB of the copies. The small rivers of a close view show on the selected layer only. A stroke or an import ends, and the app saves it, before another layer becomes the selected raster layer. The app reads the tiles of a newly selected layer for 6 ms in each frame, and it reads a layer of 8,192 pixels that is painted all over in about 9 seconds. That time is an estimate. With no project file, each raster layer keeps its tiles at the full face size in main memory. A new raster layer, a removed one, a hidden one, a moved one, a change of its opacity, and a stroke are one undo step each. The step that moves a file to format version 9 empties the undo log. If the style of a raster layer has no colors, the app puts the colors of the raster record into the style when it opens the file, and it empties the undo log. The picture of the globe for the start screen blends the visible raster layers. The readout shows the value of the selected raster layer if it is visible. If not, it shows the value of the top visible raster layer, with the name of that layer. The import of a heightmap makes a height layer with the name "Height" if the world has none. A change of a raster layer waits for its save until the texels of the stroke are back from the GPU. When the app closes, a stroke that still waits for its texels does not go to the file. A raster layer of a project file keeps its face size, and `--face-size` sets only the default face size of a new layer. If the device cannot hold a face of that size, the app does not show the layer and tells you its name, and the file keeps its record and its tiles. The save runs on the UI thread.
4. `vale-import` reads GeoJSON, and it reads PNG and TIFF images as 16-bit greyscale. It copies the data, and there is no command to replace a file.
5. `vale-sphere` clips with one simple method: rotate, unwrap, and clip in a plane. A polygon that covers more than half of the sphere is not supported. A ring closes along the short longitude way. `geo` and `rstar` are not dependencies yet.
6. `vale-style` has rule-based styles, with a solid fill, the stroke types, a circle, an SVG point symbol, a scatter, and the label settings of each layer. Each map has label classes. The style editor edits the filter, the scale range, the stroke, the fill, the point symbol, and the scatter of each rule. Only the size and the rotation of a point symbol can come from a field. On the globe, the scale denominator comes from the radius of the world and the radius of the globe on the screen.
7. The map pipeline (query, clip, project, style, label, emit) lives in the library part of `vale-app`, in modules that do not use egui.
8. A map stores its projection as a PROJ string with no radius. The app reads the string back into one of ten presets with its center, standard parallels, and an oblique aspect, and it adds the radius of the world when it builds the projection. The project keeps custom projections as named PROJ strings. The Projection tab of a frame lists the presets and the custom projections, edits the parameters, and saves the projection under a name.
9. `vale-labeler-demo` stays as the regression test of the labeler. It keeps its own GeoJSON reader and its two hand-written projections, and it now draws through the display list of `vale-render`.

The prototype leaves out the following.

- Undo and redo with no project file. Then undo and redo are for brush strokes only.
- Rasters and blend modes in the PNG and PDF export, regional source extents over a world source, SVG and Shapefile import, the command Replace from file, and georeferencing with extents or control points.
- Vector layers in the flat view of the globe workspace, in the blurred copy of the globe behind a card, and in the picture of the globe for the start screen.
- The rows "From file…" and "From tool" in the menu of the new layers.
- Pattern fills, texture fills, hatch fills, and edge effects, and style properties from expressions in the style editor other than the size and the rotation of a point symbol.
- Polygon labels, label fallbacks, leader lines, and manual label changes (pin, move, exclude).
- The overview of the pages, page templates, SVG export, and tiled raster export.
- Projection suggestions and Tissot circles.
- All editing tools other than the freehand line tool, the freehand polygon tool, and point placement: the attribute table and vertex editing.
- The GPU Vello backend. The prototype draws with `vello_cpu`.
- Wholesale procedural world generation. This is a permanent non-goal.

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
| When you replace an imported vector file, features lose identity. | Use element IDs first and geometry matching second, and report each lost match. |
| Painting needs a fast, steady stroke, and a slow brush makes the tool useless. | Stamp on the GPU and repaint only the tiles under the brush. Measure the stroke delay in phase 1. |
| The translucent macOS window needs a transparent title bar, an `NSVisualEffectView` behind the window, egui panels with a transparent fill, and a wgpu surface with alpha. | Make a test window with winit, wgpu, and the `window-vibrancy` crate before the desktop layout is built. If it fails, the bars and the dock are opaque. |
| One heightmap resolution does not fit both a world and a small region. | Each raster layer has one face size, high enough for regional maps. Finer tiles for a region, in levels, come after version 1. The tile key has a level field for them. |
| egui gives no hover, no tilt, and only 120 pen samples per second on iPad. | The iPad test of the globe prototype passed for painting, pressure, and palm rejection. A UIKit gesture recognizer below egui reads hover, tilt, and the 240 Hz samples in `vale-app`. |
| Reprojected rasters look soft. | Show the resolution of each source against each map frame, and support regional sources. |

One question is open, and eight are decided.

- [x] Decided: heightmap painting on the globe, the toolbox, and freehand lines are in version 1. A full vector drawing tool set is not.
- [x] Decided: the globe and the toolbox come before styling and labeling in the app. They are phases 1 and 3.
- [x] Decided: the shell stays egui. The globe prototype ran on an iPad with Apple Pencil, and painting, pressure, and palm rejection work. Hover and the 240 Hz pen samples need a UIKit gesture recognizer below egui.
- [x] Decided: the app and the labeler use a dual MIT and Apache-2.0 license.
- [ ] Does export need CMYK for commercial print?
- [x] Decided: a project is one file and not a folder, and an import copies its data into that file. One file keeps the transactions, QGIS reads it directly, and it is one item on each platform. A folder gives only readable diffs in version control. A sync service can break a SQLite file that is open, so the app edits a working copy and writes the project back as a whole file.
- [x] Decided: the app is named Vale, and the crates have more specific names.
- [x] Decided: the world does not mark one raster layer as its height. A raster layer with the unit `m` is a height layer. Each tool takes a layer or works on the selected layer, so a mark adds only a rule for the deletion of a layer.
- [x] Decided: the tool strip keeps its five cells when the selected layer is not a raster layer and when the world has no layer. The Sculpt cell and the Terrain cell are disabled. The strip then does not move when you select another layer.
