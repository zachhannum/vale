# iPad floating panels

These files show floating panels for the iPad layout. The panels are panes of one dock on the right edge. A pane can change size and show tabs, and more than one pane can be open at the same time. The screens are 1194 by 834 points at a pixel ratio of 2.

`panels.html` draws the screens in HTML, with the status bar of the system at the top. The globe and the map are pictures from `vale-app` in `../pad-layout/assets`. The chrome uses the colors of `ui/pad/theme.rs`. The rows and the values are sample content.

## Screens

Where the pictures differ from the rules below, they show an earlier state with pins, Close buttons, the center menu, and separate tool and context strips. The part of the URL after `#` chooses the screen. The file in `screens/` has the same name.

| Screen | Content |
| --- | --- |
| `anatomy` | The parts of the dock, of the tool strip, and of the context strip. |
| `globe-tooltip` | A long press on the Carve tool shows its name. |
| `globe-menu` | The Panels menu of the World workspace, with Layers open and pinned. |
| `atlas-menu` | The Panels menu of the Atlas workspace, with three frame panels open. |
| `globe-pinned` | Layers and Attributes pinned, one below the other. |
| `globe-full` | Layers, Attributes, and Brush fill the dock. Layers and Attributes scroll. |
| `globe-wide` | The handle drags the dock to 460 points wide, and each tab shows its name. |
| `globe-tabs` | Layers, Toolbox, and Attributes as tabs of one panel, and the Brush panel below it. |
| `globe-drag` | The Attributes tab moves out of its panel, and the drop targets show. |
| `atlas-tabs` | A selected frame opens one panel with the tabs Frame, Projection, Layers, and Labels. |
| `atlas-split` | Frame, Projection, and Layers as tabs of one panel, and Labels below it. |
| `atlas-empty` | With no frame selected, the pinned frame panels show a hint. |

## Chrome

The chrome sits on the left edge and the right edge of the screen, so the canvas gets more room. Each part touches its edge with no margin, and only its inner corners are round. The top of the screen has the status bar of the system, so no part touches the top edge. The top row is 4 points below the status bar.

- The world menu is at the left end of the top row. It has the Worlds button, with a folder icon, and the workspace switch: a globe button for World and a book button for Atlas.
- Undo, Redo, and the Panels button are at the right end of the top row. The Atlas workspace also has the layout grid and Add there.
- The tool column is on the left edge.
- The dock and the Panels menu are on the right edge.
- The readout is at the lower left of the canvas, at the right of the tool column.

The tool column is 56 points wide. It holds the tool cells, a divider, and the controls of the active tool. The controls are vertical sliders and cells, and the Brush button is last. Each cell shows an icon only. A long press on a tool or a control shows a tooltip with its name at its right side. The tooltip of a tool with modes also says "Hold to change mode".

The readout has no card. It shows the place and a scale bar, with a dark halo. In the World workspace, a view card at its left holds Globe, Flat, and the graticule, and in the flat view Projection, Recenter, and Reset. In the Atlas workspace, the page pill at the bottom center holds the previous page, the page number, the page size, and the next page.

## Dock

The dock is one card on the right edge. It holds the open panes in one column, with a bar between two panes. A pane is a panel: Layers, Attributes, Frame, and the others. Two panes are never side by side.

Each pane has a header with the title or the tabs, and the action of its panel. A tab shows the icon of its panel, and the open tab also shows the name. A pane with one tab shows the icon and the name as its title.

- A new pane opens at the bottom of the dock.
- The dock is as tall as its panes, and it is never taller than the screen. It ends 8 points above the bottom edge. If the panes do not fit, each pane that does not fit gets shorter and scrolls its content. A pane with content that fits keeps its height. A pane is never shorter than its header and two rows.
- A drag of a bar changes the heights of the two panes next to it.
- A handle at the middle of the left edge of the dock changes its width, from 300 to 520 points. During the drag, a label shows the width. The dock is 340 points wide at first.
- In a dock 420 points wide or more, each tab shows its icon and its name.
- A pane stays open until you close it. It keeps its place in the dock, its height, and its tabs. A tap on the canvas closes no pane.
- A long press on a tab or on the title opens a menu with "Close" and the name of the panel. With more than one tab, the menu also has "Move to new pane".
- A selection opens the panes that it needs, and it never closes a pane.
- A drag of the empty part of a header moves the pane to another place in the dock.
- A drag of a tab onto the tab row of another pane adds it as a tab. A drag of a tab to a place in the dock makes it a pane of its own.
- With no pane open, the dock is not there.
- The dock can collapse. It slides off the right edge and keeps its layout. A drag of the width handle past the narrowest width collapses it.
- A slim tab at the middle of the right edge shows the dock again, with a tap or a drag to the left. A panel that opens also shows the dock.
- Each workspace keeps its own dock on the device. The project file does not keep it.

The Panels button opens the Panels menu, which has a tile for each panel of the workspace. A tinted tile is an open panel. A tap on a tile opens the panel in the dock or brings it into view, and a tap on an open tile closes it. A tile is off when its panel does nothing in the workspace. Below the tiles, "Hide dock" or "Show dock" collapses or shows the dock. "Close all" closes each panel, and "Reset layout" closes each panel and puts the dock back in its first state.

The Atlas menu has two groups. Selection holds Frame, Projection, Layers, and Labels. Atlas holds Pages, Maps, Attributes, and Brush.

In the Atlas workspace, the panel of a frame is four panels: Frame, Projection, Layers, and Labels. A selected frame opens them in the arrangement of the last time, and the first arrangement is one panel with four tabs. The panels show the selected frame. With no frame selected, each frame panel shows a hint.

On a compact screen, a panel is a sheet, and its tabs stay. The sheet has a close button, and a long press on a tab opens the menu with "Close".

## Render the screens

This command renders one screen with Chromium. Run it from this folder, and replace the screen name.

```sh
chromium --headless --no-sandbox --allow-file-access-from-files --hide-scrollbars \
  --force-device-scale-factor=2 --window-size=1194,834 --virtual-time-budget=2000 \
  --screenshot="$PWD/screens/globe-pinned.png" "file://$PWD/panels.html#globe-pinned"
```
