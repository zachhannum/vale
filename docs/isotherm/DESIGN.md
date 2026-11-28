# Isotherm

Isotherm is a climate simulator for invented planets. You give it a heightmap, a star, an orbit, and an atmosphere. It computes the temperature, the winds, the rain, the ocean currents, the ice, and the climate zones, and it shows them on a globe and on a flat map. It exports the results as GeoJSON, GeoTIFF, PNG, and as layers for Vale.

Isotherm is the working name. A final name must not contain "World Climate Lab" or a name like it.

## Goals

- The feature set of World Climate Lab, as its public Steam page describes it, in a design of our own.
- One Rust workspace for Windows, macOS, Linux, iPadOS, and iOS. The iOS app is equal to the desktop app, not a reduced port.
- Vector export of each result as RFC 7946 GeoJSON, so that QGIS, Vale, and web maps read it.
- Two-way exchange with Vale: a Vale heightmap goes in, and climate layers go back to a Vale project.
- Results that do not depend on the frame rate or on the quality setting of the display.

## Non-goals

- A copy of the code, the text, the names, or the art of World Climate Lab.
- A research-grade general circulation model (a full 3D model of the atmosphere, as climate scientists use). The model is of intermediate complexity, tuned for speed and for plausible results.
- A terrain editor or procedural world generation. Vale is the tool for terrain. Isotherm imports a heightmap and its masks, and it sets the sea level.

## Sources

Isotherm is an original app. The project uses public information about other apps, but it does not copy their code, text, names, or art.

The science comes from published papers and textbooks, cited in `docs/isotherm/SCIENCE.md`. Examples are energy balance models (North 1975, Budyko 1969, Sellers 1969), moist energy balance models (Hwang and Frierson 2010), Hadley cell scaling (Held and Hou 1980), Ekman and Sverdrup ocean theory, the Köppen thresholds of Peel et al. (2007), and studies of tidally locked climates (for example Pierrehumbert 2011, Yang et al. 2013). Code from open climate models such as PlaSim, ExoPlaSim, and Isca is GPL or of unclear license. You can read their papers, but you must not copy their code.

The classes for slow rotators and tidally locked worlds are our own design, from the literature. World Climate Lab has such classes too. This document says only that the feature exists, not what its classes are.

## Platforms

| Platform | Shell | GPU | Distribution |
| --- | --- | --- | --- |
| macOS | eframe | wgpu on Metal | Notarized app, later the Mac App Store |
| Windows | eframe | wgpu on Vulkan or DX12 | Installer |
| Linux | eframe | wgpu on Vulkan | AppImage or Flatpak |
| iPadOS and iOS | eframe in a UIKit host, as in Vale | wgpu on Metal | TestFlight, then the App Store |
| Web | Not in version 1 | WebGPU | A later option, because the core crates have no platform code |

The iOS app covers iPad and iPhone from phase 1. The layout follows the breakpoints of Vale: the full layout at 1,000 points and wider, a reduced top row below 1,000 points, and bottom sheets below 600 points. An iPhone in portrait is always in the sheet layout.

## Version 1

Version 1 of Isotherm does these things.

- Inputs: a heightmap from PNG, 16-bit TIFF, GeoTIFF, or a Vale project file, with a sea mask and a depression mask. The planet settings are the radius, the gravity, the star or two stars, the orbit, the tilt, the day length, the atmosphere, and the moons.
- Outputs: the grids of the section "Outputs", with twelve monthly values and a yearly value, and the climate classes of the section "The climate model".
- Views: each output on the globe or on the flat map, with a legend, contour lines, a readout under the pointer, a month slider, and the satellite view.
- The terrain: import of the heightmap and the masks, and the sea level.
- Exports: GeoJSON, GeoPackage, GeoTIFF, PNG maps and posters, animations of a year, and a Vale project file.

## Architecture

Isotherm lives in the Vale repository, in the same Rust workspace. Its crates are in `crates/` with an `isotherm-` prefix, next to the `vale-` crates. One `Cargo.lock` gives both apps the same versions of egui, wgpu, and PROJ, and one PROJ build serves both. The core crates have no UI dependency. The app crate is the only crate that depends on egui.

```
               app (egui shell, panels, globe and map views, iOS host)

import ──> world ──> sim ──> fields ──> classify ──> vector ──> export
                      │        ^
                      └─> gpu ─┘  (wgpu compute)
```

| Crate | Role |
| --- | --- |
| `isotherm-world` | Planet record: radius, gravity, star, orbit, rotation, atmosphere, moons. Heightmap, sea mask, depression mask, and bathymetry on a cube map. |
| `isotherm-sim` | The climate model. A CPU reference version of each step, and a schedule of steps. |
| `isotherm-gpu` | The wgpu compute shaders of each step of `isotherm-sim`. |
| `isotherm-fields` | The result grids: monthly and yearly means, minimums, and maximums of each output. |
| `isotherm-classify` | Köppen-Geiger, the classes for slow rotators and tidally locked worlds, vegetation, and habitability. |
| `isotherm-vector` | Raster to vector: class polygons, contour lines, isobands, streamlines, river lines. |
| `isotherm-export` | GeoJSON, GeoTIFF, PNG maps and posters, animations, and the Vale project writer. |
| `isotherm-store` | The project file, a GeoPackage, with an undo log. |
| `isotherm-import` | Heightmaps and masks from PNG, TIFF, and GeoTIFF, and Vale project files. |
| `isotherm-app` | egui shell, globe and map views, panels, headless commands, and the iOS target. |

### What comes from Vale

Isotherm uses the Vale crates through path dependencies. A change to a shared crate and the change to each app that uses it go in one PR, so the two apps cannot drift apart.

The globe renderer is in the crate `vale-globe`. The pad theme and widgets, the iOS lifecycle, the working copy, and the resume file are in the crate `vale-shell`. The shared crates keep the `vale-` prefix. They must not depend on `vale-app` or on any `isotherm-` crate. Each app gives them its own layers and its own colors.

No `vale-` crate depends on an `isotherm-` crate. The one exception is the Vale export of `isotherm-export`, which writes a Vale project through `vale-store`, in that direction only.

These parts stay specific to Vale: the labeler, the style rules, the atlas and its export, the vector editing tools, the heightmap brushes, and the pen input. Isotherm uses the flow network of `vale-terrain`.

The table lists the parts that Isotherm uses, and the parts that it copies as a pattern.

| Vale part | Use in Isotherm |
| --- | --- |
| `vale-sphere` | Projections through PROJ on a sphere of any radius, clipping, the graticule. Used as is. |
| `vale-terrain` cube map, tiles, and `equirect.rs` | The grid of the model is a cube sphere, so the same cube map holds the terrain and the results. |
| `vale-terrain` flow network | Rivers and basins from the heightmap. |
| `vale-import` | PNG, 16-bit TIFF, and GeoTIFF read and write. |
| `vale-store` GeoPackage and undo log | The project file. The extra tables get the prefix `isotherm_`. |
| `vale-globe` | The globe renderer, navigation, and the flat view. |
| `vale-shell` pad module | The theme and the touch-size widgets. The glass cards and the panels of Vale stay in `vale-app`. |
| `vale-shell` lifecycle, working copy, and resume file | Save on background, the working copy, and the restore of the last session on iOS. Each app gives its own project file and its own view. |
| `vale-app/src/headless.rs`, `cli.rs` (copied as a pattern) | The headless flags `--screenshot`, `--export`, `--size`, and `--layout pad`. |
| `vale-app/ios` (copied as a pattern) | `project.yml` for xcodegen, `build-rust.sh`, `toolchain.cmake` for PROJ, and the app icon setup. |
| `.github/workflows/ios.yml` and `scripts/testflight` (extended) | The same workflows build both apps. Each app has its own bundle ID and its own TestFlight app. A path filter skips the build of an app that a PR does not touch. |
| `CLAUDE.md` (extended) | The shared rules stay at the top. A section for Isotherm adds its own rules. |

## The climate model

The model runs on a cube-sphere grid. A cube sphere is a cube whose six faces are pressed onto the sphere. It has no pole singularity, and its cells are close to equal in area. The default grid has 64 × 64 cells on each face for the live view, and 128 or 256 for a final run. The time step is fixed, and the step count of each year is fixed, so a slow GPU takes longer but gets the same result.

`docs/isotherm/SCIENCE.md` gives the equations of the model and the published source of each part.

The model has three tiers. Each tier is a phase. Each tier works alone, and a later tier replaces parts of an earlier tier.

### Tier 1: energy balance

- Insolation from the star, the orbit, the tilt, and the day length, for each cell and each step. A tidally locked world and a 3:2 resonance follow from the same geometry.
- A star spectrum from a black body at the temperature of the star, in a visible band and an infrared band. Snow and ice take a different albedo in each band, so a red dwarf melts ice faster.
- Binary stars as two light sources.
- A gray atmosphere: the optical depth comes from the pressure and the amounts of CO2, H2O, and CH4.
- Heat capacity of land, of a slab ocean (a mixed layer of fixed depth), and of sea ice.
- Heat transport as diffusion of moist static energy (the heat of the air plus the heat in its water vapor).
- Snow and ice with an albedo feedback.
- A lapse rate correction of the temperature with height.
- Tidal heating from the moons, as a flux from below.

At the end of tier 1, the model gives monthly temperatures and ice cover.

### Tier 2: circulation and water

- Surface pressure and winds from a diagnostic model. The Hadley cell width scales with the rotation rate, as Held and Hou describe. The Coriolis force, surface friction, and land-sea heating contrasts give the trade winds, the westerlies, and monsoons.
- A slow-rotator regime and a day-night regime for tidally locked worlds, chosen by the Rossby number (the ratio of inertia to the Coriolis force).
- Humidity carried by the winds, with evaporation from the sea and from wet soil.
- Precipitation where the air saturates, with a stronger effect where the wind climbs a slope. This gives rain shadows.
- Rain and snow split by temperature.
- A soil water bucket, and runoff to the rivers of the flow network.
- Wind-driven ocean currents from Ekman and Sverdrup theory, with stronger currents at the western edge of each basin. The currents carry heat.
- Clouds as a function of humidity and lift, with an effect on albedo and on the infrared.

At the end of tier 2, the model gives the outputs that the Köppen classes need, so the classes are first in this tier.

### Tier 3: detail

- A two-layer shallow-water model of the atmosphere on the GPU, in place of the diagnostic winds.
- A deep ocean with a few layers and an overturning circulation, as an experimental option.
- Tides from the moons and the star, over the bathymetry.
- A runaway test: the energy flow view shows whether the planet is in balance, warming, cooling, or in a runaway greenhouse.

### Correctness

Each GPU step has a CPU reference version in `isotherm-sim`. A test runs both on a small grid and compares the results within a tolerance. GPU floats differ between vendors, so the goal is equal results within a tolerance, not equal bits.

The Earth preset is the main test. It uses public data only: ETOPO 2022 for the terrain (public domain) and the Köppen-Geiger maps of Beck et al. (2023) for 1991 to 2020 (CC BY 4.0), with the monthly temperature and precipitation that the maps come from. A CI job runs the preset on a coarse grid and fails if the agreement of the land classes drops below its last value. The score counts the agreement of the five main classes and of the 30 classes. The first target is 70 % of land cells in the five main classes, and the later target is 80 %. The score of the 30 classes has no fixed target, but it must not drop below its last value. Beck et al. report that their own maps of 1991 to 2020 agree with weather stations in 92 % of the main classes and 79 % of the 30 classes.

The Köppen score alone is not sufficient. A class depends only on the monthly temperature and precipitation, so errors that cancel can give the right class, and wrong winds, currents, or clouds can still give plausible classes. Each output of the Earth preset therefore has its own check against observations, from the phase that adds the output.

| Phase | Output | Observations | Check | First target |
| --- | --- | --- | --- | --- |
| 2 | Surface air temperature over land | Climatology of Beck et al. (2023) | RMS error of the zonal mean (the mean over each band of latitude), each month | 4 K |
| 2 | Surface air temperature over the sea | ERA5 | RMS error of the zonal mean, each month | 4 K |
| 2 | Surface air temperature over land | Climatology of Beck et al. (2023) | RMS error of the seasonal range (warmest month minus coldest month) | 5 K |
| 2 | Surface air temperature over the sea | ERA5 | RMS error of the seasonal range | 5 K |
| 2 | Sea ice | ERA5 | Error of the extent of each hemisphere, in the month of the largest extent and in the month of the smallest extent | 30 % |
| 3 | Precipitation over land | Climatology of Beck et al. (2023) | RMS error of the zonal mean, each month | 1.5 mm per day |
| 3 | Precipitation over the sea | GPCP | RMS error of the zonal mean, each month | 1.5 mm per day |
| 3 | Precipitation | Climatology of Beck et al. (2023) over land, GPCP over the sea | Latitude of the zonal-mean peak near the equator, in January and July | 5° |
| 3 | Precipitation over land | Climatology of Beck et al. (2023) | Pattern correlation of the yearly mean | 0.6 |
| 3 | Precipitation over land | Climatology of Beck et al. (2023) | Share of cells where more than 70 % of the yearly precipitation falls in the same half of the year as in the observations, or in neither half in both | 70 % |
| 3 | Sea-level pressure | ERA5 | Latitude of the subtropical highs in the zonal mean | 5° |
| 3 | Surface wind | ERA5 | Latitude where the zonal-mean east-west wind changes sign, between the trade winds and the westerlies | 5° |
| 3 | Surface wind | ERA5 | Pattern correlation of the vector wind, each month | 0.7 |
| 3 | Cloud cover | ERA5 | RMS error of the zonal mean of the yearly cloud fraction | 0.15 |
| 3 | Sea surface temperature | ERA5 | RMS error of the yearly mean | 2.5 K |
| 3 | Ocean currents | Drifter climatology of Laurindo, Mariano, and Lumpkin (2017) | Pattern correlation of the yearly mean surface current | 0.5 |
| 3 | Ocean currents | Drifter climatology of Laurindo, Mariano, and Lumpkin (2017) | Ratio of the mean speed of the western boundary currents to the mean speed of the interior of each basin, in the model and in the observations | Within a factor of 2 |

The checks and the Köppen score use the same reference, so a model cannot pass one and fail the other for a reason in the data:

- The observations are the means of 1991 to 2020, the period of the Köppen maps.
- Over land, the checks of temperature and precipitation use the climatology of Beck et al., which is the input of their maps. ERA5 and GPCP give only the values over the sea and the outputs that the climatology does not have.
- `isotherm-classify` uses the thresholds of Beck et al. exactly, including the 0 °C boundary between C and D and their rule for summer and winter.
- The script averages the 0.5° climatology of Beck et al. onto the cells of the CI grid, by area. The reference classes are the classes that `isotherm-classify` gives for that averaged climatology. The score compares the classes of the model with these reference classes, cell by cell. It also prints the agreement with the map of Beck et al. resampled to the same grid by majority, for comparison.
- A test compares the reference classes with that resampled map. A low agreement means a fault in the classifier or in the averaging. The task of the classifier records the value.
- The score and the checks over land count only the cells that are land in the preset and in the maps, without Antarctica, and they weight each cell by its area.
- The temperature of the climatology on a coarse cell is a mean over its heights. The model compares its temperature at the mean height of the same cell.

ERA5 is under CC BY, and GPCP is under CC0. A script downloads each data set and reduces it to reference values on the coarse grid of the CI job. The repository holds the reference values and their attribution, not the data sets. Before the reference values of a data set go into the repository, its task makes sure that the license allows it.

The CI job of the Köppen score runs these checks too. A check fails if its result gets worse than its last value, or if it misses its first target. The first targets are a start. A later phase can make a target stricter, but not looser, and phase 7 makes each target stricter. `docs/isotherm/SCIENCE.md` gives the citation of each data set.

## Outputs

Each output is a grid of the cube map, with twelve monthly values and a yearly value.

| Group | Outputs |
| --- | --- |
| Surface | Topography, rivers and lakes, ice and snow cover |
| Climate | Temperature, rain, snow, aridity, climate class |
| Atmosphere | Pressure, wind, cloud, humidity, dew point, wet-bulb temperature, energy budget |
| Oceans | Surface currents, deep currents (tier 3), current paths, sea surface temperature, tides (tier 3) |
| Life | Vegetation cover, soil water, habitability |
| Satellite | A true-color view with clouds, a night side, and plants tinted by the light of the star |

The view shows any output on the globe or on the flat map, with a legend, contour lines, a readout under the pointer, and a month slider.

## GeoJSON export

The export writes RFC 7946 GeoJSON. The coordinates are longitude and latitude on the sphere of the planet. RFC 7946 assumes the WGS 84 ellipsoid, so each file also has a foreign member `"isotherm": {"radius_km": …}` with the radius. A GeoPackage export with a sphere CRS of the planet radius, as in Vale, carries the radius in a standard way.

| Output kind | Geometry | Properties |
| --- | --- | --- |
| Class map (Köppen, other classes, vegetation) | Polygon and MultiPolygon | `code`, `name`, `system`, `group`, `color`, `area_km2` |
| Continuous field (temperature, rain, pressure) | LineString contours, or Polygon isobands | `variable`, `unit`, `month`, `value` or `min` and `max` |
| Wind and currents | LineString streamlines | `variable`, `month`, `mean_speed`, `max_speed` |
| Rivers | LineString | `strahler_order`, `discharge` |
| Coastline, lakes, sea ice extent | Polygon | `kind`, `month`, `area_km2` |
| Points (stations) | Point | All variables of the cell, by month |

The class map export merges small zones, traces the boundaries that two zones share, smooths and simplifies them without crossings, and splits polygons at the 180° meridian. Contours and isobands use marching squares on each cube face, joined across the face edges. Each export has options for the month, the minimum area, the smoothing, and the tolerance.

An export to Vale writes the class maps and the rivers as vector layers and the fields as raster layers into a Vale project file. Vale then labels and styles them for an atlas.

## The app

The UI follows the rules of Vale: iPad first, touch-size controls, the theme of `ui/pad`, and the desktop layout from the same panels.

| Workspace | Content |
| --- | --- |
| Worlds | The start screen, with a picture of each world, as in Vale |
| Terrain | Import of the heightmap and the masks, and the sea level slider |
| Planet | Star, orbit, rotation, atmosphere, and moons, with presets that we design |
| Simulate | Run, pause, speed, the year counter, the energy flow chart, and the quality setting |
| Explore | The output layers, the month slider, contours, the readout, and the habitability profile |
| Export | GeoJSON, GeoPackage, GeoTIFF, PNG maps and posters, animations, and the Vale project |

On iOS, a run stops when the app goes to the background, and the state of the model is saved to the project file. The run continues when the app comes back. The quality setting picks a coarser grid on devices with less memory. The app reads the GPU limits of the device at the start and does not offer a grid that does not fit.

Headless commands run the same crates as the app, as in Vale:

```sh
cargo run --release -p isotherm-app -- --project world.gpkg --run-years 30 --export-geojson koppen zones.geojson
cargo run --release -p isotherm-app -- --preset earth --run-years 30 --score-koppen
cargo run --release -p isotherm-app -- --layout pad --screenshot target/app/pad.png --size 1194x834
```

## Phases

Each phase has a "done when" list in its GitHub issue, as in Vale. From phase 1 on, a phase is done only when it works on desktop, on iPad, and on iPhone.

| Phase | Name | At the end of the phase |
| --- | --- | --- |
| 0 | Specification and skeleton | `docs/isotherm/DESIGN.md` and `docs/isotherm/SCIENCE.md` exist. `vale-globe` and `vale-shell` exist, and Vale builds and passes its tests with them. Isotherm shows a sample world on the globe on four platforms in CI, and each PR arrives on an iPad and an iPhone through TestFlight. |
| 1 | World and globe | You import a heightmap, set the sea level, and see the world on the globe and the flat map, on desktop, iPad, and iPhone. The project file saves and opens. |
| 2 | Energy balance | Tier 1 runs on the GPU with a CPU reference. You set the star, the orbit, the tilt, and the day length, and you see the temperature and the ice, month by month. The temperature and the sea ice of the Earth preset pass their checks against observations in CI. |
| 3 | Circulation and water | Tier 2 runs. Winds, rain, snow, currents, and clouds show. The Köppen classes show, and the Earth preset reaches 70 % agreement in CI. The precipitation, the pressure, the winds, the clouds, the sea surface temperature, and the currents of the Earth preset pass their checks against observations in CI. |
| 4 | Vector export | GeoJSON and GeoPackage export of all output kinds, from the app and from the command line. The Vale project export works, and Vale opens the file. |
| 5 | Other worlds | Classes for slow rotators and tidally locked worlds, binary stars, moons and tidal heating, thick and thin atmospheres, and the habitability profile. |
| 6 | Presentation | The satellite view, posters with a title and a legend, and animations of a year. |
| 7 | Tier 3 | Shallow-water winds, the deep ocean option, tides, and the runaway test. The Earth preset reaches 80 %, and each check against observations has a stricter target. |
| 8 | Version 1 release | The App Store, a notarized macOS build, and Windows and Linux builds. |

Phase 4 comes before phase 5 on purpose. The GeoJSON export then exists early, and the export of each later output is a small step.

## Changes to the Vale repository

The Vale repository holds both apps. These rules of the repository apply to Isotherm.

- `README.md` and `CLAUDE.md` say that the repository holds two apps.
- The rule "give each crate a `vale-` prefix" allows the `isotherm-` prefix for the crates of Isotherm.
- The documents of Isotherm are in `docs/isotherm/`: `DESIGN.md`, which is this document, and `SCIENCE.md`.
- The documents can use public information about other apps. They forbid only copying the code, the text, the names, and the art of other apps.
- The GitHub issues of Isotherm have the label `isotherm`, and Isotherm has its own epic issue.
- The license is `MIT OR Apache-2.0` for all crates.

## Risks

| Risk | Answer |
| --- | --- |
| The model is too slow on an iPhone. | The grid scales from 32 to 256 cells per face. The live view uses a coarse grid, and a final run can take longer. Tier 1 and tier 2 are 2D, and they fit a phone GPU. |
| iOS memory limits. | Keep each grid in f16 on the GPU where the precision allows. Keep only the monthly means, not each step. Measure on the oldest supported device in phase 2. |
| The results are not plausible. | The checks of the Earth preset against observations in CI from phase 2, the Köppen score from phase 3, and a gallery of reference worlds (a desert world, an ocean world, a tidally locked world) with screenshot tests. |
| The GPU results differ by vendor. | The CPU reference and the tolerance tests. The class maps compare by agreement, not by exact pixels. |
| A slower CI for both apps. | Path filters, and the existing rule to test only the crates that a change touches. |
| egui on a small phone screen. | Vale already has the sheet layout below 600 points. Test the iPhone layout with `--layout pad --size 390x844` screenshots from phase 1. |
| A name or a look that is too close to the original. | Before the release, search the App Store for a conflict with the name. |

## Decisions

1. Isotherm is the working name.
2. Isotherm lives in the Vale repository and workspace. The shared parts become `vale-` crates.
3. iPhone support starts in phase 1.
4. The desktop builds do not go to Steam.
