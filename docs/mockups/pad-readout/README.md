# The readout of the iPad layout

These mockups show a proposal for the lower left of the World workspace on the iPad. `readout.html` draws the six states, and `screens/readout.png` is the picture.

1. The present card: the view buttons, the place, and the scale bar.
2. At rest: one View button and the scale bar.
3. While a finger or the Pencil is on the canvas: the place shows above the card, and it fades after the touch ends.
4. The View popover on the globe: Globe or Flat, and the graticule.
5. The View popover on the flat map: also the projection, "Recenter", and "Reset view".
6. A save that failed: a banner at the top, in place of the save mark.

This command makes the picture:

```sh
chrome --headless --no-sandbox --hide-scrollbars --allow-file-access-from-files --force-device-scale-factor=2 --window-size=1180,1500 --screenshot=screens/readout.png "file://$PWD/readout.html"
```
