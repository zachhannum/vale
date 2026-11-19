# Collections

These files show collections in the iPad layout. A collection is an ordered stack of layers, and a layer can be in more than one collection. A view, such as the globe or a map, is an ordered stack of collections. The screens show the Layers panel with a card for each collection, the collection popover, the page of a collection with Add layers…, the Collections checklist of a layer, Polygonize with the Collection row, Edit on globe, and the management page. The screens are 1194 by 834 points, and the narrow screens are 375 by 834 points, at a pixel ratio of 2.

`collections.html` draws the screens in HTML. The globe and the map are pictures from `vale-app` in `../pad-layout/assets`. `render.sh` renders the screens with Chromium into `screens/`. The part of the URL after `#` chooses the screen, and the file in `screens/` has the same name.

The design has these rules:

- Each checked collection is a card in the Layers panel. The header row of the card has the chevron, the name, and the grip. The layer rows are inside the card. A collapsed collection is one header row with its count.
- If a higher collection already draws a layer, the row of that layer is dimmed.
- When a layer leaves its last collection, it goes into World in the same undo step. When the user deletes a collection, each layer that is in no other collection goes into World. In World, a layer that is in no other collection has no Remove action. Delete layer removes it.
- "Edit on globe" opens the World workspace with the collections of the map checked in the order of the map, and with the extent of the map. The card with the map name at the top of the canvas shows this mode. Its close button ends the mode: the extent goes, and the checks stay.

| Screen | Content |
| --- | --- |
| `layers`, `layers-narrow` | Three checked collections as cards. Climate data is collapsed. Roads is lifted in a drag inside Vesperan. |
| `pick` | The collection popover with checkmarks, the Maps switch, New collection…, and Manage collections…. |
| `collection-page` | The page of Vesperan: the name, its layers with Remove on a swipe, Add layers…, and Delete collection. |
| `add-layers`, `add-layers-narrow` | Add layers…: each layer of the project, with its collections on a second line, search, and a checkmark on the layers in Vesperan. A check adds the layer to Vesperan and keeps its other collections. |
| `layer-collections` | The Collections checklist on the page of the layer Rivers. |
| `polygonize`, `polygonize-pick` | Polygonize with the extent Vesperan and the Collection row with the collection name. The picker has "New collection" as a row with a name field. |
| `atlas-edit` | The Frame tab of a map frame with "Edit on globe". |
| `globe-map` | The globe after Edit on globe, or after a check on a map in the Maps side of the popover: the extent, a darker ground outside it, the map card with its close button, and the cards of the map in its order. |
| `pick-maps` | The Maps side of the collection popover. |
| `manage-collections`, `manage-collections-narrow` | The Collections segment of the management page: each collection with its count, search, Rename and Delete on a swipe, the chevron to the page of the collection, and New collection…. |
| `manage-layers`, `manage-layers-narrow` | The Layers segment of the management page: each layer one time with its collections on a second line, search, the chevron to the page of the layer, and Delete on a swipe. |
