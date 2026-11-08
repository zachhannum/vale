# iPad layout mockups

These files show the iPad layout with the top bar, the tool column, and the dock. The screens are 1194 by 834 points, the size of an 11-inch iPad that is horizontal, at a pixel ratio of 2.

`layout.html` draws the chrome in HTML over pictures from the app. The canvas pictures are real output of `vale-app`, and the chrome uses the colors of `ui/pad/theme.rs` and the icon paths of `ui/pad/icons.rs`. The panel rows, the attribute values, and the layer "Height" are sample content.

The screens leave out the top band of the system. In the app, the bar is below that band.

## Screens

The part of the URL after `#` chooses the screen. The file in `screens/` has the same name.

| Screen | Content |
| --- | --- |
| `globe` | World workspace with the Move tool, the dock closed, and no panel. |
| `globe-dock` | The dock with Layers, Attributes, and Toolbox folded. A place is selected on the globe. |
| `globe-float` | The dock closed, with Layers as a floating card. |
| `globe-brush` | The Raise brush, with the Size slider and the Flow slider in the tool column, and Layers as a floating card. |
| `globe-modes` | The list of the Sculpt cell, open next to the cell. |
| `atlas-clean` | Atlas workspace with the dock closed and no selection. |
| `atlas-dock` | A selected map frame, with the panel of the frame, Pages, and Maps folded in the dock. |
| `atlas-clean-float` | The dock closed, with the panel of the frame as a floating card. |
| `atlas-clean-add` | The list of the Furniture cell. |

## Render the screens

This command renders one screen with Chromium. Run it from this folder, and replace the screen name.

```sh
chromium --headless --no-sandbox --allow-file-access-from-files --hide-scrollbars \
  --force-device-scale-factor=2 --window-size=1194,834 --virtual-time-budget=2000 \
  --screenshot="$PWD/screens/globe-dock.png" "file://$PWD/layout.html#globe-dock"
```

The app draws its UI text in the Ubuntu Light font of egui. If the computer does not have that font, the page uses Noto Sans from `assets/fonts`.

## Assets

`assets/globe.png` is the globe with no panels, at a pixel ratio of 3. This command makes it. The page uses only the globe at the middle.

```sh
cargo run --release -p vale-app -- --layout pad --zoom 0.8 --screenshot globe.png --size 1194x834 --pixel-ratio 3
```

`assets/map.png` is the map of the sample project in the flat view. This command makes it.

```sh
cargo run --release -p vale-app -- --layout pad --map-only --screenshot map.png --size 862x782 --pixel-ratio 2
```
