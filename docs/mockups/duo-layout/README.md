# iPhone Duo layout

These files show the layout of Vale on iPhone Duo in each pose, for the globe workspace and the Atlas workspace. The section "iPhone Duo" of `docs/DESIGN.md` gives the layout. `gallery.html` shows the pictures.

Vale keeps the layout of its width in each pose, so each screen starts from a picture of `vale-app` at the size of the pose. `layout.html` draws the changes of iPhone Duo on top: the card that moves away from the outer camera, the gap at the folding region, and the page that fits in one part of a folded screen. A dashed circle is the region of the outer camera. A dashed line is the hinge, and a hatched band is the folding region. The device gives the size and the place of both regions, and the pictures use a camera region of 30 points and a folding region of 28 points.

## Screens

The part of the URL after `#` chooses the screen, for example `layout.html#globe-book`. The file in `screens/` has the same name. Each pose has a screen with the prefix `globe-` and a screen with the prefix `atlas-`.

| Pose | Size in points | Content |
| --- | --- | --- |
| `outer-portrait` | 466 × 678 | The iPhone layout, with the card of the actions at the left of the camera. |
| `outer-landscape` | 678 × 466 | The medium layout, with the card of the world menu at the right of the camera. |
| `inner-portrait` | 669 × 951 | The medium layout, with the hinge. |
| `inner-landscape` | 951 × 669 | The medium layout, with the hinge. |
| `split` | 951 × 669 | Vale as the left app of Split View, with the iPhone layout. |
| `tabletop` | 669 × 951 | Portrait, partially folded. |
| `book` | 951 × 669 | Landscape, partially folded. |

## Render the screens

The pictures in `assets/` come from `vale-app`, at a pixel ratio of 2, with the safe area `0,0,34,0`. The globe screens have the Layers panel, and the globe screens of the iPhone layout have no panel. The Atlas screens have map frame 1 selected. The files with the suffix `-behind` have the dock collapsed, and they show the canvas in the gap at the folding region.

This command renders the screens with Chromium. Run it from the root of the repository. The variable `CHROME` gives the Chromium command, and the default is `chromium`.

```sh
docs/mockups/duo-layout/render.sh
```
