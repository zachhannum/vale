# UI mockups

These files are the source of the Vale UI mockups. The mockups show eight desktop screens and fifteen iPad screens.

View the mockups at https://claude.ai/artifact/K7SGPcJf76r1k1HhgvQi8T. The link is private to its owner. The screen files need the Claude Design runtime, so they do not open from this folder.

## Content

| Path | Content |
| --- | --- |
| `gen/gen.py` | Draws the sample regional map, Vesperan, and the atlas page parts. |
| `gen/world.py` | Draws the sample world as an equirectangular map. |
| `gen/globe.py` | Draws the sample world on a globe. |
| `gen/build.py` | Writes the twenty-three screen files. |
| `gen/coast-scan.jpg` | The sketch image on the Sources screen. The artifact holds it as an uploaded asset. |
| `project/` | The published files: screens, `canvas.json`, `vale.css`, and the generated SVG maps. |

The sample world, Kareth, and all names on the maps are invented for the mockups.

## Build

The scripts need Python 3 and Pillow. Run them from this folder.

```sh
python3 gen/gen.py project
python3 gen/world.py project
python3 gen/globe.py project
python3 gen/build.py project
```

Run `gen/build.py` last. Each script writes over its files in `project/`.
