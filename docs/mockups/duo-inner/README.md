# iPhone Duo inner screen

These files show the app on the inner screen of iPhone Duo, in portrait and in landscape. Each picture is real output of `vale-app`, at a pixel ratio of 2. `gallery.html` shows the pictures in pairs, with a dashed line at the hinge crease.

The screen is 1878 by 2670 pixels. At 3 pixels per point, it is 626 by 890 points in portrait and 890 by 626 points in landscape. The point size and the safe areas are estimates until a measurement on the device. Both orientations are 600 points wide or more, so the app uses the medium layout of the iPad.

## Screens

| Screen | Content |
| --- | --- |
| `start` | The start screen with two project files. |
| `globe` | World workspace with the Move tool and no pane. |
| `globe-layers` | The Layers panel in the dock. |
| `globe-dock` | Layers, Attributes, and Toolbox in the dock. |
| `globe-dock-hidden` | The dock collapsed off the right edge. |
| `globe-height` | The Raise brush on a height layer. |
| `globe-modes` | The list of the Sculpt cell. |
| `globe-stops` | The page of the color stops of the height layer. |
| `globe-panels-menu` | The Panels menu. |
| `globe-view-menu` | The View popover. |
| `globe-worlds-menu` | The menu of the worlds. |
| `globe-world-form` | The form of a new world. |
| `globe-measure` | A path of the Measure tool. |
| `flat` | The flat view. |
| `flat-layers` | The flat view with the Layers panel. |
| `atlas` | Atlas workspace with no selection. |
| `atlas-frame` | A selected map frame with Properties. |
| `atlas-labels` | The Labels panel of the frame. |
| `atlas-pages` | The Pages panel. |

Each screen has two files in `screens/`, with the suffix `-portrait` or `-landscape`.

## Render the screens

Build the app, then run the script from the root of the repository.

```sh
cargo build --release -p vale-app
docs/mockups/duo-inner/render.sh
```

The variable `VALE_APP` gives another path to the binary. A computer with no GPU renders with a software renderer, and a render can time out. If the script prints "failed" for a screen, run the script again.

`assets/height.png` is a synthetic heightmap. The screens of the height layer import it.
