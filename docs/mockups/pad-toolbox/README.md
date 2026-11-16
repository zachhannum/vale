# iPad Toolbox panel

These files show the Toolbox panel of the iPad layout. The screens are 1194 by 834 points at a pixel ratio of 2.

`toolbox.html` draws the screens in HTML. The globe is a picture from `vale-app` in `../pad-layout/assets`. The panel uses the colors of `ui/pad/theme.rs`. `render.sh` renders the screens with Chromium into `screens/`. The part of the URL after `#` chooses the screen.

| Screen | Content |
| --- | --- |
| `grid` | The search field and a tile for each operation of version 1, in the groups Terrain, Lines, and Polygons. |
| `page` | The page of "Orient lines downhill", with its inputs, Output, Run, and the result of the last run. |
