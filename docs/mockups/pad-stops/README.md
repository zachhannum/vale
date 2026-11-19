# The color stops page of the iPad layout

These files show a proposal for the page of the color stops in the Layers panel of the iPad layout. The proposal replaces only the vertical bar with the leader lines. The list of the bands is the editor, and the histogram above it only shows. The card of a band, the reset row, the row of the color ramps, and "Save as ramp…" stay as they are.

`assets/now.png` is the present page from `vale-app`, at 1194 by 834 points and a pixel ratio of 2, with an imported heightmap. `stops.html` draws the new dock over that picture, in the font of the app, if the system has it. `render.sh` renders the screens with C| Screen | Content |
| --- | --- |
| `now` | The present page. |
| `rest` | The default stops: the histogram of the area at each value, in the colors of the bands, and the list of the bands. In the list, the colors touch, and the value of each stop is on the seam between its two bands. Each row has the name of its band and its share of the area. |
| `many` | The most bands, 32, with the stops close together near sea level. Some bands have names. |
| `many-selected` | The band from 300 m to 400 m is selected. Its card is at the left of the panel, as in the app, and its row has the button that adds a stop in the middle of the band. The histogram dims the other bands. |
| `many-scrub` | A drag up or down on the value of the stop at 200 m. The value stays between the stops next to it, and the histogram marks it. |
