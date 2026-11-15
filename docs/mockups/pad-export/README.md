# Export in the Atlas workspace

These mockups show a proposal for the export command and its options. `export.html` draws the screens, and `screens/export.png` is the picture. The pictures in `assets/` come from the app, and the menus, the forms, and the sheets on top of them are HTML.

1. Now: the export rows at the bottom of the Pages panel.
2. The world menu with the row "Export…".
3. The export form with PDF and all pages.
4. The export form with PNG and one page.
5. The export form with a range of pages.
6. The export form while the export runs.
7. The share sheet of the system after the export.
8. The same steps on an iPhone 16 in portrait.
9. File › Export… and the export sheet of the desktop layout.

This command makes the picture:

```sh
google-chrome --headless --no-sandbox --hide-scrollbars --allow-file-access-from-files --force-device-scale-factor=2 --window-size=1234,8190 --screenshot=screens/export.png "file://$PWD/export.html"
```
