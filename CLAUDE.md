# Vale

Vale is a GIS for fictional worlds, for desktop and iPad. You draw the world in the app, on a globe. The design is in `docs/DESIGN.md`. Read it before you plan or write code.

## Current milestone

Milestone 1, the labeling prototype, is done. The `vale-labeler` library labels points and lines from Natural Earth data, and the demo writes a PNG. This command makes the PNG:

```sh
cargo run --release -p vale-labeler-demo -- --preset world --out target/demo/world.png
```

An app prototype also exists. It is a vertical slice through milestones 2, 3, 4, 5, and 7, ahead of the milestone order. None of those milestones is done. This command opens the window:

```sh
cargo run --release -p vale-app
```

This command renders the full UI to a PNG without a window:

```sh
cargo run --release -p vale-app -- --screenshot target/app/ui.png --size 1440x900 --report
```

A globe prototype also exists, in `vale-globe-proto`. It tests pen drawing on a globe and Apple Pencil input through egui, ahead of milestone 9. A first test on an iPad showed smooth painting, pressure, and palm rejection. The app gets 120 pen samples per second and no hover events, as the winit source predicted. `crates/vale-globe-proto/README.md` has the test list. The first command opens the window on the desktop. The second command builds the app and starts it on a connected iPad:

```sh
cargo run --release -p vale-globe-proto
scripts/globe-proto-ipad.sh
```

The app prototype leaves out project files, linked sources, rule-based styles, polygon labels, the atlas, conic and transverse projections, editing tools, and the GPU Vello backend. `README.md` has the full list.

The current milestone is milestone 2, the projection pipeline. It is done when a command-line tool renders one map frame of a custom-radius world to PDF, with clipping and a graticule.

The milestone table in `docs/DESIGN.md` gives the order of the work after that. When a milestone is done, update this section.

## Rules

- The project is one Rust workspace. Put each crate in `crates/`, and give each crate a `vale-` prefix.
- `vale-labeler` must not depend on any other Vale crate. It takes page-space geometry and returns placed text.
- The labeling engine is a clean-room design. Use published research and the public Esri documentation only. Do not read or decompile Esri code.
- The license is `MIT OR Apache-2.0` for all crates.
- Do not name a binary `vale`, because a popular prose linter uses that command name.
- Before you add Vello, Parley, or Krilla as a dependency, make sure of its current release status.
- Procedural world generation is a permanent non-goal.
- If a change contradicts `docs/DESIGN.md`, update the document in the same change.
- Reference files for the demo are updated with `VALE_UPDATE_REFERENCE=1`.
