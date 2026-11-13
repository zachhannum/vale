# iPhone Duo layout

These files show the layout of Vale on iPhone Duo in each pose, for the globe workspace and the Atlas workspace. The section "iPhone Duo" of `docs/DESIGN.md` gives the layout. `gallery.html` shows the pictures.

`layout.html` draws the chrome in HTML over pictures from the app. The globe is `../pad-layout/assets/globe.png`, and `assets/map.png` is the map of the sample project. The chrome uses the colors of `ui/pad/theme.rs` and the icon paths of `ui/pad/icons.rs`. The panel rows are sample content.

A dashed line is the hinge. A hatched band is the folding region of a partially folded device. The device gives the size of the region, and the pictures use 28 points.

## Screens

The part of the URL after `#` chooses the screen, for example `layout.html#globe-book`. The file in `screens/` has the same name. Each pose has a screen with the prefix `globe-` and a screen with the prefix `atlas-`.

| Pose | Size in points | Content |
| --- | --- | --- |
| `outer-portrait` | 466 × 678 | The side column at the right edge, and the sheet. |
| `outer-landscape` | 678 × 466 | The side column at the left edge, the tool column at its right, and the sheet. |
| `inner-portrait` | 669 × 951 | The top row, and the sheet open to the hinge. |
| `inner-landscape` | 951 × 669 | The dock at the left of the side column. |
| `split` | 951 × 669 | Vale as the left app of Split View. |
| `tabletop` | 669 × 951 | Portrait, partially folded. |
| `book` | 951 × 669 | Landscape, partially folded. |

The globe screens show the Raise brush and the Layers panel. The Atlas screens show a selected map frame and its Properties panel.

## Render the screens

Run the script from the root of the repository. The variable `CHROME` gives the Chromium command, and the default is `chromium`.

```sh
docs/mockups/duo-layout/render.sh
```

The app draws its UI text in the Ubuntu Light font of egui. If the computer does not have that font, the page uses Noto Sans from `assets/fonts`.

`assets/map.png` comes from this command:

```sh
cargo run --release -p vale-app -- --layout pad --map-only --screenshot map.png --size 862x782 --pixel-ratio 2
```
