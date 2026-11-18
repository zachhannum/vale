# iPad strip sliders of the brush

These files show how you choose the two sliders that the tool column shows for a brush mode. The screens are 1194 by 834 points at a pixel ratio of 2.

`strip.html` draws the Brush panel and the strip sliders in HTML over a picture from `vale-app` in `assets`. The picture shows the Raise mode on a height layer. `render.sh` renders the screens with Chromium into `screens/`. The part of the URL after `#` chooses the screen.

| Screen | Content |
| --- | --- |
| `panel` | A pin after the label of each slider. The pins of Size and Strength are in, so the strip shows those two sliders. |
| `swap` | A tap on the label of Hardness. Hardness replaces Strength in the strip. |
| `reset` | The pins of Raise differ from its default pair, so the header has Reset pins. |
